//! Assembles the BIOS, the boot loaders and TinyDOS with rusty486's own
//! assembler, so building rusty486 needs no external tools.

use std::path::{Path, PathBuf};

fn assemble(dir: &Path, name: &str, out_dir: &Path) {
    let src_path = dir.join(format!("{name}.asm"));
    let src = std::fs::read_to_string(&src_path)
        .unwrap_or_else(|e| panic!("{}: {e}", src_path.display()));
    let resolver = |inc: &str| {
        let p = dir.join(inc);
        println!("cargo:rerun-if-changed={}", p.display());
        std::fs::read_to_string(p).ok()
    };
    match rusty486_asm::assemble_with(&src, &resolver) {
        Ok(out) => std::fs::write(out_dir.join(format!("{name}.bin")), out.bytes).unwrap(),
        Err(errors) => {
            for e in &errors {
                let file = e.file.clone().unwrap_or_else(|| format!("{name}.asm"));
                println!("cargo:warning={}: line {}: {}", file, e.line, e.message);
            }
            panic!("{} errors assembling {}", errors.len(), src_path.display());
        }
    }
    println!("cargo:rerun-if-changed={}", src_path.display());
}

fn main() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").unwrap());
    for name in ["bios", "hello", "loader"] {
        assemble(&root.join("bios"), name, &out_dir);
    }
    for name in ["boot", "kernel", "command"] {
        assemble(&root.join("dos"), name, &out_dir);
    }
    println!("cargo:rerun-if-changed=build.rs");
}
