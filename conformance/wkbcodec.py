"""WKB parsing and emitting for the conformance harness.

Independent of the crate under test: this reads WKB in either byte order and either type-code
form, keeps the dimensions, and can re-emit in a chosen form. The harness uses it to build
inputs the crate must read and to compare geometries without trusting the crate.
"""

from __future__ import annotations

import struct

TYPE_NAMES = {
    1: "point",
    2: "linestring",
    3: "polygon",
    4: "multipoint",
    5: "multilinestring",
    6: "multipolygon",
    7: "collection",
}
NAME_TYPES = {v: k for k, v in TYPE_NAMES.items()}


class Node:
    __slots__ = ("kind", "dims", "points", "parts", "srid")

    def __init__(self, kind, dims=(False, False), points=None, parts=None, srid=None):
        self.kind = kind
        self.dims = dims
        self.points = points or []
        self.parts = parts or []
        self.srid = srid


def parse(buf, pos=0, depth=0):
    if depth > 64:
        raise ValueError("nesting too deep")
    if pos >= len(buf):
        raise ValueError("truncated: byte order")
    order = buf[pos]
    if order not in (0, 1):
        raise ValueError(f"byte order {order}")
    e = "<" if order == 1 else ">"
    pos += 1
    if pos + 4 > len(buf):
        raise ValueError("truncated: type")
    (raw,) = struct.unpack_from(e + "I", buf, pos)
    pos += 4

    has_z = bool(raw & 0x80000000)
    has_m = bool(raw & 0x40000000)
    has_srid = bool(raw & 0x20000000)
    base = raw & 0x0FFFFFFF
    if base >= 1000:
        dim = base // 1000
        base %= 1000
        if dim == 1:
            has_z = True
        elif dim == 2:
            has_m = True
        elif dim == 3:
            has_z = has_m = True
        else:
            raise ValueError(f"dimension code {dim}")
    if base not in TYPE_NAMES:
        raise ValueError(f"type {base}")

    srid = None
    if has_srid:
        if pos + 4 > len(buf):
            raise ValueError("truncated: srid")
        (srid,) = struct.unpack_from(e + "i", buf, pos)
        pos += 4

    n = 2 + int(has_z) + int(has_m)
    dims = (has_z, has_m)

    def take(count):
        nonlocal pos
        need = count * n * 8
        if pos + need > len(buf):
            raise ValueError("truncated: coordinates")
        vals = struct.unpack_from(e + "d" * (count * n), buf, pos)
        pos += need
        return [tuple(vals[i * n : (i + 1) * n]) for i in range(count)]

    def take_count():
        nonlocal pos
        if pos + 4 > len(buf):
            raise ValueError("truncated: count")
        (c,) = struct.unpack_from(e + "I", buf, pos)
        pos += 4
        return c

    if base == 1:
        return Node("point", dims, points=take(1), srid=srid), pos
    if base == 2:
        return Node("linestring", dims, points=take(take_count()), srid=srid), pos
    if base == 3:
        rings = []
        for _ in range(take_count()):
            rings.append(Node("ring", dims, points=take(take_count())))
        return Node("polygon", dims, parts=rings, srid=srid), pos

    parts = []
    for _ in range(take_count()):
        sub, pos = parse(buf, pos, depth + 1)
        parts.append(sub)
    return Node(TYPE_NAMES[base], dims, parts=parts, srid=srid), pos


def _finite(pt):
    return pt[0] == pt[0] and pt[1] == pt[1] and abs(pt[0]) != float("inf") and abs(pt[1]) != float("inf")


def _ring(pts):
    """Close the ring and drop it when it cannot be a ring. Matches output::sanitize."""
    pts = [p for p in pts if _finite(p)]
    if len(pts) < 3:
        return None
    if pts[0] != pts[-1]:
        pts = pts + [pts[0]]
    if len(pts) < 4:
        return None
    return pts


def _line(pts):
    pts = [p for p in pts if _finite(p)]
    if len(pts) < 2 or len(pts) != len(pts):
        return None
    return pts


def _part(part):
    """A polygon part: rings, exterior first, holes that survive."""
    if not part.parts:
        return None
    rings = [_ring(r.points) for r in part.parts]
    rings = [r for r in rings if r]
    return rings or None


def _san_part(part):
    rings = [_ring(r) for r in part]
    rings = [r for r in rings if r]
    return rings or None


def _norm(node, classify_only=False):
    """Normalise a parsed node to (kind, data), the shape a closed 2D host can hold.

    `classify_only` keeps non-finite coordinates, so the caller can decide a type before
    sanitizing, which is the order apply_policy and sanitize run in.
    """
    kind = node.kind
    if kind == "point":
        pts = list(node.points) if classify_only else [p for p in node.points if _finite(p)]
        return ("point", pts) if pts else None
    if kind == "multipoint":
        pts = [p.points[0] for p in node.parts if p.points]
        if not classify_only:
            pts = [p for p in pts if _finite(p)]
        return ("multipoint", pts) if pts else None
    if kind in ("linestring", "multilinestring"):
        if kind == "linestring":
            one = _line(node.points)
            return ("line", [one]) if one else None
        lines = [_line(p.points) for p in node.parts]
        lines = [l for l in lines if l]
        return ("line", lines) if lines else None
    if kind in ("polygon", "multipolygon"):
        if kind == "polygon":
            one = _part(node)
            return ("polygon", [one]) if one else None
        parts = [_part(p) for p in node.parts]
        parts = [x for x in parts if x]
        return ("polygon", parts) if parts else None
    if kind == "collection":
        # apply_policy runs before sanitize, so the shape is chosen from the raw members and
        # only then filtered. A dead member still counts when the output type is decided.
        raw_points = []
        raw_lines = []
        raw_polys = []
        for p in node.parts:
            sub = _norm(p, classify_only=True)
            if sub is None:
                continue
            k, d = sub
            if k == "point":
                raw_points.extend(d)
            elif k == "multipoint":
                raw_points.extend(d)
            elif k == "line":
                raw_lines.extend(d)
            else:
                raw_polys.extend(d)
        if raw_polys:
            parts = [x for x in (_san_part(p) for p in raw_polys) if x]
            return ("polygon", parts) if parts else None
        if raw_lines:
            lines = [x for x in (m for m in raw_lines) if x]
            return ("line", lines) if lines else None
        if not raw_points:
            return None
        kind_out = "point" if len(raw_points) == 1 else "multipoint"
        pts = [p for p in raw_points if _finite(p)]
        if not pts:
            return None
        return (kind_out, pts)
    return None


def rustgis_shape(hexstr):
    """The shape a closed 2D host sees, or None when nothing survives.

    Mirrors output::sanitize and apply_policy(TopDimension) on the oracle's bytes, so the
    comparison is against the reference implementation, not against our writer.
    """
    node, _ = parse(bytes.fromhex(hexstr))

    def b(v):
        return format(int.from_bytes(struct.pack("<d", v), "little"), "016x")

    def pt(p):
        return b(p[0]) + "," + b(p[1])

    norm = _norm(node)
    if norm is None:
        return None
    kind, data = norm
    if kind == "point":
        return "P:" + pt(data[0])
    if kind == "multipoint":
        return "MP:" + ";".join(pt(p) for p in data)
    if kind == "line":
        return "L:" + ";".join(" ".join(pt(p) for p in line) for line in data)
    return "PG:" + ";".join("|".join(" ".join(pt(p) for p in ring) for ring in part) for part in data)


def from_hex(h):
    return parse(bytes.fromhex(h))


def _emit(node, order, iso):
    e = "<" if order == 1 else ">"
    kind = "linestring" if node.kind == "ring" else node.kind
    base = NAME_TYPES[kind]
    has_z, has_m = node.dims

    if iso:
        code = base
        if has_z and has_m:
            code += 3000
        elif has_z:
            code += 1000
        elif has_m:
            code += 2000
        use_srid = False
    else:
        code = base
        if has_z:
            code |= 0x80000000
        if has_m:
            code |= 0x40000000
        use_srid = node.srid is not None
        if use_srid:
            code |= 0x20000000

    out = bytearray([order])
    out.extend(struct.pack(e + "I", code))
    if use_srid:
        out.extend(struct.pack(e + "i", node.srid))

    def put(pts):
        for p in pts:
            out.extend(struct.pack(e + "d" * len(p), *p))

    if kind == "point":
        # A point has no count field; it is exactly one coordinate.
        put(node.points)
    elif kind == "linestring":
        out.extend(struct.pack(e + "I", len(node.points)))
        put(node.points)
    elif kind == "polygon":
        out.extend(struct.pack(e + "I", len(node.parts)))
        for r in node.parts:
            out.extend(struct.pack(e + "I", len(r.points)))
            put(r.points)
    else:
        out.extend(struct.pack(e + "I", len(node.parts)))
        for p in node.parts:
            out.extend(_emit(p, order, iso))
    return bytes(out)


def emit(node, big_endian=False, iso=True):
    return _emit(node, 0 if big_endian else 1, iso)


def _strip(node):
    """Copy of the node with X and Y only, for semantic comparison."""
    return Node(
        node.kind,
        (False, False),
        points=[(p[0], p[1]) for p in node.points],
        parts=[_strip(p) for p in node.parts],
    )


def canonical_2d(hexstr):
    """Canonical little-endian 2D ISO WKB, or a marker string when the bytes do not parse."""
    if not hexstr:
        return ""
    try:
        node, _ = parse(bytes.fromhex(hexstr))
        return emit(_strip(node)).hex()
    except Exception as exc:  # noqa: BLE001 - a bad blob is a finding, not a crash
        return f"UNPARSABLE:{exc}"


def to_big_endian(hexstr):
    node, _ = parse(bytes.fromhex(hexstr))
    return emit(node, big_endian=True, iso=True).hex()


def with_srid(hexstr, srid):
    node, _ = parse(bytes.fromhex(hexstr))
    node.srid = srid
    return emit(node, iso=False).hex()


def srid_of(hexstr):
    node, _ = parse(bytes.fromhex(hexstr))
    return node.srid
