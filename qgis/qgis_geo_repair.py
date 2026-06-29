#!/usr/bin/env python3
"""QGIS Processing script: Geo Repair — fix invalid geometries using Rust engine.

=== INSTALL ===
1. Copy this file AND the geo_repair-*.whl next to it into:
   Windows: %APPDATA%/QGIS/QGIS3/profiles/default/processing/scripts/
   Linux:   ~/.local/share/QGIS/... (same path)
2. Restart QGIS -> Processing Toolbox -> Geo Repair.
3. Wheel auto-installs on first run. Manual: pip install <folder>/geo_repair-*.whl

=== BEST PRACTICES ===
- Batched WKB streaming — iterates features one-by-one via QGIS, batches WKBs
  into chunks, and sends them to the Rust engine.  Memory is O(1).
- UI stays responsive — processEvents() is called inside batch processing so
  the progress bar updates and cancellation works mid-batch.
- Cancellation is checked before and during every batch.
"""

from qgis.core import (
    QgsProcessingAlgorithm,
    QgsProcessingParameterFeatureSource,
    QgsProcessingParameterEnum,
    QgsProcessingParameterVectorDestination,
    QgsFeatureSink,
    QgsProcessing,
    QgsProcessingContext,
    QgsGeometry,
    QgsApplication,
    QgsFeatureRequest,
    Qgis,
)
from qgis.PyQt.QtCore import QCoreApplication


_BATCH_SIZE = 500


class GeoRepairAlgo(QgsProcessingAlgorithm):
    def tr(self, t):
        return QCoreApplication.translate("GeoRepair", t)

    def name(self):
        return "geo_repair"

    def displayName(self):
        return self.tr("Geo Repair")

    def createInstance(self):
        return GeoRepairAlgo()

    def group(self):
        return self.tr("Geo Repair")

    def groupId(self):
        return "geo_repair"

    def shortHelpString(self):
        return self.tr(
            "Repair invalid geometries using the Rust geo-repair engine.\n\n"
            "Modes:\n"
            "  Diagnose + Repair - reports all errors then fixes them (default)\n"
            "  Diagnose only    - reports errors without modifying\n"
            "  Repair only      - silently fixes, no diagnostic output\n\n"
            "Methods:\n"
            "  Auto       - Structure fast path, falls back to Arrangement\n"
            "  Structure  - boolean-operation based (fast)\n"
            "  Arrange    - CDT triangulation (robust)\n\n"
            "All output is guaranteed OGC-valid.\n\n"
            "Memory: O(1) - uses batched WKB streaming, never loads entire "
            "dataset into memory."
        )

    def initAlgorithm(self, c=None):
        self.addParameter(
            QgsProcessingParameterFeatureSource(
                "INPUT",
                "Input layer",
                [QgsProcessing.TypeVectorPolygon, QgsProcessing.TypeVectorLine],
            )
        )
        self.addParameter(
            QgsProcessingParameterEnum(
                "MODE",
                "Mode",
                ["Diagnose + Repair", "Diagnose only", "Repair only"],
                defaultValue=0,
            )
        )
        self.addParameter(
            QgsProcessingParameterEnum(
                "METHOD", "Method", ["Auto", "Structure", "Arrange"], defaultValue=0
            )
        )
        self.addParameter(
            QgsProcessingParameterVectorDestination("OUTPUT", "Output layer")
        )

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair

        no_check = self._invalid_geometry_check_no_check()
        ctx.setInvalidGeometryCheck(no_check)

        src = self.parameterAsSource(params, "INPUT", ctx)
        mode = self.parameterAsEnum(params, "MODE", ctx)
        midx = self.parameterAsEnum(params, "METHOD", ctx)
        ms = ["auto", "structure", "arrange"]
        is_diagnose = mode == 1
        is_repair_only = mode == 2

        tot = src.featureCount() or 0
        fb.pushInfo(
            "Processing %d features via batched WKB streaming (batch=%d)\u2026"
            % (tot, _BATCH_SIZE)
        )

        snk, dst = self.parameterAsSink(
            params, "OUTPUT", ctx, src.fields(), src.wkbType(), src.sourceCrs()
        )

        self._disable_geom_checks(ctx)
        freq = QgsFeatureRequest().setInvalidGeometryCheck(no_check)

        has_parallel = hasattr(geo_repair, "par_repair_wkb_batch")
        has_diag_batch = hasattr(geo_repair, "repair_validate_wkb_batch")
        total_diags = []
        processed = 0

        batch_wkbs = []
        batch_fids = []
        batch_feats = []

        def yield_to_ui():
            QgsApplication.processEvents()

        def check_cancel():
            if fb.isCanceled():
                raise RuntimeError("Canceled by user")

        def update_progress():
            nonlocal processed
            if tot > 0:
                fb.setProgress(int(processed / tot * 100))
            yield_to_ui()

        def flush_batch():
            nonlocal batch_wkbs, batch_fids, batch_feats, processed, total_diags
            if not batch_wkbs:
                return

            check_cancel()

            try:
                if is_diagnose:
                    if has_diag_batch:
                        results = geo_repair.repair_validate_wkb_batch(
                            batch_wkbs, ms[midx]
                        )
                        total_diags.extend((v, e) for _, v, e in results)
                    else:
                        for wkb in batch_wkbs:
                            _, valid, errors = geo_repair.repair_validate_wkb(
                                wkb, ms[midx]
                            )
                            total_diags.append((valid, errors))
                            yield_to_ui()

                elif is_repair_only:
                    if has_parallel and len(batch_wkbs) >= 4:
                        results = geo_repair.par_repair_wkb_batch(batch_wkbs, ms[midx])
                    else:
                        results = geo_repair.repair_wkb_batch(batch_wkbs, ms[midx])
                    for feat, out_wkb in zip(batch_feats, results):
                        if out_wkb:
                            self._set_feature_geom(feat, out_wkb)
                        snk.addFeature(feat, QgsFeatureSink.FastInsert)

                else:
                    if has_diag_batch:
                        results = geo_repair.repair_validate_wkb_batch(
                            batch_wkbs, ms[midx]
                        )
                        for feat, (out_wkb, valid, errors) in zip(batch_feats, results):
                            total_diags.append((valid, errors))
                            if out_wkb:
                                self._set_feature_geom(feat, out_wkb)
                            snk.addFeature(feat, QgsFeatureSink.FastInsert)
                    elif has_parallel and len(batch_wkbs) >= 4:
                        out_wkbs = geo_repair.par_repair_wkb_batch(batch_wkbs, ms[midx])
                        for feat, out_wkb in zip(batch_feats, out_wkbs):
                            if out_wkb:
                                self._set_feature_geom(feat, out_wkb)
                            snk.addFeature(feat, QgsFeatureSink.FastInsert)
                    else:
                        for feat, wkb in zip(batch_feats, batch_wkbs):
                            out_wkb, valid, errors = geo_repair.repair_validate_wkb(
                                wkb, ms[midx]
                            )
                            total_diags.append((valid, errors))
                            if out_wkb:
                                self._set_feature_geom(feat, out_wkb)
                            snk.addFeature(feat, QgsFeatureSink.FastInsert)
                            yield_to_ui()

            except RuntimeError:
                raise
            except Exception as e:
                fb.reportError("Batch error: %s" % e)

            processed += len(batch_wkbs)
            batch_wkbs = []
            batch_fids = []
            batch_feats = []

        for f in src.getFeatures(freq):
            check_cancel()

            wkb = self._extract_wkb(f)
            batch_wkbs.append(wkb)
            batch_fids.append(f.id())
            batch_feats.append(f)

            if len(batch_wkbs) >= _BATCH_SIZE:
                flush_batch()
                update_progress()

        flush_batch()
        update_progress()
        fb.setProgress(100)

        self._report_diagnostics(fb, mode, total_diags, tot)
        return {"OUTPUT": dst}

    @staticmethod
    def _invalid_geometry_check_no_check():
        try:
            return Qgis.InvalidGeometryCheck.NoCheck
        except AttributeError:
            return QgsProcessingContext.InvalidGeometryCheck.NoCheck

    @staticmethod
    def _disable_geom_checks(ctx):
        try:
            nc = QgsProcessingContext.InvalidGeometryCheck.NoCheck
            ctx.setInvalidGeometryCheck(nc)
        except (AttributeError, TypeError):
            pass

    @staticmethod
    def _extract_wkb(f):
        g = f.geometry()
        if g and not g.isEmpty():
            data = g.asWkb()
            if data:
                return bytes(data)
        return b""

    @staticmethod
    def _set_feature_geom(feat, wkb_bytes):
        g = QgsGeometry.fromWkb(wkb_bytes)
        if g and not g.isEmpty():
            feat.setGeometry(g)

    @staticmethod
    def _report_diagnostics(fb, mode, diags, tot):
        bad = sum(1 for v, _ in diags if not v) if diags else 0
        if mode == 2:
            fb.pushInfo("Done \u2014 repaired %d features" % tot)
        else:
            warned = 0
            for i, (valid, errors) in enumerate(diags):
                if not valid and warned < 20:
                    fb.pushWarning("  Feature %d: %s" % (i, ", ".join(errors[:3])))
                    warned += 1
            fb.pushInfo("Done \u2014 %d invalid features out of %d" % (bad, tot))


def createAlgorithms():
    return [GeoRepairAlgo()]
