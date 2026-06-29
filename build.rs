fn main() {
    println!("cargo::rustc-check-cfg=cfg(feature, values(\"rstar\"))");
}
