fn main() {
    println!("cargo:rerun-if-changed=build.rs");

    let mut libgeos_config = cmake::Config::new("source");
    libgeos_config
        .define("BUILD_BENCHMARKS", "OFF")
        .define("BUILD_TESTING", "OFF")
        .define("GEOS_ENABLE_TESTS", "OFF")
        .define("BUILD_DOCUMENTATION", "OFF")
        .define("CMAKE_INSTALL_LIBDIR", "lib")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("GEOS_BUILD_STATIC", "ON")
        .define("GEOS_BUILD_SHARED", "OFF")
        .cflag("/O2 /Ob2 /DNDEBUG")
        .cxxflag("/O2 /Ob2 /DNDEBUG")
        .profile("Release");

    let libgeos = libgeos_config.build();

    println!("cargo:lib=geos_c");
    println!("cargo:lib=geos");

    let search_path = format!("{}/lib", libgeos.display());
    assert!(std::path::Path::new(&search_path).exists());
    println!("cargo:search={}", search_path);
}
