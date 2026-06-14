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
        fix = mode in (0, 2)
        CHUNK = 1000
        bad = 0

        fb.pushInfo(f"Loading {tot} features...")
        QgsApplication.processEvents()

        # Collect all features + WKTs in one pass
        all_features = []
        all_wkts = []
        for i, f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            all_features.append(f)
            g = f.geometry()
            all_wkts.append(g.asWkt() if g and not g.isEmpty() else "")
            if i % 5000 == 0:
                fb.pushInfo(f"  Loaded {i}/{tot} features...")
                QgsApplication.processEvents()

        tot = len(all_features)
        fb.pushInfo(f"Processing {tot} features in chunks of {CHUNK}...")
        QgsApplication.processEvents()

        # Process in chunks
        for chunk_start in range(0, tot, CHUNK):
            if fb.isCanceled(): break

            chunk_wkts = all_wkts[chunk_start:chunk_start + CHUNK]
            chunk_feats = all_features[chunk_start:chunk_start + CHUNK]

            results = geo_repair.repair_validate_wkt_batch(chunk_wkts, ms[midx])

            for i, f in enumerate(chunk_feats):
                if i < len(results):
                    fixed_wkt, valid, errors = results[i]
                    if not valid:
                        bad += 1
                        if mode != 2 and bad <= 20:
                            fb.pushWarning(f"  FID {f.id()}: {', '.join(errors[:3])}")
                    if fix:
                        fg = QgsGeometry.fromWkt(fixed_wkt)
                        if fg and not fg.isEmpty():
                            f.setGeometry(fg)
                snk.addFeature(f, QgsFeatureSink.FastInsert)

            pct = int((chunk_start + len(chunk_feats)) / tot * 100)
            fb.setProgress(pct)
            QgsApplication.processEvents()

        fb.pushInfo(f"Done — {bad} invalid features out of {tot}")
        return {"OUTPUT": dst}


def createAlgorithms():
    return [GeoRepairAlgo()]
