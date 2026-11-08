//! Hardware task switching through 32-bit TSSs: far JMP/CALL to a TSS or a
//! task gate, interrupts and exceptions through task gates, and IRET with
//! NT set (return to the previous task). 16-bit (286) TSSs are not
//! supported and raise #GP.


/// EFLAGS.NT (nested task).
pub(crate) const NT: u32 = 1 << 14;
use super::protected::Desc;
use super::*;

// 32-bit TSS layout.
const T_LINK: u32 = 0x00;
const T_CR3: u32 = 0x1C;
const T_EIP: u32 = 0x20;
const T_EFLAGS: u32 = 0x24;
const T_REGS: u32 = 0x28; // EAX..EDI, 4 bytes each
const T_SEGS: u32 = 0x48; // ES, CS, SS, DS, FS, GS, 4 bytes each
const T_LDT: u32 = 0x60;

/// What started a task switch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TaskKind {
    Jmp,
    Call,
    /// Interrupt or exception through a task gate.
    Int,
    Iret,
}

fn sel_err(sel: u16) -> u32 {
    (sel & 0xFFFC) as u32
}

/// Invalid TSS (#TS).
fn ts(sel: u16) -> Fault {
    Fault(10, sel_err(sel))
}

impl Cpu {
    /// Set or clear the busy bit of the TSS descriptor for `sel` in the GDT.
    fn set_tss_busy<B: Bus>(&mut self, bus: &mut B, sel: u16, busy: bool) -> R<()> {
        let a = self.gdtr.base.wrapping_add((sel & 0xFFF8) as u32 + 5);
        let v = self.sys_read(bus, a, 8)?;
        let v = if busy { v | 2 } else { v & !2 };
        self.sys_write(bus, a, 8, v)
    }

    /// Switch to the task whose TSS descriptor (`d`, for `sel`) has been
    /// validated by the caller for access rights. `ret_eip` is the EIP saved
    /// in the outgoing task; `error` is pushed on the new task's stack.
    pub(crate) fn task_switch<B: Bus>(
        &mut self,
        bus: &mut B,
        sel: u16,
        d: &Desc,
        kind: TaskKind,
        ret_eip: u32,
        error: Option<u32>,
    ) -> R<()> {
        if sel & 4 != 0 || d.is_seg() {
            return Err(gp(sel_err(sel)));
        }
        let busy = d.sys_type() == 11;
        match d.sys_type() {
            9 if kind != TaskKind::Iret => {}
            11 if kind == TaskKind::Iret => {}
            1 | 3 => return Err(gp(sel_err(sel))), // 286 TSS: unsupported
            _ => return Err(gp(sel_err(sel))),
        }
        if !d.present() {
            return Err(Fault(11, sel_err(sel)));
        }
        if d.limit < 0x67 {
            return Err(ts(sel));
        }
        if self.tr.limit < 0x5F && self.tr.sel & 0xFFFC != 0 {
            return Err(ts(self.tr.sel));
        }

        // Save the outgoing task's state.
        let old = self.tr;
        let mut fl = self.eflags;
        if kind == TaskKind::Iret {
            fl &= !NT;
        }
        if old.sel & 0xFFFC != 0 {
            let b = old.base;
            self.sys_write(bus, b.wrapping_add(T_EIP), 32, ret_eip)?;
            self.sys_write(bus, b.wrapping_add(T_EFLAGS), 32, fl)?;
            for i in 0..8 {
                let v = self.regs[i];
                self.sys_write(bus, b.wrapping_add(T_REGS + 4 * i as u32), 32, v)?;
            }
            for s in 0..6 {
                let v = self.sregs[s] as u32;
                self.sys_write(bus, b.wrapping_add(T_SEGS + 4 * s as u32), 32, v)?;
            }
            if matches!(kind, TaskKind::Jmp | TaskKind::Iret) {
                self.set_tss_busy(bus, old.sel, false)?;
            }
        }

        // Link the new task back to the old one for CALL and interrupts.
        let nb = d.base;
        if matches!(kind, TaskKind::Call | TaskKind::Int) {
            self.sys_write(bus, nb.wrapping_add(T_LINK), 16, old.sel as u32)?;
        }
        if !busy {
            self.set_tss_busy(bus, sel, true)?;
        }
        self.tr = SysSeg { sel, base: nb, limit: d.limit, attr: (d.attr & !0x0F) | 11 };
        self.cr0 |= CR0_TS;

        // Read the incoming task's state.
        let cr3 = self.sys_read(bus, nb.wrapping_add(T_CR3), 32)?;
        let eip = self.sys_read(bus, nb.wrapping_add(T_EIP), 32)?;
        let mut eflags = self.sys_read(bus, nb.wrapping_add(T_EFLAGS), 32)?;
        let mut regs = [0u32; 8];
        for (i, r) in regs.iter_mut().enumerate() {
            *r = self.sys_read(bus, nb.wrapping_add(T_REGS + 4 * i as u32), 32)?;
        }
        let mut segs = [0u16; 6];
        for (s, v) in segs.iter_mut().enumerate() {
            *v = self.sys_read(bus, nb.wrapping_add(T_SEGS + 4 * s as u32), 16)? as u16;
        }
        let ldt = self.sys_read(bus, nb.wrapping_add(T_LDT), 16)? as u16;

        if self.cr0 & CR0_PG != 0 {
            self.cr3 = cr3;
            self.tlb_flush();
        }
        if matches!(kind, TaskKind::Call | TaskKind::Int) {
            eflags |= NT;
        }
        self.eflags = eflags | 2;
        self.regs = regs;
        self.eip = eip;

        // From here on, faults belong to the new task: don't roll back into
        // the old one.
        self.restore_regs = self.regs;
        self.restore_eflags = self.eflags;
        self.start_eip = self.eip;

        // LDT, then the segment registers, with the new task's privilege.
        self.lldt(bus, ldt).map_err(|_| ts(ldt))?;
        if self.vm() {
            for (s, &v) in segs.iter().enumerate() {
                self.load_seg_v86(s, v);
            }
            self.cpl = 3;
        } else {
            let cs = segs[CS];
            if cs & 0xFFFC == 0 {
                return Err(ts(cs));
            }
            let (cd, _) = self.read_desc(bus, cs).map_err(|_| ts(cs))?;
            let rpl = (cs & 3) as u8;
            if !cd.is_code() || (cd.conforming() && cd.dpl() > rpl) || (!cd.conforming() && cd.dpl() != rpl) {
                return Err(ts(cs));
            }
            if !cd.present() {
                return Err(Fault(11, sel_err(cs)));
            }
            self.sregs[CS] = cs;
            self.seg_base[CS] = cd.base;
            self.seg_limit[CS] = cd.limit;
            self.seg_attr[CS] = cd.attr | 1;
            self.cpl = rpl;
            for s in [SS, DS, ES, FS, GS] {
                self.load_seg_pm(bus, s, segs[s]).map_err(|f| if f.0 == 13 { ts(segs[s]) } else { f })?;
            }
            if self.eip > self.seg_limit[CS] {
                return Err(GP);
            }
        }
        if let Some(code) = error {
            let osz = 32;
            self.push(bus, osz, code)?;
        }
        self.cycles += 300;
        Ok(())
    }

    /// IRET with NT set: return to the task in the current TSS's back link.
    pub(crate) fn task_return<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let link = self.sys_read(bus, self.tr.base, 16)? as u16;
        if link & 4 != 0 || link & 0xFFFC == 0 {
            return Err(ts(link));
        }
        let (d, _) = self.read_desc(bus, link).map_err(|_| ts(link))?;
        if d.is_seg() || d.sys_type() != 11 {
            return Err(ts(link));
        }
        let eip = self.eip;
        self.task_switch(bus, link, &d, TaskKind::Iret, eip, None)
    }
}
