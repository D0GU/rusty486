//! End-to-end: assemble the BIOS and the demo boot sector with NASM, boot the
//! machine headless, and check the greeting reaches the screen.

use std::path::{Path, PathBuf};
use std::process::Command;

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};

fn assemble(src: &str, test: &str) -> Option<Vec<u8>> {
    let root = if src.contains('/') {
        Path::new(env!("CARGO_MANIFEST_DIR")).to_path_buf()
    } else {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bios")
    };
    let out_name = src.replace('/', "_");
    let out: PathBuf = std::env::temp_dir().join(format!("rusty486-{test}-{out_name}.bin"));
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
fn keyboard_and_timer() {
    let (Some(bios), Some(boot)) = (assemble("bios.asm", "kbd"), assemble("tests/kbd.asm", "kbd")) else {
        eprintln!("SKIPPED: nasm not found");
        return;
    };
    let mut floppy = DiskImage::blank_floppy();
    floppy.data[..512].copy_from_slice(&boot);
    let mut m = Machine::new(bios);
    m.insert_disk(Drive::A, floppy);
    let hz = m.clock_hz;
    m.run_cycles(hz / 10); // boot

    // Type "Hi" (shift + h, i) with key down/up events.
    m.key_event(0x2A, false, true, 0, 0x02, true);
    m.key_event(0x23, false, true, b'H', 0x02, false);
    m.key_event(0x23, false, false, 0, 0x02, false);
    m.key_event(0x2A, false, false, 0, 0x00, true);
    m.key_event(0x17, false, true, b'i', 0, false);
    m.key_event(0x17, false, false, 0, 0, false);
    // Arrow key (extended, no ASCII) must not echo anything printable.
    m.key_event(0x48, true, true, 0, 0, false);
    m.run_cycles(hz); // one emulated second
    m.key_event(0x02, false, true, b'!', 0x02, false);
    m.run_cycles(hz / 10);

    let screen = m.screen_text();
    assert!(screen.contains("Hi"), "screen:\n{screen}");
    let ticks: u32 = screen
        .split("T=")
        .nth(1)
        .and_then(|s| s.split_whitespace().next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("no tick count on screen:\n{screen}"));
    // About 1.1 emulated seconds at 18.2 Hz.
    assert!((17..=23).contains(&ticks), "ticks = {ticks}");
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
