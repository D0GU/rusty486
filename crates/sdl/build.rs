//! Assembles the BIOS and the built-in demo boot sector with NASM so they can
//! be embedded in the executable.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let bios_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bios");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    let nasm = std::env::var("NASM").unwrap_or_else(|_| "nasm".into());
    println!("cargo:rerun-if-env-changed=NASM");

    // Statically linked SDL2 uses the registry API, which sdl2-sys doesn't link.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        println!("cargo:rustc-link-lib=advapi32");
    }

    for name in ["bios", "hello", "loader"] {
        let src = bios_dir.join(format!("{name}.asm"));
        println!("cargo:rerun-if-changed={}", src.display());
        let out = out_dir.join(format!("{name}.bin"));
        let status = Command::new(&nasm)
            .args(["-f", "bin", "-o"])
            .arg(&out)
            .arg(&src)
            .status()
            .unwrap_or_else(|e| {
                panic!("could not run NASM ({nasm}): {e}. Install NASM or set the NASM env var.")
            });
        assert!(status.success(), "NASM failed to assemble {}", src.display());
    }
}
