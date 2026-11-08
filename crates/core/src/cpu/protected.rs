//! Protected mode: descriptors, segment loading, privilege checks, far
//! control transfers (including call gates), interrupt/trap gates and the
//! system instructions.

use super::flags::*;
use super::*;

/// A segment descriptor as read from the GDT/LDT/IDT.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Desc {
    pub base: u32,
    /// Limit in bytes (granularity applied).
    pub limit: u32,
    /// Access byte | flags (D/B bit 14, G bit 15), as stored in seg_attr.
    pub attr: u16,
    /// Raw words for gates: selector (word 1) and offsets (words 0 and 3).
    pub raw_lo: u32,
    pub raw_hi: u32,
}

impl Desc {
    fn from_raw(lo: u32, hi: u32) -> Desc {
        let base = (lo >> 16) | (hi & 0xFF) << 16 | (hi & 0xFF00_0000);
        let mut limit = (lo & 0xFFFF) | (hi & 0x000F_0000);
        if hi & 0x0080_0000 != 0 {
            limit = limit << 12 | 0xFFF;
        }
        let attr = ((hi >> 8) & 0xFF) as u16 | ((hi >> 8) & 0xF000) as u16;
        Desc { base, limit, attr, raw_lo: lo, raw_hi: hi }
    }

    pub fn present(&self) -> bool {
        self.attr & 0x80 != 0
    }
    pub fn dpl(&self) -> u8 {
        ((self.attr >> 5) & 3) as u8
    }
    /// Code or data (S bit set).
    pub fn is_seg(&self) -> bool {
        self.attr & 0x10 != 0
    }
    pub fn is_code(&self) -> bool {
        self.is_seg() && self.attr & 0x08 != 0
    }
    pub fn is_data(&self) -> bool {
        self.is_seg() && self.attr & 0x08 == 0
    }
    pub fn conforming(&self) -> bool {
        self.is_code() && self.attr & 0x04 != 0
    }
    pub fn readable(&self) -> bool {
        self.is_data() || self.attr & 0x02 != 0
    }
    pub fn writable(&self) -> bool {
        self.is_data() && self.attr & 0x02 != 0
    }
    /// System descriptor type (S bit clear).
    pub fn sys_type(&self) -> u8 {
        (self.attr & 0x0F) as u8
    }
    // Gate fields.
    fn gate_sel(&self) -> u16 {
        (self.raw_lo >> 16) as u16
    }
    fn gate_offset(&self) -> u32 {
        let lo = self.raw_lo & 0xFFFF;
        if self.sys_type() & 8 != 0 {
            lo | (self.raw_hi & 0xFFFF_0000)
        } else {
            lo
        }
    }
    fn gate_params(&self) -> u32 {
        self.raw_hi & 0x1F
    }
}

fn sel_err(sel: u16) -> u32 {
    (sel & 0xFFFC) as u32
}

impl Cpu {
    /// Read the descriptor for `sel` (GDT or LDT). Returns it and its address.
    pub(crate) fn read_desc<B: Bus>(&mut self, bus: &mut B, sel: u16) -> R<(Desc, u32)> {
        let (base, limit) = if sel & 4 != 0 {
            if self.ldtr.sel & 0xFFFC == 0 {
                return Err(gp(sel_err(sel)));
            }
            (self.ldtr.base, self.ldtr.limit)
        } else {
            (self.gdtr.base, self.gdtr.limit as u32)
        };
        let idx = (sel & 0xFFF8) as u32;
        if idx + 7 > limit {
            return Err(gp(sel_err(sel)));
        }
        let a = base.wrapping_add(idx);
        let lo = self.sys_read(bus, a, 32)?;
        let hi = self.sys_read(bus, a.wrapping_add(4), 32)?;
        Ok((Desc::from_raw(lo, hi), a))
    }

    /// Set the accessed bit of a code/data descriptor in memory.
    fn mark_accessed<B: Bus>(&mut self, bus: &mut B, d: &Desc, addr: u32) -> R<()> {
        if d.is_seg() && d.attr & 1 == 0 {
            let a = addr.wrapping_add(5);
            let v = self.sys_read(bus, a, 8)?;
            self.sys_write(bus, a, 8, v | 1)?;
        }
        Ok(())
    }

    fn set_cache(&mut self, seg: usize, sel: u16, d: &Desc) {
        self.sregs[seg] = sel;
        self.seg_base[seg] = d.base;
        self.seg_limit[seg] = d.limit;
        self.seg_attr[seg] = d.attr | 1;
    }

    /// Protected-mode load of DS/ES/FS/GS/SS (CS goes through far transfers).
    pub(crate) fn load_seg_pm<B: Bus>(&mut self, bus: &mut B, seg: usize, sel: u16) -> R<()> {
        let rpl = (sel & 3) as u8;
        if sel & 0xFFFC == 0 {
            if seg == SS {
                return Err(GP);
            }
            // Null selector: loads fine, faults on use.
            self.sregs[seg] = sel;
            self.seg_attr[seg] = 0;
            self.seg_base[seg] = 0;
            self.seg_limit[seg] = 0;
            return Ok(());
        }
        let (d, addr) = self.read_desc(bus, sel)?;
        if seg == SS {
            if rpl != self.cpl || d.dpl() != self.cpl || !d.writable() {
                return Err(gp(sel_err(sel)));
            }
            if !d.present() {
                return Err(Fault(12, sel_err(sel)));
            }
        } else {
            if !d.readable() || !d.is_seg() {
                return Err(gp(sel_err(sel)));
            }
            if (d.is_data() || !d.conforming()) && d.dpl() < self.cpl.max(rpl) {
                return Err(gp(sel_err(sel)));
            }
            if !d.present() {
                return Err(Fault(11, sel_err(sel)));
            }
        }
        self.mark_accessed(bus, &d, addr)?;
        self.set_cache(seg, sel, &d);
        Ok(())
    }

    /// Load CS for a transfer to `sel` (already validated), at privilege `cpl`.
    fn load_cs(&mut self, sel: u16, d: &Desc, cpl: u8) {
        self.set_cache(CS, (sel & 0xFFFC) | cpl as u16, d);
        self.cpl = cpl;
    }

    /// Far JMP or CALL to sel:off (both modes). `osz` sizes the pushes.
    pub(crate) fn far_transfer<B: Bus>(&mut self, bus: &mut B, sel: u16, off: u32, call: bool) -> R<()> {
        let osz = self.osz();
        if !self.pe() || self.vm() {
            if off > 0xFFFF && !call {
                return Err(GP);
            }
            if call {
                let (old_cs, old_ip) = (self.sregs[CS], self.eip);
                self.push(bus, osz, old_cs as u32)?;
                self.push(bus, osz, old_ip)?;
            }
            if off > self.seg_limit[CS].max(0xFFFF) {
                return Err(GP);
            }
            self.load_cs_realish(sel);
            self.eip = off;
            return Ok(());
        }
        if sel & 0xFFFC == 0 {
            return Err(GP);
        }
        let (d, addr) = self.read_desc(bus, sel)?;
        let rpl = (sel & 3) as u8;
        if d.is_code() {
            if d.conforming() {
                if d.dpl() > self.cpl {
                    return Err(gp(sel_err(sel)));
                }
            } else if rpl > self.cpl || d.dpl() != self.cpl {
                return Err(gp(sel_err(sel)));
            }
            if !d.present() {
                return Err(Fault(11, sel_err(sel)));
            }
            let off = if osz == 16 { off & 0xFFFF } else { off };
            if off > d.limit {
                return Err(GP);
            }
            if call {
                let (old_cs, old_ip) = (self.sregs[CS], self.eip);
                self.push(bus, osz, old_cs as u32)?;
                self.push(bus, osz, old_ip)?;
            }
            self.mark_accessed(bus, &d, addr)?;
            let cpl = self.cpl;
            self.load_cs(sel, &d, cpl);
            self.eip = off;
            return Ok(());
        }
        // Call gate (286: type 4, 386: type 12).
        if d.is_seg() || !matches!(d.sys_type(), 4 | 12) {
            // Task gates and TSS jumps (task switching) aren't emulated.
            return Err(gp(sel_err(sel)));
        }
        if d.dpl() < self.cpl || d.dpl() < rpl {
            return Err(gp(sel_err(sel)));
        }
        if !d.present() {
            return Err(Fault(11, sel_err(sel)));
        }
        let gate32 = d.sys_type() == 12;
        let tsel = d.gate_sel();
        let toff = d.gate_offset();
        if tsel & 0xFFFC == 0 {
            return Err(GP);
        }
        let (td, taddr) = self.read_desc(bus, tsel)?;
        if !td.is_code() || td.dpl() > self.cpl {
            return Err(gp(sel_err(tsel)));
        }
        if !td.present() {
            return Err(Fault(11, sel_err(tsel)));
        }
        if toff > td.limit {
            return Err(GP);
        }
        self.mark_accessed(bus, &td, taddr)?;
        let gsz = if gate32 { 32 } else { 16 };
        if !call {
            if !td.conforming() && td.dpl() != self.cpl {
                return Err(gp(sel_err(tsel)));
            }
            let cpl = self.cpl;
            self.load_cs(tsel, &td, cpl);
            self.eip = toff;
            return Ok(());
        }
        if !td.conforming() && td.dpl() < self.cpl {
            // Call to a more privileged level: switch stacks via the TSS.
            let new_cpl = td.dpl();
            let (new_ss, new_esp) = self.tss_stack(bus, new_cpl)?;
            let (old_ss, old_esp) = (self.sregs[SS], self.sp());
            let (old_cs, old_ip) = (self.sregs[CS], self.eip);
            // Copy parameters from the old stack.
            let n = d.gate_params();
            let mut params = Vec::with_capacity(n as usize);
            for k in 0..n {
                params.push(self.stack_peek(bus, k * (gsz / 8), gsz)?);
            }
            self.switch_stack(bus, new_ss, new_esp, new_cpl)?;
            self.cpl = new_cpl; // pushes to the inner stack are supervisor accesses
            self.push(bus, gsz, old_ss as u32)?;
            self.push(bus, gsz, old_esp)?;
            for p in params.iter().rev() {
                self.push(bus, gsz, *p)?;
            }
            self.push(bus, gsz, old_cs as u32)?;
            self.push(bus, gsz, old_ip)?;
            self.load_cs(tsel, &td, new_cpl);
        } else {
            let (old_cs, old_ip) = (self.sregs[CS], self.eip);
            self.push(bus, gsz, old_cs as u32)?;
            self.push(bus, gsz, old_ip)?;
            let cpl = self.cpl;
            self.load_cs(tsel, &td, cpl);
        }
        self.eip = toff;
        Ok(())
    }

    /// The inner-level stack for privilege `pl` from the current TSS.
    fn tss_stack<B: Bus>(&mut self, bus: &mut B, pl: u8) -> R<(u16, u32)> {
        if self.tr.sel & 0xFFFC == 0 {
            return Err(Fault(10, 0));
        }
        let tss32 = self.tr.attr & 0x08 != 0;
        let (off, n) = if tss32 { (4 + 8 * pl as u32, 8) } else { (2 + 4 * pl as u32, 4) };
        if off + n - 1 > self.tr.limit {
            return Err(Fault(10, sel_err(self.tr.sel)));
        }
        let b = self.tr.base;
        if tss32 {
            let esp = self.sys_read(bus, b + off, 32)?;
            let ss = self.sys_read(bus, b + off + 4, 16)? as u16;
            Ok((ss, esp))
        } else {
            let sp = self.sys_read(bus, b + off, 16)?;
            let ss = self.sys_read(bus, b + off + 2, 16)? as u16;
            Ok((ss, sp))
        }
    }

    /// Load SS:ESP for privilege level `pl` (validating SS against `pl`).
    fn switch_stack<B: Bus>(&mut self, bus: &mut B, ss: u16, esp: u32, pl: u8) -> R<()> {
        if ss & 0xFFFC == 0 {
            return Err(Fault(10, 0));
        }
        let (d, addr) = self.read_desc(bus, ss)?;
        if (ss & 3) as u8 != pl || d.dpl() != pl || !d.writable() {
            return Err(Fault(10, sel_err(ss)));
        }
        if !d.present() {
            return Err(Fault(12, sel_err(ss)));
        }
        self.mark_accessed(bus, &d, addr)?;
        self.set_cache(SS, ss, &d);
        self.regs[ESP] = esp;
        Ok(())
    }

    /// After returning to an outer level, null data segment registers the
    /// new privilege level may not use.
    fn invalidate_segs(&mut self) {
        for seg in [ES, DS, FS, GS] {
            let a = self.seg_attr[seg];
            let dpl = ((a >> 5) & 3) as u8;
            let code = a & 0x08 != 0;
            let conforming = code && a & 0x04 != 0;
            if a & ATTR_PRESENT != 0 && (!code || !conforming) && dpl < self.cpl {
                self.sregs[seg] = 0;
                self.seg_attr[seg] = 0;
            }
        }
    }

    /// Far RET (with `extra` bytes of parameters to release).
    pub(crate) fn far_return<B: Bus>(&mut self, bus: &mut B, extra: u32) -> R<()> {
        let osz = self.osz();
        let n = osz / 8;
        let ip = self.stack_peek(bus, 0, osz)?;
        let cs = self.stack_peek(bus, n, osz)? as u16;
        if !self.pe() || self.vm() {
            if ip > self.seg_limit[CS].max(0xFFFF) {
                return Err(GP);
            }
            self.load_cs_realish(cs);
            self.eip = ip;
            let sp = self.sp().wrapping_add(2 * n + extra);
            self.set_sp(sp);
            return Ok(());
        }
        let rpl = (cs & 3) as u8;
        if cs & 0xFFFC == 0 || rpl < self.cpl {
            return Err(gp(sel_err(cs)));
        }
        let (d, addr) = self.read_desc(bus, cs)?;
        if !d.is_code() || (d.conforming() && d.dpl() > rpl) || (!d.conforming() && d.dpl() != rpl) {
            return Err(gp(sel_err(cs)));
        }
        if !d.present() {
            return Err(Fault(11, sel_err(cs)));
        }
        let ip = if osz == 16 { ip & 0xFFFF } else { ip };
        if ip > d.limit {
            return Err(GP);
        }
        if rpl == self.cpl {
            self.mark_accessed(bus, &d, addr)?;
            self.load_cs(cs, &d, rpl);
            self.eip = ip;
            let sp = self.sp().wrapping_add(2 * n + extra);
            self.set_sp(sp);
            return Ok(());
        }
        // Return to an outer privilege level: SS:ESP follow the parameters.
        let new_esp = self.stack_peek(bus, 2 * n + extra, osz)?;
        let new_ss = self.stack_peek(bus, 3 * n + extra, osz)? as u16;
        self.mark_accessed(bus, &d, addr)?;
        self.switch_stack(bus, new_ss, new_esp, rpl)?;
        self.load_cs(cs, &d, rpl);
        self.eip = ip;
        let sp = self.sp().wrapping_add(extra);
        self.set_sp(sp);
        self.invalidate_segs();
        Ok(())
    }

    /// IRET (both modes).
    pub(crate) fn iret<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let osz = self.osz();
        let n = osz / 8;
        let ip = self.stack_peek(bus, 0, osz)?;
        let cs = self.stack_peek(bus, n, osz)? as u16;
        let fl = self.stack_peek(bus, 2 * n, osz)?;
        if self.vm() && self.iopl() < 3 {
            return Err(GP);
        }
        if !self.pe() || self.vm() {
            if ip > self.seg_limit[CS].max(0xFFFF) {
                return Err(GP);
            }
            self.load_cs_realish(cs);
            self.eip = ip;
            let sp = self.sp().wrapping_add(3 * n);
            self.set_sp(sp);
            self.set_flags_image(osz, fl);
            return Ok(());
        }
        if osz == 32 && fl & VM_FLAG != 0 && self.cpl == 0 {
            return self.iret_to_v86(bus, ip, cs, fl);
        }
        let rpl = (cs & 3) as u8;
        if cs & 0xFFFC == 0 || rpl < self.cpl {
            return Err(gp(sel_err(cs)));
        }
        let (d, addr) = self.read_desc(bus, cs)?;
        if !d.is_code() || (d.conforming() && d.dpl() > rpl) || (!d.conforming() && d.dpl() != rpl) {
            return Err(gp(sel_err(cs)));
        }
        if !d.present() {
            return Err(Fault(11, sel_err(cs)));
        }
        let ip = if osz == 16 { ip & 0xFFFF } else { ip };
        if ip > d.limit {
            return Err(GP);
        }
        if rpl == self.cpl {
            self.mark_accessed(bus, &d, addr)?;
            self.load_cs(cs, &d, rpl);
            self.eip = ip;
            let sp = self.sp().wrapping_add(3 * n);
            self.set_sp(sp);
            self.set_flags_image(osz, fl);
            return Ok(());
        }
        let new_esp = self.stack_peek(bus, 3 * n, osz)?;
        let new_ss = self.stack_peek(bus, 4 * n, osz)? as u16;
        // Flags are restored with the old privilege's rights.
        self.set_flags_image(osz, fl);
        self.mark_accessed(bus, &d, addr)?;
        self.switch_stack(bus, new_ss, new_esp, rpl)?;
        self.load_cs(cs, &d, rpl);
        self.eip = ip;
        self.invalidate_segs();
        Ok(())
    }

    /// Load CS in real mode or V86 mode.
    fn load_cs_realish(&mut self, sel: u16) {
        if self.vm() {
            self.load_seg_v86(CS, sel);
        } else {
            self.load_seg_real(CS, sel);
        }
    }

    /// IRET from protected mode (CPL 0) back into virtual-8086 mode.
    fn iret_to_v86<B: Bus>(&mut self, bus: &mut B, ip: u32, cs: u16, fl: u32) -> R<()> {
        let mut v = [0u32; 6];
        for (k, slot) in v.iter_mut().enumerate() {
            *slot = self.stack_peek(bus, 12 + 4 * k as u32, 32)?;
        }
        let [esp, ss, es, ds, fs, gs] = v;
        self.eflags = (fl & 0x0027_7FD5) | 2 | VM_FLAG;
        self.cpl = 3;
        self.load_seg_v86(CS, cs);
        self.load_seg_v86(SS, ss as u16);
        self.load_seg_v86(ES, es as u16);
        self.load_seg_v86(DS, ds as u16);
        self.load_seg_v86(FS, fs as u16);
        self.load_seg_v86(GS, gs as u16);
        self.regs[ESP] = esp;
        self.eip = ip & 0xFFFF;
        Ok(())
    }

    /// Protected-mode interrupt delivery through the IDT.
    pub(crate) fn interrupt_pm<B: Bus>(&mut self, bus: &mut B, vector: u8, ret_eip: u32, kind: IntKind) -> R<()> {
        let ext = matches!(kind, IntKind::Hardware) as u32;
        let idt_err = vector as u32 * 8 + 2 + ext;
        let entry = vector as u32 * 8;
        if entry + 7 > self.idtr.limit as u32 {
            return Err(gp(idt_err));
        }
        let a = self.idtr.base.wrapping_add(entry);
        let lo = self.sys_read(bus, a, 32)?;
        let hi = self.sys_read(bus, a.wrapping_add(4), 32)?;
        let g = Desc::from_raw(lo, hi);
        let ty = g.sys_type();
        if g.is_seg() || !matches!(ty, 6 | 7 | 14 | 15) {
            // Task gates (5) aren't emulated.
            return Err(gp(idt_err));
        }
        if kind == IntKind::Software && g.dpl() < self.cpl {
            return Err(gp(idt_err));
        }
        if !g.present() {
            return Err(Fault(11, idt_err));
        }
        let gsz = if ty & 8 != 0 { 32 } else { 16 };
        let tsel = g.gate_sel();
        let toff = g.gate_offset();
        if tsel & 0xFFFC == 0 {
            return Err(gp(ext));
        }
        let (td, taddr) = self.read_desc(bus, tsel)?;
        if !td.is_code() || td.dpl() > self.cpl {
            return Err(gp(sel_err(tsel) + ext));
        }
        if !td.present() {
            return Err(Fault(11, sel_err(tsel) + ext));
        }
        if toff > td.limit {
            return Err(gp(ext));
        }
        let error = match kind {
            IntKind::Exception(code) if matches!(vector, 8 | 10..=14 | 17) => Some(code),
            _ => None,
        };
        let from_vm = self.vm();
        if from_vm && (td.dpl() != 0 || td.conforming()) {
            return Err(gp(sel_err(tsel) + ext));
        }
        let old_flags = self.flags_image(32) | if from_vm { VM_FLAG } else { 0 };
        let (old_cs, old_ss, old_esp) = (self.sregs[CS], self.sregs[SS], self.sp());
        let old_segs = [self.sregs[GS], self.sregs[FS], self.sregs[DS], self.sregs[ES]];
        if !td.conforming() && td.dpl() < self.cpl {
            let new_cpl = td.dpl();
            let (ss, esp) = self.tss_stack(bus, new_cpl)?;
            let saved = (self.sregs[SS], self.seg_base[SS], self.seg_limit[SS], self.seg_attr[SS], self.regs[ESP], self.cpl);
            self.switch_stack(bus, ss, esp, new_cpl)?;
            self.cpl = new_cpl; // pushes to the inner stack are supervisor accesses
            if from_vm {
                self.eflags &= !VM_FLAG;
            }
            let pushes = (|| {
                if from_vm {
                    for s in old_segs {
                        self.push(bus, 32, s as u32)?;
                    }
                }
                self.push(bus, gsz, old_ss as u32)?;
                self.push(bus, gsz, old_esp)?;
                self.push(bus, gsz, old_flags)?;
                self.push(bus, gsz, old_cs as u32)?;
                self.push(bus, gsz, ret_eip)?;
                if let Some(code) = error {
                    self.push(bus, gsz, code)?;
                }
                Ok(())
            })();
            if let Err(f) = pushes {
                // Undo the stack switch so the fault is taken from the old context.
                (self.sregs[SS], self.seg_base[SS], self.seg_limit[SS], self.seg_attr[SS], self.regs[ESP], self.cpl) = saved;
                if from_vm {
                    self.eflags |= VM_FLAG;
                }
                return Err(f);
            }
            if from_vm {
                for seg in [ES, DS, FS, GS] {
                    self.sregs[seg] = 0;
                    self.seg_attr[seg] = 0;
                    self.seg_base[seg] = 0;
                }
            }
            self.mark_accessed(bus, &td, taddr)?;
            self.load_cs(tsel, &td, new_cpl);
        } else {
            self.push(bus, gsz, old_flags)?;
            self.push(bus, gsz, old_cs as u32)?;
            self.push(bus, gsz, ret_eip)?;
            if let Some(code) = error {
                self.push(bus, gsz, code)?;
            }
            self.mark_accessed(bus, &td, taddr)?;
            let cpl = self.cpl;
            self.load_cs(tsel, &td, cpl);
        }
        self.eip = if gsz == 16 { toff & 0xFFFF } else { toff };
        self.eflags &= !(TF | (1 << 14) | (1 << 16) | (1 << 17)); // TF NT RF VM
        if ty & 1 == 0 {
            self.eflags &= !IF; // interrupt gate
        }
        Ok(())
    }

    /// Writing CR0: handle mode changes.
    pub(crate) fn write_cr0(&mut self, v: u32) -> R<()> {
        let v = v | 0x10; // ET is hardwired on the 486
        if v & CR0_PG != 0 && v & CR0_PE == 0 {
            return Err(GP);
        }
        if (v ^ self.cr0) & (CR0_PG | CR0_PE | CR0_WP) != 0 {
            self.tlb_flush();
        }
        self.cr0 = v;
        if !self.pe() {
            self.cpl = 0;
        }
        Ok(())
    }

    // ---------------------------------------------------------------- 0F 00 / LAR / LSL

    /// LLDT.
    pub(crate) fn lldt<B: Bus>(&mut self, bus: &mut B, sel: u16) -> R<()> {
        if sel & 0xFFFC == 0 {
            self.ldtr = SysSeg::default();
            return Ok(());
        }
        if sel & 4 != 0 {
            return Err(gp(sel_err(sel)));
        }
        let (d, _) = self.read_desc(bus, sel)?;
        if d.is_seg() || d.sys_type() != 2 {
            return Err(gp(sel_err(sel)));
        }
        if !d.present() {
            return Err(Fault(11, sel_err(sel)));
        }
        self.ldtr = SysSeg { sel, base: d.base, limit: d.limit, attr: d.attr };
        Ok(())
    }

    /// LTR: load an available TSS and mark it busy.
    pub(crate) fn ltr<B: Bus>(&mut self, bus: &mut B, sel: u16) -> R<()> {
        if sel & 0xFFFC == 0 || sel & 4 != 0 {
            return Err(gp(sel_err(sel)));
        }
        let (d, addr) = self.read_desc(bus, sel)?;
        if d.is_seg() || !matches!(d.sys_type(), 1 | 9) {
            return Err(gp(sel_err(sel)));
        }
        if !d.present() {
            return Err(Fault(11, sel_err(sel)));
        }
        let a = addr.wrapping_add(5);
        let v = self.sys_read(bus, a, 8)?;
        self.sys_write(bus, a, 8, v | 2)?;
        self.tr = SysSeg { sel, base: d.base, limit: d.limit, attr: d.attr | 2 };
        Ok(())
    }

    /// For LAR/LSL/VERR/VERW: the descriptor if visible at CPL/RPL.
    fn visible_desc<B: Bus>(&mut self, bus: &mut B, sel: u16) -> Option<Desc> {
        if sel & 0xFFFC == 0 {
            return None;
        }
        let (d, _) = self.read_desc(bus, sel).ok()?;
        let rpl = (sel & 3) as u8;
        if !d.conforming() && (d.dpl() < self.cpl || d.dpl() < rpl) {
            return None;
        }
        Some(d)
    }

    /// LAR (lsl = false) / LSL: Some(value) sets ZF.
    pub(crate) fn lar_lsl<B: Bus>(&mut self, bus: &mut B, sel: u16, lsl: bool) -> Option<u32> {
        let d = self.visible_desc(bus, sel)?;
        if !d.is_seg() {
            let ok = if lsl { matches!(d.sys_type(), 1 | 2 | 3 | 9 | 11) } else { matches!(d.sys_type(), 1..=7 | 9 | 11 | 12) };
            if !ok {
                return None;
            }
        }
        Some(if lsl { d.limit } else { d.raw_hi & 0x00F0_FF00 })
    }

    /// VERR (write = false) / VERW.
    pub(crate) fn verify<B: Bus>(&mut self, bus: &mut B, sel: u16, write: bool) -> bool {
        match self.visible_desc(bus, sel) {
            Some(d) if write => d.writable(),
            Some(d) => d.is_seg() && d.readable(),
            None => false,
        }
    }
}
