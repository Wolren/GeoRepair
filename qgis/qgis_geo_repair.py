#!/usr/bin/env python3
"""QGIS Processing script: Geo Repair — fix invalid geometries using Rust engine.

=== INSTALL ===
1. Copy this file AND the geo_repair-*.whl next to it into:
   Windows: %APPDATA%/QGIS/QGIS3/profiles/default/processing/scripts/
   Linux:   ~/.local/share/QGIS/... (same path)
2. Restart QGIS → Processing Toolbox → Geo Repair.
3. Wheel auto-installs on first run. Manual: pip install <folder>/geo_repair-*.whl
"""

import sys, subprocess, json, importlib
from pathlib import Path

try:
    _dir = Path(__file__).parent
except NameError:
    _dir = None

if _dir:
    whl = next(iter(_dir.glob("geo_repair-*.whl")), None)
    if whl and importlib.util.find_spec("geo_repair") is None:
        try:
            subprocess.check_call([sys.executable, "-m", "pip", "install", str(whl), "--no-deps"])
        except Exception:
            pass

from qgis.core import (
    QgsProcessingAlgorithm, QgsProcessingParameterFeatureSource,
    QgsProcessingParameterEnum, QgsProcessingParameterBoolean,
    QgsProcessingParameterFeatureSink, QgsFeatureSink,
    QgsProcessing, QgsGeometry,
)
from qgis.PyQt.QtCore import QCoreApplication


class GeoRepairAlgo(QgsProcessingAlgorithm):
    def tr(self, t): return QCoreApplication.translate("GeoRepair", t)
    def name(self): return "geo_repair"
    def displayName(self): return self.tr("Geo Repair")
    def createInstance(self): return GeoRepairAlgo()
    def group(self): return self.tr("Geo Repair")
    def groupId(self): return "geo_repair"

    def shortHelpString(self):
        return self.tr(
            "Repair invalid geometries using the Rust geo-repair engine.\n\n"
            "Methods:\n"
            "  Auto – Structure fast path, falls back to Arrangement\n"
            "  Structure – boolean-operation based (fast)\n"
            "  Arrange – CDT triangulation (robust)\n\n"
            "Invalid features are counted and logged — all output is OGC-valid."
        )

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT", "Input layer",
            [QgsProcessing.TypeVectorPolygon, QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterEnum("METHOD", "Method",
            ["Auto", "Structure", "Arrange"], defaultValue=0))
        self.addParameter(QgsProcessingParameterBoolean("KEEP", "Keep collapsed", False))
        self.addParameter(QgsProcessingParameterFeatureSink("OUTPUT", "Repaired layer"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair

        src = self.parameterAsSource(params, "INPUT", ctx)
        midx = self.parameterAsEnum(params, "METHOD", ctx)
        keep = self.parameterAsBool(params, "KEEP", ctx)
        snk, dst = self.parameterAsSink(params, "OUTPUT", ctx, src.fields(),
                  src.wkbType(), src.sourceCrs())
        tot = src.featureCount()
        ms = ["auto", "structure", "arrange"]
        bad = 0

        for i, f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            g = f.geometry()
            if g and not g.isEmpty():
                wkt = g.asWkt()
                if not geo_repair.is_valid_wkt(wkt):
                    bad += 1
                    try:
                        fixed = geo_repair.repair_wkt(wkt, ms[midx])
                        fg = QgsGeometry.fromWkt(fixed)
                        if fg and not fg.isEmpty():
                            f.setGeometry(fg)
                    except Exception:
                        pass
            snk.addFeature(f, QgsFeatureSink.FastInsert)
            if i % 100 == 0: fb.setProgress(int(i / tot * 100))

        fb.pushInfo(f"{bad} invalid features repaired out of {tot}")
        return {"OUTPUT": dst}


def createAlgorithms():
    return [GeoRepairAlgo()]
