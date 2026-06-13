#!/usr/bin/env python3
"""
QGIS Processing script: Geo Repair — fix invalid geometries using Rust engine.
Drop this file into your QGIS Processing scripts folder (Settings → Processing → Scripts).
The first run auto-installs the bundled geo_repair wheel; requires QGIS restart.
"""

import os, sys, subprocess, json
from pathlib import Path
from qgis.core import (
    QgsProcessingAlgorithm, QgsProcessingParameterFeatureSource,
    QgsProcessingParameterEnum, QgsProcessingParameterBoolean,
    QgsProcessingParameterFileDestination, QgsProcessingOutputVectorLayer,
    QgsFeatureSink, QgsProcessing, QgsProcessingException,
    QgsCoordinateTransform, QgsProject, QgsFeature, QgsGeometry,
    QgsProcessingFeatureSourceDefinition,
)
from qgis.PyQt.QtCore import QCoreApplication

# ---------- auto-install ----------
_wheel = Path(__file__).parent / "geo_repair-0.1.0-cp312-cp312-win_amd64.whl"
if _wheel.exists():
    try:
        import geo_repair  # noqa
    except ImportError:
        py = Path(sys.executable).parent / "python3.exe" or "python"
        subprocess.check_call([str(py), "-m", "pip", "install", str(_wheel), "--quiet"])

# ---------- algorithms ----------
class RepAlgo(QgsProcessingAlgorithm):
    def tr(self, t): return QCoreApplication.translate("GeoRepair", t)
    def name(self): return "repair"
    def displayName(self): return self.tr("Repair geometries")
    def createInstance(self): return RepAlgo()
    def group(self): return self.tr("Geo Repair")
    def groupId(self): return "geo_repair"

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT", "Input",
            [QgsProcessing.TypeVectorPolygon,QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterEnum("METHOD", "Method",
            ["Auto","Structure","Arrange"], defaultValue=0))
        self.addParameter(QgsProcessingParameterBoolean("KEEP","Keep collapsed",False))
        self.addParameter(QgsProcessingParameterFeatureSink("OUTPUT","Repaired"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair
        src = self.parameterAsSource(params,"INPUT",ctx)
        midx = self.parameterAsEnum(params,"METHOD",ctx)
        keep = self.parameterAsBool(params,"KEEP",ctx)
        snk,dst = self.parameterAsSink(params,"OUTPUT",ctx,src.fields(),
                  src.wkbType(),src.sourceCrs())
        tot = src.featureCount()
        ms = ["auto","structure","arrange"]
        for i,f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            g = f.geometry()
            if g and not g.isEmpty():
                try:
                    fw = geo_repair.repair_wkt(g.asWkt(),ms[midx])
                    fg = QgsGeometry.fromWkt(fw)
                    if fg and not fg.isEmpty(): f.setGeometry(fg)
                except: pass
            snk.addFeature(f,QgsFeatureSink.FastInsert)
            if i%100==0: fb.setProgress(int(i/tot*100))
        return {"OUTPUT":dst}

class ValAlgo(QgsProcessingAlgorithm):
    def tr(self, t): return QCoreApplication.translate("GeoRepair", t)
    def name(self): return "validate"
    def displayName(self): return self.tr("Validate geometries")
    def createInstance(self): return ValAlgo()
    def group(self): return self.tr("Geo Repair")
    def groupId(self): return "geo_repair"

    def initAlgorithm(self, c=None):
        self.addParameter(QgsProcessingParameterFeatureSource("INPUT","Input",
            [QgsProcessing.TypeVectorPolygon,QgsProcessing.TypeVectorLine]))
        self.addParameter(QgsProcessingParameterFileDestination("OUTPUT","Error report","JSON files (*.json)"))

    def processAlgorithm(self, params, ctx, fb):
        import geo_repair
        src = self.parameterAsSource(params,"INPUT",ctx)
        out = self.parameterAsFileOutput(params,"OUTPUT",ctx)
        tot = src.featureCount(); errors=[]
        for i,f in enumerate(src.getFeatures()):
            if fb.isCanceled(): break
            g = f.geometry()
            if g and not g.isEmpty():
                w = g.asWkt()
                if not geo_repair.is_valid_wkt(w):
                    e = geo_repair.validate_wkt(w)
                    errors.append({"fid":f.id(),"wkt":w[:100],"errors":e})
            if i%100==0: fb.setProgress(int(i/tot*100))
        with open(out,"w") as fh: json.dump(errors,fh,indent=2)
        fb.pushInfo(f"{len(errors)} invalid features out of {tot}")
        return {"OUTPUT":out}

# ---------- registration ----------
def createAlgorithms():
    return [RepAlgo(), ValAlgo()]
