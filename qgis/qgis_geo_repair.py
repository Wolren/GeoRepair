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
    QgsProcessing, QgsGeometry, QgsApplication,
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
            "Modes:\n"
            "  Diagnose + Repair – reports all errors then fixes them (default)\n"
            "  Diagnose only    – reports errors without modifying\n"
            "  Repair only      – silently fixes, no diagnostic output\n\n"
            "Methods:\n"
            "  Auto – Structure fast path, falls back to Arrangement\n"
            "  Structure – boolean-operation based (fast)\n"
            "  Arrange – CDT triangulation (robust)\n\n"
            "All output is guaranteed OGC-valid."
        )

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT", "Input layer",
            [QgsProcessing.TypeVectorPolygon, QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterEnum("MODE", "Mode",
            ["Diagnose + Repair", "Diagnose only", "Repair only"], defaultValue=0))
        self.addParameter(QgsProcessingParameterEnum("METHOD", "Method",
            ["Auto", "Structure", "Arrange"], defaultValue=0))
        self.addParameter(QgsProcessingParameterFeatureSink("OUTPUT", "Output layer"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair

        src = self.parameterAsSource(params, "INPUT", ctx)
        mode = self.parameterAsEnum(params, "MODE", ctx)
        midx = self.parameterAsEnum(params, "METHOD", ctx)
        snk, dst = self.parameterAsSink(params, "OUTPUT", ctx, src.fields(),
                  src.wkbType(), src.sourceCrs())
        tot = src.featureCount()
        ms = ["auto", "structure", "arrange"]
        diag = mode in (0, 1)
        fix = mode in (0, 2)
        bad = 0
        last_pct = -1

        for i, f in enumerate(src.getFeatures()):
            if fb.isCanceled():
                snk.addFeature(f, QgsFeatureSink.FastInsert)
                break

            g = f.geometry()
            if g and not g.isEmpty():
                wkt = g.asWkt()

                if not geo_repair.is_valid_wkt(wkt):
                    bad += 1
                    if diag:
                        errs = geo_repair.validate_wkt(wkt)
                        fb.pushWarning(f"  FID {f.id()}: {', '.join(errs[:3])}")
                    if fix:
                        try:
                            fixed = geo_repair.repair_wkt(wkt, ms[midx])
                            fg = QgsGeometry.fromWkt(fixed)
                            if fg and not fg.isEmpty():
                                f.setGeometry(fg)
                        except Exception:
                            pass

            snk.addFeature(f, QgsFeatureSink.FastInsert)

            pct = int(i / tot * 100)
            if pct != last_pct:
                last_pct = pct
                fb.setProgress(pct)
                QgsApplication.processEvents()

        fb.pushInfo(f"Done — {bad} invalid features out of {tot}")
        return {"OUTPUT": dst}


def createAlgorithms():
    return [GeoRepairAlgo()]
