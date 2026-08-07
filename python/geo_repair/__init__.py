"""geo_repair - validate and repair invalid OGC GIS geometries.

The Rust extension module is re-exported here; the public surface is the
full WKB + WKT repair/validate API (see ``geo_repair/__init__.pyi`` for
typed signatures).
"""

from .geo_repair import *  # noqa: F401,F403
from .geo_repair import __version__, version  # noqa: F401

__doc__ = geo_repair.__doc__  # noqa: F405
if hasattr(geo_repair, "__all__"):  # noqa: F405
    __all__ = geo_repair.__all__  # noqa: F405
