//! Runs the test386.asm CPU test ROM (https://github.com/barotto/test386.asm).
//!
//! The ROM isn't vendored (it's GPL); assemble it into testdata/test386 with
//! NASM first (see README). Skipped if testdata/test386/test386.bin is missing.

use std::path::Path;

use rusty486_core::machine::Machine;

#[test]
fn test386_rom() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/test386");
    let Ok(rom) = std::fs::read(dir.join("test386.bin")) else {
        eprintln!("SKIPPED: testdata/test386/test386.bin not found");
        return;
    };
    let mut m = Machine::new(rom);
    let hz = m.clock_hz;
    if std::env::var_os("RUSTY486_TRACE").is_some() {
        // Step one instruction at a time, keeping the recent CS:EIP history.
        let mut hist = std::collections::VecDeque::new();
        for _ in 0..50_000_000u64 {
            hist.push_back((m.cpu.sregs[1], m.cpu.eip, m.bus.post_code));
            if hist.len() > 60 {
                hist.pop_front();
            }
            m.step();
            // Stop at the ROM's "error" label (it spins in ring 3).
            if m.cpu.halted || m.cpu.shutdown || m.cpu.eip == 0xFE7F {
                break;
            }
        }
        for (cs, eip, post) in hist {
            println!("trace {cs:04X}:{eip:08X} post {post:02X}");
        }
    }
    // Run until the ROM halts (it halts on failure and at the end).
    for _ in 0..600 {
        m.run_cycles(hz / 10);
        if m.cpu.halted || m.cpu.shutdown {
            break;
        }
    }
    let out = String::from_utf8_lossy(&m.drain_debug()).into_owned();
    std::fs::write(dir.join("rusty486-output.txt"), &out).unwrap();
    let post = m.bus.post_code;
    println!("POST code: {post:#04X}, CS:EIP = {:04X}:{:08X}, shutdown = {}", m.cpu.sregs[1], m.cpu.eip, m.cpu.shutdown);
    println!("fault log (vector, fault, code): {:x?}  CPL {} TR {:x?}", m.cpu.fault_log, m.cpu.cpl, m.cpu.tr);
    assert_eq!(post, 0xFF, "test386 stopped at POST {post:#04X} (see test386.lst near EIP {:#X})", m.cpu.eip);

    // POST 0xEE prints arithmetic results; compare with the reference.
    let reference = std::fs::read_to_string(dir.join("test386-EE-reference.txt")).unwrap_or_default();
    let ours: Vec<&str> = out.lines().filter(|l| !l.trim().is_empty()).collect();
    let theirs: Vec<&str> = reference.lines().filter(|l| !l.trim().is_empty()).collect();
    let mismatches: Vec<String> = theirs
        .iter()
        .zip(ours.iter())
        .filter(|(a, b)| a.trim() != b.trim())
        .take(20)
        .map(|(a, b)| format!("ref:  {a}\nours: {b}"))
        .collect();
    assert!(mismatches.is_empty() && ours.len() >= theirs.len(), "{} vs {} lines\n{}", ours.len(), theirs.len(), mismatches.join("\n"));
}
