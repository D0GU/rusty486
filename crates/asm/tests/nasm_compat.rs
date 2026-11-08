//! Byte-for-byte comparison with NASM (`nasm -f bin`). Skipped if NASM isn't
//! installed (or set the NASM env var to its path).

use std::path::{Path, PathBuf};
use std::process::Command;

fn nasm(src: &Path, name: &str) -> Option<Vec<u8>> {
    let out: PathBuf = std::env::temp_dir().join(format!("rusty486-asm-{name}.bin"));
    let nasm = std::env::var("NASM").unwrap_or_else(|_| "nasm".into());
    let inc = format!("-i{}/", src.parent().unwrap().display());
    let status = Command::new(nasm).arg(inc).args(["-f", "bin", "-o"]).arg(&out).arg(src).status().ok()?;
    assert!(status.success(), "NASM rejected {}", src.display());
    std::fs::read(out).ok()
}

fn compare(path: &Path, name: &str) {
    let Some(expected) = nasm(path, name) else {
        eprintln!("SKIPPED: nasm not found");
        return;
    };
    let src = std::fs::read_to_string(path).unwrap();
    let dir = path.parent().unwrap().to_path_buf();
    let resolver = |n: &str| std::fs::read_to_string(dir.join(n)).ok();
    let out = match rusty486_asm::assemble_with(&src, &resolver) {
        Ok(o) => o,
        Err(errs) => {
            let lines: Vec<&str> = src.lines().collect();
            let msgs: Vec<String> = errs
                .iter()
                .take(30)
                .map(|e| format!("{e}\n    {}", lines.get(e.line - 1).unwrap_or(&"").trim()))
                .collect();
            panic!("{name}: {} errors:\n{}", errs.len(), msgs.join("\n"));
        }
    };
    if out.bytes == expected {
        return;
    }
    // Report the first lines whose bytes differ.
    let lines: Vec<&str> = src.lines().collect();
    let mut report = Vec::new();
    // NASM's listing isn't available, so find where lengths diverge by checking
    // which of our instructions NASM's bytes would continue differently after.
    let first_diff = out.bytes.iter().zip(&expected).position(|(a, b)| a != b).unwrap_or(0);
    report.push(format!("first differing byte at offset {first_diff:#x}"));
    for li in &out.lines {
        let s = (li.addr - out.origin) as usize;
        let ours = &out.bytes[s..s + li.len as usize];
        let theirs = expected.get(s..s + li.len as usize).unwrap_or(&[]);
        if ours != theirs {
            let hex = |b: &[u8]| b.iter().map(|x| format!("{x:02X}")).collect::<Vec<_>>().join(" ");
            report.push(format!(
                "line {} `{}`\n    ours: {}\n    nasm: {}",
                li.line,
                lines.get(li.line - 1).unwrap_or(&"").trim(),
                hex(ours),
                hex(theirs)
            ));
            if report.len() >= 25 {
                break;
            }
        }
    }
    panic!(
        "{name}: output differs (ours {} bytes, nasm {} bytes)\n{}",
        out.bytes.len(),
        expected.len(),
        report.join("\n")
    );
}

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn coverage_matches_nasm() {
    compare(&Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/coverage.asm"), "coverage");
}

#[test]
fn bios_matches_nasm() {
    compare(&repo().join("bios/bios.asm"), "bios");
}

#[test]
fn hello_matches_nasm() {
    compare(&repo().join("bios/hello.asm"), "hello");
}
