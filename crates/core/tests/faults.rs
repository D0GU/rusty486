//! Faults are precise: an instruction that faults part-way (here, a write to
//! a read-only page after the read succeeded) leaves registers and flags as
//! they were, so it can be restarted after the handler. DPMI hosts such as
//! CWSDPMI rely on this for demand paging.

use rusty486_core::bus::Bus;
use rusty486_core::cpu::{Cpu, CR0_PE, CR0_PG, CR0_WP, EAX, ESP};

struct Ram(Vec<u8>);

impl Bus for Ram {
    fn read8(&mut self, a: u32) -> u8 {
        self.0[a as usize & 0xF_FFFF]
    }
    fn write8(&mut self, a: u32, v: u8) {
        self.0[a as usize & 0xF_FFFF] = v;
    }
    fn io_read8(&mut self, _: u16) -> u8 {
        0xFF
    }
    fn io_write8(&mut self, _: u16, _: u8) {}
}

fn put32(ram: &mut Ram, a: u32, v: u32) {
    ram.0[a as usize..a as usize + 4].copy_from_slice(&v.to_le_bytes());
}

fn get32(ram: &Ram, a: u32) -> u32 {
    u32::from_le_bytes(ram.0[a as usize..a as usize + 4].try_into().unwrap())
}

/// Flat 32-bit ring-0 protected mode with paging: the first 1 MB identity
/// mapped, page 5 (5000h) read-only (CR0.WP makes that apply to ring 0),
/// and a #PF handler at 3000h that just halts.
fn machine(code: &[u8]) -> (Cpu, Ram) {
    let mut ram = Ram(vec![0; 1 << 20]);
    // GDT at 800h: null, code 08h, data 10h (flat, 4 GB, 32-bit).
    put32(&mut ram, 0x808, 0x0000_FFFF);
    put32(&mut ram, 0x80C, 0x00CF_9A00);
    put32(&mut ram, 0x810, 0x0000_FFFF);
    put32(&mut ram, 0x814, 0x00CF_9200);
    // IDT at 900h: vector 14 -> 08:3000h, 32-bit interrupt gate.
    put32(&mut ram, 0x900 + 14 * 8, 0x0008_3000);
    put32(&mut ram, 0x904 + 14 * 8, 0x0000_8E00);
    ram.0[0x3000] = 0xF4; // hlt
    // Page directory at 10000h, one page table at 11000h.
    put32(&mut ram, 0x10000, 0x11000 | 3);
    for p in 0..256u32 {
        let flags = if p == 5 { 1 } else { 3 }; // page 5: present, read-only
        put32(&mut ram, 0x11000 + p * 4, p << 12 | flags);
    }
    ram.0[0x2000..0x2000 + code.len()].copy_from_slice(code);

    let mut cpu = Cpu::new();
    cpu.gdtr.base = 0x800;
    cpu.gdtr.limit = 0x17;
    cpu.idtr.base = 0x900;
    cpu.idtr.limit = 0x7FF;
    cpu.cr3 = 0x10000;
    cpu.cr0 = CR0_PE | CR0_PG | CR0_WP | 0x10;
    for s in 0..6 {
        let code = s == 1;
        cpu.sregs[s] = if code { 0x08 } else { 0x10 };
        cpu.seg_base[s] = 0;
        cpu.seg_limit[s] = 0xFFFF_FFFF;
        cpu.seg_attr[s] = if code { 0xC09B } else { 0xC093 };
    }
    cpu.cpl = 0;
    cpu.eip = 0x2000;
    cpu.regs[ESP] = 0x9000;
    (cpu, ram)
}

fn run(cpu: &mut Cpu, ram: &mut Ram) {
    for _ in 0..100 {
        cpu.step(ram);
        if cpu.halted {
            return;
        }
    }
    panic!("did not halt");
}

#[test]
fn adc_to_read_only_page_keeps_carry_for_restart() {
    let code = [
        0xB8, 0x01, 0x00, 0x00, 0x00, // mov eax, 1
        0xF9, //                         stc
        0x11, 0x05, 0x00, 0x50, 0x00, 0x00, // adc [5000h], eax  (read ok, write faults)
        0xF4,
    ];
    let (mut cpu, mut ram) = machine(&code);
    put32(&mut ram, 0x5000, 1);
    run(&mut cpu, &mut ram);
    assert_eq!(cpu.eip, 0x3001, "stopped in the #PF handler");
    assert_eq!(cpu.cr2, 0x5000);
    // Stack: error code, EIP, CS, EFLAGS.
    let esp = cpu.regs[ESP];
    assert_eq!(get32(&ram, esp + 4), 0x2006, "EIP of the ADC");
    assert_eq!(get32(&ram, esp + 12) & 1, 1, "CF as before the ADC");
    assert_eq!(cpu.regs[EAX], 1);
    assert_eq!(get32(&ram, 0x5000), 1, "memory untouched");
}

#[test]
fn xadd_to_read_only_page_keeps_register() {
    let code = [
        0xB8, 0x05, 0x00, 0x00, 0x00, // mov eax, 5
        0x0F, 0xC1, 0x05, 0x00, 0x50, 0x00, 0x00, // xadd [5000h], eax
        0xF4,
    ];
    let (mut cpu, mut ram) = machine(&code);
    put32(&mut ram, 0x5000, 7);
    run(&mut cpu, &mut ram);
    assert_eq!(cpu.eip, 0x3001, "stopped in the #PF handler");
    assert_eq!(cpu.regs[EAX], 5, "EAX not yet exchanged");
    assert_eq!(get32(&ram, 0x5000), 7);
}
