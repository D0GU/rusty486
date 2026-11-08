//! REP MOVS / REP STOS with 32-bit addressing (the bulk fast path), against
//! what element-by-element execution must produce.

use rusty486_core::cpu::{Cpu, ECX, EDI, ESI};
use rusty486_core::machine::Machine;

fn run(code: &[u8], setup: impl FnOnce(&mut Machine)) -> Machine {
    let mut m = Machine::new(vec![0xF4; 16]);
    m.bus.ram[0x100..0x100 + code.len()].copy_from_slice(code);
    setup(&mut m);
    let cpu: &mut Cpu = &mut m.cpu;
    for s in 0..6 {
        cpu.sregs[s] = 0;
        cpu.seg_base[s] = 0;
        cpu.seg_limit[s] = 0xFFFF_FFFF; // flat ("unreal") segments for 32-bit offsets
    }
    cpu.eip = 0x100;
    for _ in 0..100_000 {
        m.step();
        if m.cpu.halted {
            return m;
        }
    }
    panic!("did not halt");
}

#[test]
fn rep_movsd_and_stosd() {
    let code = [
        0x66, 0xB9, 0x00, 0x04, 0x00, 0x00, // mov ecx, 1024
        0x66, 0xBE, 0x00, 0x00, 0x01, 0x00, // mov esi, 10000h
        0x66, 0xBF, 0x02, 0x00, 0x02, 0x00, // mov edi, 20002h (unaligned, crosses pages)
        0x67, 0x66, 0xF3, 0xA5, //             rep movsd (a32)
        0x66, 0xB8, 0x44, 0x33, 0x22, 0x11, // mov eax, 11223344h
        0x66, 0xB9, 0x05, 0x00, 0x00, 0x00, // mov ecx, 5
        0x66, 0xBF, 0x00, 0x00, 0x03, 0x00, // mov edi, 30000h
        0x67, 0x66, 0xF3, 0xAB, //             rep stosd (a32)
        0xF4,
    ];
    let m = run(&code, |m| {
        for i in 0..4096 {
            m.bus.ram[0x10000 + i] = (i * 7 + 3) as u8;
        }
    });
    assert_eq!(&m.bus.ram[0x20002..0x20002 + 4096], &m.bus.ram[0x10000..0x10000 + 4096]);
    assert_eq!(m.cpu.regs[ESI], 0x11000);
    assert_eq!(m.cpu.regs[EDI], 0x30014);
    assert_eq!(m.cpu.regs[ECX], 0);
    assert_eq!(&m.bus.ram[0x30000..0x30008], &[0x44, 0x33, 0x22, 0x11, 0x44, 0x33, 0x22, 0x11]);
    assert_eq!(m.bus.ram[0x30014], 0);
}

#[test]
fn overlapping_rep_movsd_replicates() {
    // Copying forward onto itself repeats the first dword, unlike memmove.
    let code = [
        0x66, 0xB9, 0x08, 0x00, 0x00, 0x00, // mov ecx, 8
        0x66, 0xBE, 0x00, 0x00, 0x01, 0x00, // mov esi, 10000h
        0x66, 0xBF, 0x04, 0x00, 0x01, 0x00, // mov edi, 10004h
        0x67, 0x66, 0xF3, 0xA5, //             rep movsd
        0xF4,
    ];
    let m = run(&code, |m| m.bus.ram[0x10000..0x10004].copy_from_slice(&[1, 2, 3, 4]));
    for k in 0..9 {
        assert_eq!(&m.bus.ram[0x10000 + 4 * k..0x10004 + 4 * k], &[1, 2, 3, 4], "dword {k}");
    }
}
