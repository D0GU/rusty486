//! Assembles the DOS extender stub that every R++ program is packed with.

use std::path::{Path, PathBuf};

fn main() {
    let src_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../dos/pmstub.asm");
    let src = std::fs::read_to_string(&src_path).unwrap();
    let out = rusty486_asm::assemble(&src).unwrap_or_else(|e| panic!("pmstub.asm: {e:?}"));
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    std::fs::write(out_dir.join("pmstub.bin"), out.bytes).unwrap();
    println!("cargo:rerun-if-changed={}", src_path.display());
    println!("cargo:rerun-if-changed=build.rs");
}
