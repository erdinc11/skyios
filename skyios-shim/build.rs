fn main() {
    let dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    if std::env::var("TARGET").unwrap().ends_with("-none-softfloat") {
        println!("cargo:rustc-link-arg-bins=-T{dir}/shim.ld");
        println!("cargo:rustc-link-arg-bins=--pie");
        println!("cargo:rustc-link-arg-bins=-znotext");
        println!("cargo:rustc-link-arg-bins=--no-dynamic-linker");
        println!("cargo:rustc-link-arg-bins=--build-id=none");
    }
    println!("cargo:rerun-if-changed=shim.ld");
}
