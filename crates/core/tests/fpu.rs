//! The fantasy R1024's x87 FPU, driven by hand-assembled real-mode code.

use rusty486_core::bus::Bus;
use rusty486_core::cpu::{Cpu, CpuModel, EAX};

struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read8(&mut self, a: u32) -> u8 {
        self.0[a as usize & 0xFFFFF]
    }
    fn write8(&mut self, a: u32, v: u8) {
        self.0[a as usize & 0xFFFFF] = v;
    }
    fn io_read8(&mut self, _: u16) -> u8 {
        0xFF
    }
    fn io_write8(&mut self, _: u16, _: u8) {}
}

/// Run `code` at 0000:0100 (data at 0000:1000) until HLT.
fn run(code: &[u8], data: &[(u32, &[u8])]) -> (Cpu, Ram) {
    let mut ram = Ram(vec![0; 1 << 20]);
    ram.0[0x100..0x100 + code.len()].copy_from_slice(code);
    for (a, d) in data {
        ram.0[*a as usize..*a as usize + d.len()].copy_from_slice(d);
    }
    let mut cpu = Cpu::new();
    cpu.model = CpuModel::R1024;
    cpu.reset();
    for s in 0..6 {
        cpu.sregs[s] = 0;
        cpu.seg_base[s] = 0;
    }
    cpu.eip = 0x100;
    for _ in 0..1000 {
        cpu.step(&mut ram);
        if cpu.halted {
            return (cpu, ram);
        }
    }
    panic!("did not halt");
}

fn f64_at(ram: &Ram, a: usize) -> f64 {
    f64::from_le_bytes(ram.0[a..a + 8].try_into().unwrap())
}

#[test]
fn arithmetic_and_stores() {
    let code = [
        0xDB, 0xE3, // fninit
        0xDD, 0x06, 0x00, 0x10, // fld qword [1000h]
        0xD9, 0xFA, // fsqrt
        0xDE, 0x0E, 0x08, 0x10, // fimul word [1008h]
        0xD9, 0xE8, // fld1
        0xDE, 0xC1, // faddp st1
        0xDD, 0x1E, 0x10, 0x10, // fstp qword [1010h]
        0xD9, 0xEB, // fldpi
        0xDB, 0x3E, 0x20, 0x10, // fstp tword [1020h]
        0xDB, 0x2E, 0x20, 0x10, // fld tword [1020h]
        0xD8, 0x36, 0x40, 0x10, // fdiv dword [1040h]
        0xDD, 0x1E, 0x30, 0x10, // fstp qword [1030h]
        0xF4,
    ];
    let (cpu, ram) = run(&code, &[(0x1000, &2.0f64.to_le_bytes()), (0x1008, &3i16.to_le_bytes()), (0x1040, &4.0f32.to_le_bytes())]);
    assert!((f64_at(&ram, 0x1010) - (3.0 * 2f64.sqrt() + 1.0)).abs() < 1e-12);
    assert_eq!(&ram.0[0x1020..0x102A], &[0x00, 0xC0, 0x68, 0x21, 0xA2, 0xDA, 0x0F, 0xC9, 0x00, 0x40][..]);
    assert_eq!(f64_at(&ram, 0x1030), std::f64::consts::PI / 4.0);
    assert_eq!(cpu.fpu.top, 0, "stack balanced");
}

#[test]
fn compare_and_status_word() {
    let code = [
        0xDB, 0xE3, // fninit
        0xD9, 0x06, 0x00, 0x10, // fld dword [1000h]   (1.5)
        0xD8, 0x1E, 0x04, 0x10, // fcomp dword [1004h] (2.5)
        0xDF, 0xE0, // fnstsw ax
        0xF4,
    ];
    let (cpu, _) = run(&code, &[(0x1000, &1.5f32.to_le_bytes()), (0x1004, &2.5f32.to_le_bytes())]);
    let sw = cpu.regs[EAX] & 0xFFFF;
    assert_eq!(sw & 0x4500, 0x0100, "C0 set (less than): {sw:04X}");
    assert_eq!((sw >> 11) & 7, 0);
}

#[test]
fn integer_stores_round_to_nearest_even() {
    let code = [
        0xDB, 0xE3, // fninit
        0xDD, 0x06, 0x00, 0x10, // fld qword [1000h]   (2.5)
        0xDF, 0x1E, 0x10, 0x10, // fistp word [1010h]
        0xDD, 0x06, 0x08, 0x10, // fld qword [1008h]   (-7.5)
        0xDB, 0x1E, 0x12, 0x10, // fistp dword [1012h]
        0xD9, 0x2E, 0x20, 0x10, // fldcw [1020h]       (truncate)
        0xDD, 0x06, 0x08, 0x10, // fld qword [1008h]
        0xDF, 0x3E, 0x18, 0x10, // fistp qword [1018h]
        0xF4,
    ];
    let cw: u16 = 0x0F7F;
    let (_, ram) = run(&code, &[(0x1000, &2.5f64.to_le_bytes()), (0x1008, &(-7.5f64).to_le_bytes()), (0x1020, &cw.to_le_bytes())]);
    assert_eq!(i16::from_le_bytes([ram.0[0x1010], ram.0[0x1011]]), 2);
    assert_eq!(i32::from_le_bytes(ram.0[0x1012..0x1016].try_into().unwrap()), -8);
    assert_eq!(i64::from_le_bytes(ram.0[0x1018..0x1020].try_into().unwrap()), -7);
}

#[test]
fn sx_traps_fpu_instructions_when_emulated() {
    let mut ram = Ram(vec![0; 1 << 20]);
    ram.0[0x100..0x102].copy_from_slice(&[0xD9, 0xE8]); // fld1
    let mut cpu = Cpu::new();
    for s in 0..6 {
        cpu.sregs[s] = 0;
        cpu.seg_base[s] = 0;
    }
    cpu.cr0 |= rusty486_core::cpu::CR0_EM;
    // IVT entry 7 -> 0000:0200 (HLT).
    ram.0[0x1C..0x20].copy_from_slice(&[0x00, 0x02, 0x00, 0x00]);
    ram.0[0x200] = 0xF4;
    cpu.eip = 0x100;
    cpu.step(&mut ram);
    assert_eq!(cpu.eip, 0x200, "#NM delivered");
}
