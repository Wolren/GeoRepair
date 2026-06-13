#!/usr/bin/env python3
"""QGIS Processing script: Geo Repair — fix invalid geometries using Rust engine.

=== INSTALL ===
1. Copy this file AND the geo_repair-*.whl file next to it into your
   Processing scripts folder:
   Windows: %APPDATA%/QGIS/QGIS3/profiles/default/processing/scripts/
   Linux:   ~/.local/share/QGIS/QGIS3/profiles/default/processing/scripts/
   macOS:   ~/Library/Application Support/QGIS/QGIS3/profiles/default/processing/scripts/
2. Restart QGIS — the algorithms appear in Processing Toolbox > Geo Repair.

The wheel auto-installs on first run (requires pip). Manual install:
   pip install <script_folder>/geo_repair-*.whl --no-deps
"""

import os, sys, subprocess, json, importlib
from pathlib import Path

# ---------- auto-install (wrapped for Script Editor compat) ----------
try:
    _script_dir = Path(__file__).parent
except NameError:
    _script_dir = None

if _script_dir:
    _wheels = list(_script_dir.glob("geo_repair-*.whl"))
    if _wheels and importlib.util.find_spec("geo_repair") is None:
        try:
            subprocess.check_call(
                [sys.executable, "-m", "pip", "install", str(_wheels[0]), "--no-deps"]
            )
        except Exception:
            pass  # silently skip — user can pip install manually

from qgis.core import (
    QgsProcessingAlgorithm, QgsProcessingParameterFeatureSource,
    QgsProcessingParameterEnum, QgsProcessingParameterBoolean,
    QgsProcessingParameterFileDestination, QgsProcessingFeatureSink,
    QgsProcessing,
)
from qgis.PyQt.QtCore import QCoreApplication


class RepAlgo(QgsProcessingAlgorithm):
    def tr(self, t): return QCoreApplication.translate("GeoRepair", t)
    def name(self): return "repair"
    def displayName(self): return self.tr("Repair geometries")
    def createInstance(self): return RepAlgo()
    def group(self): return self.tr("Geo Repair")
    def groupId(self): return "geo_repair"

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT", "Input layer",
            [QgsProcessing.TypeVectorPolygon, QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterEnum("METHOD", "Method",
            ["Auto", "Structure", "Arrange"], defaultValue=0))
        self.addParameter(QgsProcessingParameterBoolean("KEEP", "Keep collapsed", False))
        self.addParameter(QgsProcessingParameterFeatureSink("OUTPUT", "Repaired"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair
        src = self.parameterAsSource(params, "INPUT", ctx)
        midx = self.parameterAsEnum(params, "METHOD", ctx)
        snk, dst = self.parameterAsSink(params, "OUTPUT", ctx, src.fields(),
                  src.wkbType(), src.sourceCrs())
        tot = src.featureCount()
        ms = ["auto", "structure", "arrange"]

        for i, f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            g = f.geometry()
            if g and not g.isEmpty():
                try:
                    fw = geo_repair.repair_wkt(g.asWkt(), ms[midx])
                    fg = QgsGeometry.fromWkt(fw)
                    if fg and not fg.isEmpty():
                        f.setGeometry(fg)
                except Exception:
                    pass
            snk.addFeature(f, QgsFeatureSink.FastInsert)
            if i % 100 == 0:
                fb.setProgress(int(i / tot * 100))

        fb.pushInfo(f"Done: {tot} features processed")
        return {"OUTPUT": dst}


class ValAlgo(QgsProcessingAlgorithm):
    def tr(self, t): return QCoreApplication.translate("GeoRepair", t)
    def name(self): return "validate"
    def displayName(self): return self.tr("Validate geometries")
    def createInstance(self): return ValAlgo()
    def group(self): return self.tr("Geo Repair")
    def groupId(self): return "geo_repair"

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT", "Input layer",
            [QgsProcessing.TypeVectorPolygon, QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterFileDestination("OUTPUT", "Error report",
            "JSON files (*.json)"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair
        src = self.parameterAsSource(params, "INPUT", ctx)
        out = self.parameterAsFileOutput(params, "OUTPUT", ctx)
        tot = src.featureCount()
        errors = []

        for i, f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            g = f.geometry()
            if g and not g.isEmpty():
                w = g.asWkt()
                if not geo_repair.is_valid_wkt(w):
                    errors.append({
                        "fid": f.id(),
                        "wkt": w[:100],
                        "errors": geo_repair.validate_wkt(w),
                    })
            if i % 100 == 0:
                fb.setProgress(int(i / tot * 100))

        with open(out, "w") as fh:
            json.dump(errors, fh, indent=2)
        fb.pushInfo(f"{len(errors)} invalid features out of {tot}")
        return {"OUTPUT": out}


def createAlgorithms():
    return [RepAlgo(), ValAlgo()]
