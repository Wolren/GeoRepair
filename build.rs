fn main() {
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"rstar\"))");
    // GDAL bench (benches/io_cmp.rs): link search for gdal.lib comes from GDAL_LIB_DIR.
    if let Ok(dir) = std::env::var("GDAL_LIB_DIR") {
        println!("cargo:rustc-link-search=native={dir}");
        println!("cargo:rerun-if-env-changed=GDAL_LIB_DIR");
    }
}
