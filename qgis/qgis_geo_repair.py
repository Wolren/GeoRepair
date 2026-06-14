#!/usr/bin/env python3
"""QGIS Processing script: Geo Repair — fix invalid geometries using Rust engine.

=== INSTALL ===
1. Copy this file AND the geo_repair-*.whl next to it into:
   Windows: %APPDATA%/QGIS/QGIS3/profiles/default/processing/scripts/
   Linux:   ~/.local/share/QGIS/... (same path)
2. Restart QGIS → Processing Toolbox → Geo Repair.
3. Wheel auto-installs on first run. Manual: pip install <folder>/geo_repair-*.whl
"""

import sys, os
from qgis.core import (
    QgsProcessingAlgorithm, QgsProcessingParameterFeatureSource,
    QgsProcessingParameterEnum,
    QgsProcessingParameterVectorDestination, QgsFeatureSink,
    QgsProcessing, QgsProcessingContext,
    QgsGeometry, QgsApplication, QgsFeatureRequest, Qgis,
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
        self.addParameter(QgsProcessingParameterVectorDestination("OUTPUT", "Output layer"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair
        try:
            _NO_CHECK = Qgis.InvalidGeometryCheck.NoCheck
        except AttributeError:
            _NO_CHECK = QgsProcessingContext.InvalidGeometryCheck.NoCheck
        try:
            ctx.setInvalidGeometryCheck(_NO_CHECK)
        except (AttributeError, TypeError):
            pass

        src = self.parameterAsSource(params, "INPUT", ctx)
        mode = self.parameterAsEnum(params, "MODE", ctx)
        midx = self.parameterAsEnum(params, "METHOD", ctx)
        ms = ["auto", "structure", "arrange"]
        rust_modes = ["both", "validate", "repair"]

        from qgis.core import QgsProcessingUtils
        raw = self.parameterAsString(params, "INPUT", ctx) or ""
        layer = QgsProcessingUtils.mapLayerFromString(raw, ctx)
        src_path = layer.source().split("|")[0].split("?")[0] if layer else ""
        if src_path and os.path.splitext(src_path)[1].lower() in (".dbf", ".shx"):
            shp = os.path.splitext(src_path)[0] + ".shp"
            if os.path.isfile(shp):
                src_path = shp
        use_file = bool(src_path) and os.path.isfile(src_path)

        if use_file:
            output_path = self.parameterAsOutputLayer(params, "OUTPUT", ctx)
            fb.pushInfo("Processing via Rust engine...")

            def on_progress(pct):
                fb.setProgress(int(pct))
                QgsApplication.processEvents()
                if fb.isCanceled():
                    raise RuntimeError("Canceled by user")

            try:
                count, diags = geo_repair.repair_file_to_file(
                    src_path, output_path, ms[midx], rust_modes[mode], on_progress)
            except RuntimeError:
                fb.reportError("Canceled")
                return {"OUTPUT": ""}

            if mode != 2:
                bad = 0
                warned = 0
                for i, (valid, errors) in enumerate(diags):
                    if not valid:
                        bad += 1
                        if warned < 20:
                            fb.pushWarning(f"  Feature {i}: {', '.join(errors[:3])}")
                            warned += 1
                    if i % 500 == 0:
                        QgsApplication.processEvents()
                fb.pushInfo(f"Done — {bad} invalid features out of {count}")
            else:
                fb.pushInfo(f"Done — repaired {count} features")

        else:
            tot = src.featureCount()
            fb.pushInfo(f"Processing {tot} features...")
            snk, dst = self.parameterAsSink(params, "OUTPUT", ctx,
                      src.fields(), src.wkbType(), src.sourceCrs())
            output_path = dst
            freq = QgsFeatureRequest().setInvalidGeometryCheck(_NO_CHECK)
            feats = list(src.getFeatures(freq))
            wkbs = [f.geometry().asWkb().data() if f.geometry() and not f.geometry().isEmpty()
                    else b"" for f in feats]
            results = geo_repair.repair_validate_wkb_batch(wkbs, ms[midx])

            bad = 0
            for i, f in enumerate(feats):
                if fb.isCanceled():
                    break
                fixed_wkb, valid, errors = results[i]
                if not valid:
                    bad += 1
                    if mode != 2 and bad <= 20:
                        fb.pushWarning(f"  FID {f.id()}: {', '.join(errors[:3])}")
                if mode != 1 and fixed_wkb:
                    fg = QgsGeometry()
                    fg.fromWkb(fixed_wkb)
                    if not fg.isEmpty():
                        f.setGeometry(fg)
                snk.addFeature(f, QgsFeatureSink.FastInsert)
                if i % 100 == 0:
                    fb.setProgress(int(i / tot * 100))
                    QgsApplication.processEvents()

            fb.setProgress(100)
            fb.pushInfo(f"Done — {bad} invalid features out of {tot}")

        return {"OUTPUT": output_path}


def createAlgorithms():
    return [GeoRepairAlgo()]
