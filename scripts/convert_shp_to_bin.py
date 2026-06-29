"""Convert SHP file to GeoRepair binary format.

Usage:
    python scripts/convert_shp_to_bin.py input.shp output.bin

Requires: geopandas, numpy
"""

import sys
import struct
import numpy as np


def ring_coords(ring) -> list:
    """Extract coordinate pairs from a shapely ring/linestring."""
    if ring is None or ring.is_empty:
        return []
    return list(ring.coords)


def write_ring(f, ring):
    """Write a single ring (exterior or interior) to binary."""
    coords = ring_coords(ring)
    f.write(struct.pack("<I", len(coords)))
    for x, y in coords:
        f.write(struct.pack("<dd", x, y))


def convert_shp_to_bin(shp_path: str, bin_path: str):
    """Read SHP with geopandas, write custom binary format."""
    import geopandas as gpd

    gdf = gpd.read_file(shp_path)
    polys = []

    for geom in gdf.geometry:
        if geom is None or geom.is_empty:
            continue
        if geom.geom_type == "Polygon":
            polys.append(geom)
        elif geom.geom_type == "MultiPolygon":
            polys.extend(list(geom.geoms))
        # skip other geometry types

    with open(bin_path, "wb") as f:
        f.write(struct.pack("<I", len(polys)))
        for poly in polys:
            exterior = poly.exterior
            write_ring(f, exterior)
            interiors = poly.interiors
            f.write(struct.pack("<I", len(interiors)))
            for ring in interiors:
                write_ring(f, ring)

    print(f"Wrote {len(polys)} polygons to {bin_path}")
    print(f"  Input SHP: {shp_path}")


if __name__ == "__main__":
    if len(sys.argv) != 3:
        print(
            "Usage: python convert_shp_to_bin.py <input.shp> <output.bin>",
            file=sys.stderr,
        )
        sys.exit(1)
    convert_shp_to_bin(sys.argv[1], sys.argv[2])
