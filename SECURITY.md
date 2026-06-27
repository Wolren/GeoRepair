# Security Policy

## Reporting a Vulnerability

If you find a security vulnerability, please open a private advisory on GitHub:

1. Go to https://github.com/Wolren/GeoRepair/security/advisories
2. Click **New draft security advisory**
3. Describe the issue in detail

You should receive a response within 7 days.

## Supported Versions

Only the latest published crates.io release receives security patches.

## Scope

- Geometry parsing (malformed WKT, WKB, GeoJSON inputs)
- Coordinate validation (NaN, infinity, extreme values)
- Dependency vulnerabilities
- Unsafe code correctness (SIMD paths, FFI bindings)

Out of scope: local dev tooling, experimental features marked as unstable, benchmark-only code.
