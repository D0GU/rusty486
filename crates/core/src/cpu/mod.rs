//! The i486 CPU core: real mode, protected mode (segmentation, privilege
//! levels, gates) and paging. Not emulated: task switching, virtual-8086
//! mode and the FPU (this is a 486SX).

mod alu;
mod exec;
mod exec0f;
mod modrm;
mod paging;
mod protected;

use crate::bus::Bus;

pub use alu::flags;
pub use protected::Desc;

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

/// A CPU exception: vector and error code (pushed in protected mode for
/// vectors 8, 10-14 and 17).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fault(pub u8, pub u32);

pub(crate) type R<T> = Result<T, Fault>;

pub(crate) const DE: Fault = Fault(0, 0); // divide error
pub(crate) const BR: Fault = Fault(5, 0); // BOUND range exceeded
pub(crate) const UD: Fault = Fault(6, 0); // invalid opcode
pub(crate) const NM: Fault = Fault(7, 0); // device (FPU) not available
pub(crate) const SSF: Fault = Fault(12, 0); // stack-segment fault
pub(crate) const GP: Fault = Fault(13, 0); // general protection

pub(crate) fn gp(code: u32) -> Fault {
    Fault(13, code)
}

// CR0 bits.
pub const CR0_PE: u32 = 1 << 0;
pub const CR0_MP: u32 = 1 << 1;
pub const CR0_EM: u32 = 1 << 2;
pub const CR0_TS: u32 = 1 << 3;
pub const CR0_WP: u32 = 1 << 16;
pub const CR0_PG: u32 = 1 << 31;

/// Descriptor-cache attribute bits (access byte in bits 0-7, the D/B flag
/// in bit 14 and G in bit 15, as in the descriptor's high word).
pub const ATTR_PRESENT: u16 = 0x80;
pub const ATTR_DB: u16 = 0x4000;
/// EFLAGS.VM (virtual-8086 mode).
pub const VM_FLAG: u32 = 1 << 17;

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

/// A system segment register (LDTR, TR) with its descriptor cache.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SysSeg {
    pub sel: u16,
    pub base: u32,
    pub limit: u32,
    pub attr: u16,
}

/// How an interrupt was raised (affects privilege checks and error codes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IntKind {
    /// INT n, INT3, INTO: gate DPL is checked.
    Software,
    /// CPU exception (may push an error code).
    Exception(u32),
    /// External interrupt.
    Hardware,
}

#[derive(Clone, Debug)]
pub struct Cpu {
    pub regs: [u32; 8],
    pub sregs: [u16; 6],
    /// Hidden descriptor cache: base, limit and attributes per segment register.
    pub seg_base: [u32; 6],
    pub seg_limit: [u32; 6],
    pub seg_attr: [u16; 6],
    pub eip: u32,
    pub eflags: u32,
    pub cr0: u32,
    pub cr2: u32,
    pub cr3: u32,
    pub dr: [u32; 8],
    pub gdtr: DescTable,
    pub idtr: DescTable,
    pub ldtr: SysSeg,
    pub tr: SysSeg,
    /// Current privilege level (0 in real mode).
    pub cpl: u8,
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
    /// Emulate i386 quirks where the 386 and 486 differ (POPAD with a
    /// 16-bit stack loads ESP's upper half on the 386).
    pub quirks_386: bool,
    /// Failed interrupt deliveries: (vector, fault vector, error code).
    pub fault_log: Vec<(u8, u8, u32)>,

    /// Paging TLB: tag (page | 1) and data (physical page | permission bits).
    tlb_tag: Vec<u32>,
    tlb_data: Vec<u32>,

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
            seg_attr: [0x93; 6],
            eip: 0,
            eflags: 2,
            cr0: 0,
            cr2: 0,
            cr3: 0,
            dr: [0; 8],
            gdtr: DescTable::default(),
            idtr: DescTable::default(),
            ldtr: SysSeg::default(),
            tr: SysSeg::default(),
            cpl: 0,
            halted: false,
            shutdown: false,
            cycles: 0,
            // CF PF AF ZF SF TF IF DF OF IOPL NT, plus AC (bit 18) and ID (bit 21).
            popf_mask32: 0x7FD5 | 1 << 18 | 1 << 21,
            has_cpuid: true,
            quirks_386: false,
            fault_log: Vec::new(),
            tlb_tag: vec![0; paging::TLB_SIZE],
            tlb_data: vec![0; paging::TLB_SIZE],
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
        self.cr0 = 0x6000_0010; // CD, NW, ET
        self.cpl = 0;
        for s in 0..6 {
            self.load_seg_real(s, 0);
            self.seg_limit[s] = 0xFFFF;
            self.seg_attr[s] = 0x93;
        }
        self.load_seg_real(CS, 0xF000);
        self.seg_attr[CS] = 0x9B;
        self.eip = 0xFFF0;
        self.eflags = 2;
        self.cr2 = 0;
        self.cr3 = 0;
        self.dr = [0; 8];
        self.gdtr = DescTable { base: 0, limit: 0xFFFF };
        self.idtr = DescTable { base: 0, limit: 0x03FF };
        self.ldtr = SysSeg::default();
        self.tr = SysSeg::default();
        self.tlb_flush();
        self.halted = false;
        self.shutdown = false;
        self.irq_inhibit = false;
    }

    /// Protected mode enabled.
    pub fn pe(&self) -> bool {
        self.cr0 & CR0_PE != 0
    }

    /// Virtual-8086 mode (EFLAGS.VM).
    pub fn vm(&self) -> bool {
        self.eflags & VM_FLAG != 0
    }

    /// 32-bit code segment (default operand/address size 32).
    pub fn code32(&self) -> bool {
        self.pe() && !self.vm() && self.seg_attr[CS] & ATTR_DB != 0
    }

    /// 32-bit stack segment (push/pop use ESP).
    pub fn stack32(&self) -> bool {
        self.pe() && !self.vm() && self.seg_attr[SS] & ATTR_DB != 0
    }

    pub(crate) fn iopl(&self) -> u8 {
        ((self.eflags >> 12) & 3) as u8
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
                self.deliver(bus, vector, ret, IntKind::Hardware);
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
        let d = self.code32();
        self.op32 = d;
        self.a32 = d;
        self.rep = Rep::None;
        self.lock = false;
        self.cycles += 1;

        if let Err(fault) = self.exec(bus) {
            // Faults restart the instruction: the pushed IP is the faulting one,
            // and any partial stack pointer change (POPA, ENTER...) is undone.
            self.eip = self.start_eip;
            self.regs[ESP] = start_esp;
            self.deliver(bus, fault.0, self.start_eip, IntKind::Exception(fault.1));
        }
    }

    /// Deliver an interrupt or exception, escalating to double fault and
    /// shutdown if delivery itself faults.
    fn deliver<B: Bus>(&mut self, bus: &mut B, vector: u8, ret_eip: u32, kind: IntKind) {
        if let Err(f) = self.interrupt(bus, vector, ret_eip, kind) {
            self.log_fault(vector, f);
            if let Err(f2) = self.interrupt(bus, 8, ret_eip, IntKind::Exception(0)) {
                self.log_fault(8, f2);
                self.shutdown = true;
            }
        }
    }

    /// Remember failed interrupt deliveries (vector being delivered and the
    /// fault that stopped it) for debugging triple faults.
    fn log_fault(&mut self, vector: u8, f: Fault) {
        if self.fault_log.len() >= 8 {
            self.fault_log.remove(0);
        }
        self.fault_log.push((vector, f.0, f.1));
    }

    /// Take interrupt `vector`, returning to `ret_eip`.
    pub(crate) fn interrupt<B: Bus>(&mut self, bus: &mut B, vector: u8, ret_eip: u32, kind: IntKind) -> R<()> {
        self.cycles += 26;
        if self.pe() {
            if self.vm() && kind == IntKind::Software && self.iopl() < 3 {
                return Err(GP);
            }
            return self.interrupt_pm(bus, vector, ret_eip, kind);
        }
        let entry = vector as u32 * 4;
        if entry + 3 > self.idtr.limit as u32 {
            return Err(GP);
        }
        let ivt = self.idtr.base.wrapping_add(entry);
        let new_ip = self.sys_read(bus, ivt, 16)? as u16;
        let new_cs = self.sys_read(bus, ivt.wrapping_add(2), 16)? as u16;
        let old_cs = self.sregs[CS];
        self.push(bus, 16, self.eflags & 0xFFFF)?;
        self.push(bus, 16, old_cs as u32)?;
        self.push(bus, 16, ret_eip & 0xFFFF)?;
        self.eflags &= !(flags::IF | flags::TF | flags::AC);
        self.load_seg_real(CS, new_cs);
        self.eip = new_ip as u32;
        Ok(())
    }

    /// Load a segment register with real-mode semantics (base = selector * 16).
    pub fn set_segment(&mut self, seg: usize, sel: u16) {
        self.load_seg_real(seg, sel);
    }

    /// Real-mode segment load: only the base changes (the limit and
    /// attributes stay, which is what makes "unreal mode" work).
    pub(crate) fn load_seg_real(&mut self, seg: usize, sel: u16) {
        self.sregs[seg] = sel;
        self.seg_base[seg] = (sel as u32) << 4;
        self.seg_attr[seg] |= ATTR_PRESENT;
        if seg == CS {
            self.seg_attr[CS] &= !ATTR_DB;
        }
        if seg == SS {
            self.irq_inhibit = true;
        }
    }

    /// Virtual-8086 segment load: real-mode base, 64 KB limit, DPL 3 data.
    pub(crate) fn load_seg_v86(&mut self, seg: usize, sel: u16) {
        self.sregs[seg] = sel;
        self.seg_base[seg] = (sel as u32) << 4;
        self.seg_limit[seg] = 0xFFFF;
        self.seg_attr[seg] = 0xF3;
        if seg == SS {
            self.irq_inhibit = true;
        }
    }

    /// Load a data/stack segment register (MOV, POP, LxS) in either mode.
    pub(crate) fn load_seg<B: Bus>(&mut self, bus: &mut B, seg: usize, sel: u16) -> R<()> {
        if self.vm() {
            self.load_seg_v86(seg, sel);
            return Ok(());
        }
        if self.pe() {
            self.load_seg_pm(bus, seg, sel)?;
            if seg == SS {
                self.irq_inhibit = true;
            }
            Ok(())
        } else {
            self.load_seg_real(seg, sel);
            Ok(())
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
        let lin = self.seg_base[CS].wrapping_add(self.eip);
        let v = if self.cr0 & CR0_PG != 0 {
            let p = self.translate(bus, lin, false, self.cpl == 3)?;
            bus.read8(p)
        } else {
            bus.read8(lin)
        };
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

    /// Segment checks for an access of `n` bytes at `seg:off`: limit (with
    /// expand-down), and in protected mode presence and read/write rights.
    pub(crate) fn check_access(&self, seg: usize, off: u32, n: u32, write: bool) -> R<()> {
        let fault = if seg == SS { SSF } else { GP };
        let attr = self.seg_attr[seg];
        if self.pe() {
            if attr & ATTR_PRESENT == 0 {
                return Err(fault); // null selector
            }
            let code = attr & 0x08 != 0;
            if write && (code || attr & 0x02 == 0) {
                return Err(fault);
            }
            if !write && code && attr & 0x02 == 0 && seg != CS {
                return Err(fault); // execute-only code
            }
            if !code && attr & 0x04 != 0 {
                // Expand-down: valid offsets are limit+1 ..= max.
                let max = if attr & ATTR_DB != 0 { 0xFFFF_FFFF } else { 0xFFFF };
                let last = off.wrapping_add(n - 1);
                if off <= self.seg_limit[seg] || last > max || last < off {
                    return Err(fault);
                }
                return Ok(());
            }
        }
        let lim = self.seg_limit[seg];
        if off > lim || n - 1 > lim - off {
            Err(fault)
        } else {
            Ok(())
        }
    }

    pub(crate) fn check(&self, seg: usize, off: u32, n: u32) -> R<()> {
        self.check_access(seg, off, n, false)
    }

    pub(crate) fn rd<B: Bus>(&mut self, bus: &mut B, seg: usize, off: u32, sz: u32) -> R<u32> {
        self.check_access(seg, off, sz / 8, false)?;
        let lin = self.seg_base[seg].wrapping_add(off);
        self.lin_read(bus, lin, sz, self.cpl == 3)
    }

    pub(crate) fn wr<B: Bus>(&mut self, bus: &mut B, seg: usize, off: u32, sz: u32, v: u32) -> R<()> {
        self.check_access(seg, off, sz / 8, true)?;
        let lin = self.seg_base[seg].wrapping_add(off);
        self.lin_write(bus, lin, sz, v, self.cpl == 3)
    }

    /// Check that `n` bytes at seg:off could be written (segment and paging
    /// checks) without writing anything.
    pub(crate) fn probe_write<B: Bus>(&mut self, bus: &mut B, seg: usize, off: u32, n: u32) -> R<()> {
        self.check_access(seg, off, n, true)?;
        if self.cr0 & CR0_PG != 0 {
            let lin = self.seg_base[seg].wrapping_add(off);
            let user = self.cpl == 3;
            self.translate(bus, lin, true, user)?;
            self.translate(bus, lin.wrapping_add(n - 1), true, user)?;
        }
        Ok(())
    }

    /// Supervisor-level read of a linear address (descriptor tables, TSS, IVT).
    pub(crate) fn sys_read<B: Bus>(&mut self, bus: &mut B, lin: u32, sz: u32) -> R<u32> {
        self.lin_read(bus, lin, sz, false)
    }

    pub(crate) fn sys_write<B: Bus>(&mut self, bus: &mut B, lin: u32, sz: u32, v: u32) -> R<()> {
        self.lin_write(bus, lin, sz, v, false)
    }

    // ---------------------------------------------------------------- stack

    /// Mask for the stack pointer (16- or 32-bit stack).
    pub(crate) fn smask(&self) -> u32 {
        if self.stack32() {
            0xFFFF_FFFF
        } else {
            0xFFFF
        }
    }

    pub(crate) fn sp(&self) -> u32 {
        self.regs[ESP] & self.smask()
    }

    pub(crate) fn set_sp(&mut self, v: u32) {
        let m = self.smask();
        self.regs[ESP] = (self.regs[ESP] & !m) | (v & m);
    }

    pub(crate) fn push<B: Bus>(&mut self, bus: &mut B, sz: u32, v: u32) -> R<()> {
        let sp = self.sp().wrapping_sub(sz / 8) & self.smask();
        self.wr(bus, SS, sp, sz, v)?;
        self.set_sp(sp);
        Ok(())
    }

    pub(crate) fn pop<B: Bus>(&mut self, bus: &mut B, sz: u32) -> R<u32> {
        let sp = self.sp();
        let v = self.rd(bus, SS, sp, sz)?;
        self.set_sp(sp.wrapping_add(sz / 8));
        Ok(v)
    }

    /// POP into a segment register: only 16 bits are read, even with a
    /// 32-bit operand size (which still adds 4 to SP).
    pub(crate) fn pop_sreg<B: Bus>(&mut self, bus: &mut B, seg: usize) -> R<()> {
        let sp = self.sp();
        let mask = self.smask(); // the stack size before a POP SS changes it
        let v = self.rd(bus, SS, sp, 16)?;
        self.load_seg(bus, seg, v as u16)?;
        let new = sp.wrapping_add(self.osz() / 8);
        self.regs[ESP] = (self.regs[ESP] & !mask) | (new & mask);
        Ok(())
    }

    /// Read a value `depth` bytes above the stack pointer without popping.
    pub(crate) fn stack_peek<B: Bus>(&mut self, bus: &mut B, depth: u32, sz: u32) -> R<u32> {
        let sp = self.sp().wrapping_add(depth) & self.smask();
        self.rd(bus, SS, sp, sz)
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

    /// Effective FLAGS image for PUSHF / interrupt frames.
    pub(crate) fn flags_image(&self, sz: u32) -> u32 {
        if sz == 32 {
            self.eflags & 0x00FC_FFFF // VM and RF read as 0
        } else {
            self.eflags & 0xFFFF
        }
    }

    /// Load FLAGS from a POPF / IRET image. In protected mode IOPL only
    /// changes at CPL 0 and IF only when CPL <= IOPL.
    pub(crate) fn set_flags_image(&mut self, sz: u32, v: u32) {
        let mut mask = if sz == 32 { self.popf_mask32 } else { 0x7FD5 };
        if self.pe() {
            if self.cpl > 0 {
                mask &= !0x3000;
            }
            if self.cpl > self.iopl() {
                mask &= !flags::IF;
            }
        }
        self.eflags = (self.eflags & !mask) | (v & mask) | 2;
    }

    /// #GP(0) unless running at CPL 0 (privileged instructions).
    pub(crate) fn require_cpl0(&self) -> R<()> {
        if self.pe() && self.cpl != 0 {
            Err(GP)
        } else {
            Ok(())
        }
    }

    /// #GP(0) unless CPL <= IOPL (CLI, STI; in V86 also PUSHF, POPF, INT n, IRET).
    pub(crate) fn require_iopl(&self) -> R<()> {
        if self.pe() && self.cpl > self.iopl() {
            Err(GP)
        } else {
            Ok(())
        }
    }

    /// I/O permission: allowed if CPL <= IOPL (not in V86), otherwise every
    /// port's bit in the TSS I/O permission bitmap must be clear.
    pub(crate) fn check_io<B: Bus>(&mut self, bus: &mut B, port: u16, bytes: u32) -> R<()> {
        if !self.pe() || (!self.vm() && self.cpl <= self.iopl()) {
            return Ok(());
        }
        if self.tr.sel & 0xFFFC == 0 || self.tr.attr & 0x08 == 0 || self.tr.limit < 0x67 {
            return Err(GP);
        }
        let io_base = self.sys_read(bus, self.tr.base + 0x66, 16)?;
        let off = io_base + port as u32 / 8;
        if off + 1 > self.tr.limit {
            return Err(GP);
        }
        let bits = self.sys_read(bus, self.tr.base + off, 16)?;
        let mask = ((1u32 << bytes) - 1) << (port % 8);
        if bits & mask != 0 {
            return Err(GP);
        }
        Ok(())
    }
}
