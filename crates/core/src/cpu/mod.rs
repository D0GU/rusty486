//! The i486 CPU core.
//!
//! Milestone 1 implements real mode (including 32-bit operand/address size
//! prefixes and the 386/486 instruction additions). Protected mode, paging and
//! the FPU come later.

mod alu;
mod exec;
mod exec0f;
mod modrm;

use crate::bus::Bus;

pub use alu::flags;

// General register indices, in x86 encoding order.
pub const EAX: usize = 0;
pub const ECX: usize = 1;
pub const EDX: usize = 2;
pub const EBX: usize = 3;
pub const ESP: usize = 4;
pub const EBP: usize = 5;
pub const ESI: usize = 6;
pub const EDI: usize = 7;

// Segment register indices, in x86 encoding order.
pub const ES: usize = 0;
pub const CS: usize = 1;
pub const SS: usize = 2;
pub const DS: usize = 3;
pub const FS: usize = 4;
pub const GS: usize = 5;

/// A CPU exception, identified by its interrupt vector.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault(pub u8);

pub(crate) type R<T> = Result<T, Fault>;

pub(crate) const DE: Fault = Fault(0); // divide error
pub(crate) const BR: Fault = Fault(5); // BOUND range exceeded
pub(crate) const UD: Fault = Fault(6); // invalid opcode
pub(crate) const NM: Fault = Fault(7); // device (FPU) not available
pub(crate) const SSF: Fault = Fault(12); // stack-segment fault
pub(crate) const GP: Fault = Fault(13); // general protection

// CR0 bits.
pub const CR0_PE: u32 = 1 << 0;
pub const CR0_MP: u32 = 1 << 1;
pub const CR0_EM: u32 = 1 << 2;
pub const CR0_TS: u32 = 1 << 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Rep {
    None,
    /// F3: REP / REPE / REPZ
    Repe,
    /// F2: REPNE / REPNZ
    Repne,
}

/// GDTR / IDTR contents.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DescTable {
    pub base: u32,
    pub limit: u16,
}

#[derive(Clone, Debug)]
pub struct Cpu {
    pub regs: [u32; 8],
    pub sregs: [u16; 6],
    /// Hidden descriptor cache: segment base addresses.
    pub seg_base: [u32; 6],
    /// Hidden descriptor cache: segment limits (0xFFFF in real mode).
    pub seg_limit: [u32; 6],
    pub eip: u32,
    pub eflags: u32,
    pub cr0: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub dr: [u32; 8],
    pub gdtr: DescTable,
    pub idtr: DescTable,
    /// Set by HLT; cleared by an interrupt.
    pub halted: bool,
    /// Set on a triple fault. The machine stops until reset.
    pub shutdown: bool,
    /// Approximate clock cycles executed since reset.
    pub cycles: u64,
    /// EFLAGS bits a 32-bit POPF/IRETD may change (differs between 386 and 486).
    pub popf_mask32: u32,
    /// Whether CPUID exists (late 486s). Without it, 0F A2 is #UD.
    pub has_cpuid: bool,

    // Per-instruction decode state.
    start_eip: u32,
    ilen: u32,
    seg_ovr: Option<usize>,
    op32: bool,
    a32: bool,
    rep: Rep,
    lock: bool,
    /// Interrupts are inhibited for one instruction after MOV SS / POP SS / STI.
    irq_inhibit: bool,
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

impl Cpu {
    pub fn new() -> Self {
        let mut cpu = Cpu {
            regs: [0; 8],
            sregs: [0; 6],
            seg_base: [0; 6],
            seg_limit: [0xFFFF; 6],
            eip: 0,
            eflags: 2,
            cr0: 0,
            cr2: 0,
            cr3: 0,
            dr: [0; 8],
            gdtr: DescTable::default(),
            idtr: DescTable::default(),
            halted: false,
            shutdown: false,
            cycles: 0,
            // CF PF AF ZF SF TF IF DF OF IOPL NT, plus AC (bit 18) and ID (bit 21).
            popf_mask32: 0x7FD5 | 1 << 18 | 1 << 21,
            has_cpuid: true,
            start_eip: 0,
            ilen: 0,
            seg_ovr: None,
            op32: false,
            a32: false,
            rep: Rep::None,
            lock: false,
            irq_inhibit: false,
        };
        cpu.reset();
        cpu
    }

    /// Power-on / hardware reset state of a 486SX.
    pub fn reset(&mut self) {
        self.regs = [0; 8];
        self.regs[EDX] = 0x0000_0422; // reset signature: family 4, model 2 (486SX)
        for s in 0..6 {
            self.load_seg_real(s, 0);
        }
        self.load_seg_real(CS, 0xF000);
        self.eip = 0xFFF0;
        self.eflags = 2;
        self.cr0 = 0x6000_0010; // CD, NW, ET
        self.cr2 = 0;
        self.cr3 = 0;
        self.dr = [0; 8];
        self.gdtr = DescTable { base: 0, limit: 0xFFFF };
        self.idtr = DescTable { base: 0, limit: 0x03FF };
        self.halted = false;
        self.shutdown = false;
        self.irq_inhibit = false;
    }

    /// Execute one instruction (or deliver one pending interrupt).
    pub fn step<B: Bus>(&mut self, bus: &mut B) {
        if self.shutdown {
            self.cycles += 1;
            return;
        }

        if self.irq_inhibit {
            self.irq_inhibit = false;
        } else if self.eflags & flags::IF != 0 {
            if let Some(vector) = bus.irq_ack() {
                self.halted = false;
                let ret = self.eip;
                self.deliver(bus, vector, ret);
                return;
            }
        }

        if self.halted {
            self.cycles += 1;
            return;
        }

        self.start_eip = self.eip;
        let start_esp = self.regs[ESP];
        self.ilen = 0;
        self.seg_ovr = None;
        self.op32 = false;
        self.a32 = false;
        self.rep = Rep::None;
        self.lock = false;
        self.cycles += 1;

        if let Err(fault) = self.exec(bus) {
            // Faults restart the instruction: the pushed IP is the faulting one,
            // and any partial stack pointer change (POPA, ENTER...) is undone.
            self.eip = self.start_eip;
            self.regs[ESP] = start_esp;
            self.deliver(bus, fault.0, self.start_eip);
        }
    }

    /// Deliver an interrupt or exception, escalating to double fault and
    /// shutdown if delivery itself faults.
    fn deliver<B: Bus>(&mut self, bus: &mut B, vector: u8, ret_eip: u32) {
        if self.interrupt(bus, vector, ret_eip).is_err()
            && self.interrupt(bus, 8, ret_eip).is_err()
        {
            self.shutdown = true;
        }
    }

    /// Real-mode interrupt: push FLAGS, CS, IP and jump through the IVT.
    pub(crate) fn interrupt<B: Bus>(&mut self, bus: &mut B, vector: u8, ret_eip: u32) -> R<()> {
        self.cycles += 26;
        let entry = vector as u32 * 4;
        if entry + 3 > self.idtr.limit as u32 {
            return Err(GP);
        }
        let ivt = self.idtr.base.wrapping_add(entry);
        let new_ip = bus.read16(ivt);
        let new_cs = bus.read16(ivt.wrapping_add(2));
        let old_cs = self.sregs[CS];
        self.push(bus, 16, self.eflags & 0xFFFF)?;
        self.push(bus, 16, old_cs as u32)?;
        self.push(bus, 16, ret_eip & 0xFFFF)?;
        self.eflags &= !(flags::IF | flags::TF | flags::AC);
        self.load_seg_real(CS, new_cs);
        self.eip = new_ip as u32;
        Ok(())
    }

    pub(crate) fn load_seg_real(&mut self, seg: usize, sel: u16) {
        self.sregs[seg] = sel;
        self.seg_base[seg] = (sel as u32) << 4;
        if seg == SS {
            self.irq_inhibit = true;
        }
    }

    // ---------------------------------------------------------------- registers

    pub(crate) fn reg(&self, sz: u32, r: usize) -> u32 {
        match sz {
            8 if r < 4 => self.regs[r] & 0xFF,
            8 => (self.regs[r - 4] >> 8) & 0xFF,
            16 => self.regs[r] & 0xFFFF,
            _ => self.regs[r],
        }
    }

    pub(crate) fn set_reg(&mut self, sz: u32, r: usize, v: u32) {
        match sz {
            8 if r < 4 => self.regs[r] = (self.regs[r] & !0xFF) | (v & 0xFF),
            8 => self.regs[r - 4] = (self.regs[r - 4] & !0xFF00) | (v & 0xFF) << 8,
            16 => self.regs[r] = (self.regs[r] & !0xFFFF) | (v & 0xFFFF),
            _ => self.regs[r] = v,
        }
    }

    /// Operand size of the current instruction, in bits.
    pub(crate) fn osz(&self) -> u32 {
        if self.op32 {
            32
        } else {
            16
        }
    }

    /// Mask for the current address size.
    pub(crate) fn amask(&self) -> u32 {
        if self.a32 {
            0xFFFF_FFFF
        } else {
            0xFFFF
        }
    }

    /// The data segment for string sources, XLAT and moffs: DS unless overridden.
    pub(crate) fn dseg(&self) -> usize {
        self.seg_ovr.unwrap_or(DS)
    }

    // ---------------------------------------------------------------- fetching

    pub(crate) fn fetch8<B: Bus>(&mut self, bus: &mut B) -> R<u8> {
        self.ilen += 1;
        if self.ilen > 15 || self.eip > self.seg_limit[CS] {
            return Err(GP);
        }
        let v = bus.read8(self.seg_base[CS].wrapping_add(self.eip));
        self.eip = self.eip.wrapping_add(1);
        Ok(v)
    }

    pub(crate) fn fetch16<B: Bus>(&mut self, bus: &mut B) -> R<u16> {
        Ok(u16::from_le_bytes([self.fetch8(bus)?, self.fetch8(bus)?]))
    }

    pub(crate) fn fetch32<B: Bus>(&mut self, bus: &mut B) -> R<u32> {
        Ok(self.fetch16(bus)? as u32 | (self.fetch16(bus)? as u32) << 16)
    }

    /// Fetch an immediate of `sz` bits (8, 16 or 32), zero-extended.
    pub(crate) fn fetch_imm<B: Bus>(&mut self, bus: &mut B, sz: u32) -> R<u32> {
        match sz {
            8 => Ok(self.fetch8(bus)? as u32),
            16 => Ok(self.fetch16(bus)? as u32),
            _ => self.fetch32(bus),
        }
    }

    /// Fetch an 8-bit immediate, sign-extended to 32 bits.
    pub(crate) fn fetch_simm8<B: Bus>(&mut self, bus: &mut B) -> R<u32> {
        Ok(self.fetch8(bus)? as i8 as i32 as u32)
    }

    // ---------------------------------------------------------------- memory

    /// Segment limit check for an access of `n` bytes at `seg:off`.
    pub(crate) fn check(&self, seg: usize, off: u32, n: u32) -> R<()> {
        let lim = self.seg_limit[seg];
        if off > lim || n - 1 > lim - off {
            Err(if seg == SS { SSF } else { GP })
        } else {
            Ok(())
        }
    }

    pub(crate) fn rd<B: Bus>(&mut self, bus: &mut B, seg: usize, off: u32, sz: u32) -> R<u32> {
        self.check(seg, off, sz / 8)?;
        let a = self.seg_base[seg].wrapping_add(off);
        Ok(match sz {
            8 => bus.read8(a) as u32,
            16 => bus.read16(a) as u32,
            _ => bus.read32(a),
        })
    }

    pub(crate) fn wr<B: Bus>(&mut self, bus: &mut B, seg: usize, off: u32, sz: u32, v: u32) -> R<()> {
        self.check(seg, off, sz / 8)?;
        let a = self.seg_base[seg].wrapping_add(off);
        match sz {
            8 => bus.write8(a, v as u8),
            16 => bus.write16(a, v as u16),
            _ => bus.write32(a, v),
        }
        Ok(())
    }

    // ---------------------------------------------------------------- stack

    pub(crate) fn push<B: Bus>(&mut self, bus: &mut B, sz: u32, v: u32) -> R<()> {
        let sp = (self.regs[ESP] as u16).wrapping_sub((sz / 8) as u16);
        self.wr(bus, SS, sp as u32, sz, v)?;
        self.set_reg(16, ESP, sp as u32);
        Ok(())
    }

    pub(crate) fn pop<B: Bus>(&mut self, bus: &mut B, sz: u32) -> R<u32> {
        let sp = self.regs[ESP] as u16;
        let v = self.rd(bus, SS, sp as u32, sz)?;
        self.set_reg(16, ESP, sp.wrapping_add((sz / 8) as u16) as u32);
        Ok(v)
    }

    /// Read a value `depth` bytes above the stack pointer without popping.
    pub(crate) fn stack_peek<B: Bus>(&mut self, bus: &mut B, depth: u32, sz: u32) -> R<u32> {
        let sp = (self.regs[ESP] as u16).wrapping_add(depth as u16);
        self.rd(bus, SS, sp as u32, sz)
    }

    // ---------------------------------------------------------------- control flow

    /// Jump to `target` within CS, truncated to the operand size.
    pub(crate) fn jump(&mut self, target: u32) -> R<()> {
        let target = if self.op32 { target } else { target & 0xFFFF };
        if target > self.seg_limit[CS] {
            return Err(GP);
        }
        self.eip = target;
        Ok(())
    }

    pub(crate) fn jump_far(&mut self, cs: u16, ip: u32) -> R<()> {
        // Real mode: the new CS has a 64K limit.
        if ip > 0xFFFF {
            return Err(GP);
        }
        self.load_seg_real(CS, cs);
        self.eip = ip;
        Ok(())
    }

    /// Effective FLAGS image for PUSHF / interrupt frames.
    pub(crate) fn flags_image(&self, sz: u32) -> u32 {
        if sz == 32 {
            self.eflags & 0x00FC_FFFF // VM and RF read as 0
        } else {
            self.eflags & 0xFFFF
        }
    }

    /// Load FLAGS from a POPF / IRET image (real mode: IOPL and IF writable).
    pub(crate) fn set_flags_image(&mut self, sz: u32, v: u32) {
        let mask = if sz == 32 { self.popf_mask32 } else { 0x7FD5 };
        self.eflags = (self.eflags & !mask) | (v & mask) | 2;
    }
}
