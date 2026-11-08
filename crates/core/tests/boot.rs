//! End-to-end: assemble the BIOS and the demo boot sector with NASM, boot the
//! machine headless, and check the greeting reaches the screen.

use std::path::{Path, PathBuf};
use std::process::Command;

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};

fn assemble(src: &str, test: &str) -> Option<Vec<u8>> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bios");
    let out: PathBuf = std::env::temp_dir().join(format!("rusty486-{test}-{src}.bin"));
    let nasm = std::env::var("NASM").unwrap_or_else(|_| "nasm".into());
    let status = Command::new(nasm)
        .args(["-f", "bin", "-o"])
        .arg(&out)
        .arg(root.join(src))
        .status()
        .ok()?;
    assert!(status.success(), "nasm failed on {src}");
    std::fs::read(out).ok()
}

#[test]
fn boots_hello_floppy() {
    let (Some(bios), Some(boot)) = (assemble("bios.asm", "hello"), assemble("hello.asm", "hello"))
    else {
        eprintln!("SKIPPED: nasm not found");
        return;
    };

    let mut floppy = DiskImage::blank_floppy();
    floppy.data[..512].copy_from_slice(&boot);

    let mut m = Machine::new(bios);
    m.insert_disk(Drive::A, floppy);
    // A few emulated milliseconds is plenty; stop early once the guest halts.
    for _ in 0..100 {
        m.run_cycles(100_000);
        if m.cpu.halted && m.cpu.eflags & rusty486_core::cpu::flags::IF == 0 {
            break;
        }
    }

    let log = String::from_utf8_lossy(&m.drain_debug()).into_owned();
    let screen = m.screen_text();
    assert!(!m.cpu.shutdown, "CPU shut down (triple fault)");
    assert!(log.contains("rusty486 BIOS"), "debug log: {log}");
    assert!(log.contains("Booting from floppy A:"), "debug log: {log}");
    assert!(screen.contains("Hello from rusty486!"), "screen:\n{screen}");
    assert!(screen.contains("milestone 1"), "screen:\n{screen}");
}

#[test]
fn reports_no_bootable_disk() {
    let Some(bios) = assemble("bios.asm", "noboot") else {
        eprintln!("SKIPPED: nasm not found");
        return;
    };
    let mut m = Machine::new(bios);
    m.run_cycles(5_000_000);
    assert!(m.screen_text().contains("No bootable disk"), "screen:\n{}", m.screen_text());
}
