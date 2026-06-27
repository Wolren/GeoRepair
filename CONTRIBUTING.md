# Contributing

Thanks for your interest in GeoRepair. This is a personal project preparing for public release — contributions are welcome but may take time to review.

## Getting started

1. Fork the repo and clone your fork
2. Ensure you have Rust 1.95+ installed (`rustup install stable`)
3. Run `cargo test --all-features` to verify the test suite

## Before submitting

- Ensure `cargo test --features "arrange,structure,parallel,simd,io-geojson,io-wkt,io-wkb,io-csv,ffi"` passes
- Ensure `cargo clippy --features "arrange,structure,parallel,simd,io-geojson,io-wkt,io-wkb,io-csv,ffi"` is clean
- Keep PRs focused on a single change
- Include property-based tests for new geometry repair or validation logic

## Property-based testing

Every geometry or GIS function should have property-based invariant tests using proptest.
Degenerate inputs (empty rings, NaN coordinates, collinear vertices) must be covered.
No tests that pass after mutation without catching the fault.

## Questions?

Open a discussion or issue first before investing significant time.
