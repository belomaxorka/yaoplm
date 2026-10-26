//! On MSVC cargo-fuzz passes `/include:main` to every crate it builds, so
//! that the linker pulls libFuzzer's `main` into the targets. A dependency
//! that is also a `cdylib` (sevenz-rust2) then fails to link: a DLL has no
//! `main`. Build with `--no-include-main-msvc`; this adds the argument to
//! the fuzz targets alone.

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        println!("cargo:rustc-link-arg-bins=/include:main");
    }
}
