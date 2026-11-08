fn main() {
    // Statically linked SDL2 uses the registry API, which sdl2-sys doesn't link.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }
}
