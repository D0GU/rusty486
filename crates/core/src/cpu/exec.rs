//! One-byte opcode execution.

use super::alu::{mask, sext};
use super::flags::*;
use super::*;

/// String instruction kinds.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Str {
    Movs,
    Cmps,
    Stos,
    Lods,
    Scas,
    Ins,
    Outs,
}

/// Maximum string iterations per `step`, so interrupts can be serviced
/// during long REP operations (the instruction is simply restarted).
const REP_BATCH: u32 = 4096;

impl Cpu {
    pub(crate) fn exec<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        loop {
            let op = self.fetch8(bus)?;
            match op {
                0x26 => self.seg_ovr = Some(ES),
                0x2E => self.seg_ovr = Some(CS),
                0x36 => self.seg_ovr = Some(SS),
                0x3E => self.seg_ovr = Some(DS),
                0x64 => self.seg_ovr = Some(FS),
                0x65 => self.seg_ovr = Some(GS),
                0x66 => self.op32 = true,
                0x67 => self.a32 = true,
                0xF0 => self.lock = true,
                0xF2 => self.rep = Rep::Repne,
                0xF3 => self.rep = Rep::Repe,
                _ => {
                    if self.lock {
                        self.check_lock(bus, op)?;
                    }
                    return self.exec_op(bus, op);
                }
            }
        }
    }

    /// LOCK is only valid on read-modify-write instructions with a memory
    /// destination; anything else raises #UD.
    fn check_lock<B: Bus>(&mut self, bus: &mut B, op: u8) -> R<()> {
        let at = |cpu: &Cpu, bus: &mut B, n: u32| {
            bus.read8(cpu.seg_base[CS].wrapping_add(cpu.eip.wrapping_add(n)))
        };
        let (modrm, ok) = match op {
            0x00..=0x3F if op & 7 < 2 && op & 0x38 != 0x38 => (at(self, bus, 0), true),
            0x80..=0x83 => {
                let m = at(self, bus, 0);
                (m, (m >> 3) & 7 != 7)
            }
            0x86 | 0x87 => (at(self, bus, 0), true),
            0xF6 | 0xF7 => {
                let m = at(self, bus, 0);
                (m, matches!((m >> 3) & 7, 2 | 3))
            }
            0xFE | 0xFF => {
                let m = at(self, bus, 0);
                (m, (m >> 3) & 7 < 2)
            }
            0x0F => {
                let op2 = at(self, bus, 0);
                let m = at(self, bus, 1);
                let ok = match op2 {
                    0xAB | 0xB3 | 0xBB | 0xB0 | 0xB1 | 0xC0 | 0xC1 => true,
                    0xBA => (m >> 3) & 7 >= 5,
                    _ => false,
                };
                (m, ok)
            }
            _ => (0xC0, false),
        };
        if !ok || modrm >> 6 == 3 {
            return Err(UD);
        }
        Ok(())
    }

    fn exec_op<B: Bus>(&mut self, bus: &mut B, op: u8) -> R<()> {
        let osz = self.osz();
        match op {
            // ALU ops: ADD OR ADC SBB AND SUB XOR CMP in six addressing forms.
            0x00..=0x3F if op & 7 < 6 => {
                let aop = (op >> 3) as u32;
                let write = aop != 7;
                match op & 7 {
                    0 | 1 => {
                        let sz = if op & 1 == 0 { 8 } else { osz };
                        let m = self.modrm(bus)?;
                        let a = self.read_rm(bus, &m, sz)?;
                        let b = self.reg(sz, m.reg);
                        let r = self.alu(aop, sz, a, b);
                        if write {
                            self.write_rm(bus, &m, sz, r)?;
                        }
                    }
                    2 | 3 => {
                        let sz = if op & 1 == 0 { 8 } else { osz };
                        let m = self.modrm(bus)?;
                        let b = self.read_rm(bus, &m, sz)?;
                        let a = self.reg(sz, m.reg);
                        let r = self.alu(aop, sz, a, b);
                        if write {
                            self.set_reg(sz, m.reg, r);
                        }
                    }
                    4 => {
                        let b = self.fetch8(bus)? as u32;
                        let r = self.alu(aop, 8, self.reg(8, 0), b);
                        if write {
                            self.set_reg(8, 0, r);
                        }
                    }
                    _ => {
                        let b = self.fetch_imm(bus, osz)?;
                        let r = self.alu(aop, osz, self.reg(osz, EAX), b);
                        if write {
                            self.set_reg(osz, EAX, r);
                        }
                    }
                }
            }
            0x06 | 0x0E | 0x16 | 0x1E => {
                let v = self.sregs[(op >> 3) as usize] as u32;
                self.push(bus, osz, v)?;
            }
            0x07 | 0x17 | 0x1F => {
                let v = self.pop(bus, osz)?;
                self.load_seg_real((op >> 3) as usize, v as u16);
            }
            0x0F => return self.exec_0f(bus),
            0x27 => self.daa(),
            0x2F => self.das(),
            0x37 => self.aaa(),
            0x3F => self.aas(),

            0x40..=0x47 => {
                let r = (op & 7) as usize;
                let v = self.inc_f(osz, self.reg(osz, r));
                self.set_reg(osz, r, v);
            }
            0x48..=0x4F => {
                let r = (op & 7) as usize;
                let v = self.dec_f(osz, self.reg(osz, r));
                self.set_reg(osz, r, v);
            }
            0x50..=0x57 => {
                let v = self.reg(osz, (op & 7) as usize);
                self.push(bus, osz, v)?;
            }
            0x58..=0x5F => {
                let v = self.pop(bus, osz)?;
                self.set_reg(osz, (op & 7) as usize, v);
            }
            0x60 => {
                // PUSHA
                let sp = self.reg(osz, ESP);
                for r in 0..8 {
                    let v = if r == ESP { sp } else { self.reg(osz, r) };
                    self.push(bus, osz, v)?;
                }
            }
            0x61 => {
                // POPA. The saved SP is discarded, except that POPAD with a
                // 16-bit stack loads the upper half of ESP from it (386 behaviour).
                let mut saved_sp = 0;
                for r in (0..8).rev() {
                    let v = self.pop(bus, osz)?;
                    if r == ESP {
                        saved_sp = v;
                    } else {
                        self.set_reg(osz, r, v);
                    }
                }
                if osz == 32 {
                    self.regs[ESP] = (saved_sp & 0xFFFF_0000) | (self.regs[ESP] & 0xFFFF);
                }
            }
            0x62 => {
                // BOUND
                let m = self.modrm(bus)?;
                if m.is_reg() {
                    return Err(UD);
                }
                let idx = sext(self.reg(osz, m.reg), osz);
                let lo = sext(self.rd(bus, m.seg, m.off, osz)?, osz);
                let hi_off = self.off_plus(&m, osz / 8);
                let hi = sext(self.rd(bus, m.seg, hi_off, osz)?, osz);
                if idx < lo || idx > hi {
                    return Err(BR);
                }
            }
            0x63 => return Err(UD), // ARPL is invalid in real mode
            0x68 => {
                let v = self.fetch_imm(bus, osz)?;
                self.push(bus, osz, v)?;
            }
            0x6A => {
                let v = self.fetch_simm8(bus)?;
                self.push(bus, osz, v & mask(osz))?;
            }
            0x69 | 0x6B => {
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, osz)?;
                let b = if op == 0x69 {
                    self.fetch_imm(bus, osz)?
                } else {
                    self.fetch_simm8(bus)? & mask(osz)
                };
                let r = self.imul_f(osz, a, b);
                self.set_reg(osz, m.reg, r);
            }
            0x6C => self.string(bus, Str::Ins, 8)?,
            0x6D => self.string(bus, Str::Ins, osz)?,
            0x6E => self.string(bus, Str::Outs, 8)?,
            0x6F => self.string(bus, Str::Outs, osz)?,
            0x70..=0x7F => {
                let d = self.fetch_simm8(bus)?;
                if self.cond(op & 0xF) {
                    self.jump(self.eip.wrapping_add(d))?;
                }
            }

            0x80..=0x83 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, sz)?;
                let b = match op {
                    0x81 => self.fetch_imm(bus, sz)?,
                    0x83 => self.fetch_simm8(bus)? & mask(sz),
                    _ => self.fetch8(bus)? as u32,
                };
                let r = self.alu(m.reg as u32, sz, a, b);
                if m.reg != 7 {
                    self.write_rm(bus, &m, sz, r)?;
                }
            }
            0x84 | 0x85 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, sz)?;
                self.logic_f(sz, a & self.reg(sz, m.reg));
            }
            0x86 | 0x87 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, sz)?;
                let b = self.reg(sz, m.reg);
                self.write_rm(bus, &m, sz, b)?;
                self.set_reg(sz, m.reg, a);
            }
            0x88 | 0x89 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let v = self.reg(sz, m.reg);
                self.write_rm(bus, &m, sz, v)?;
            }
            0x8A | 0x8B => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let v = self.read_rm(bus, &m, sz)?;
                self.set_reg(sz, m.reg, v);
            }
            0x8C => {
                // MOV r/m, Sreg
                let m = self.modrm(bus)?;
                if m.reg > 5 {
                    return Err(UD);
                }
                let v = self.sregs[m.reg] as u32;
                if m.is_reg() {
                    self.set_reg(osz, m.rm, v);
                } else {
                    self.wr(bus, m.seg, m.off, 16, v)?;
                }
            }
            0x8D => {
                // LEA
                let m = self.modrm(bus)?;
                if m.is_reg() {
                    return Err(UD);
                }
                self.set_reg(osz, m.reg, m.off);
            }
            0x8E => {
                // MOV Sreg, r/m
                let m = self.modrm(bus)?;
                if m.reg == CS || m.reg > 5 {
                    return Err(UD);
                }
                let v = self.read_rm(bus, &m, 16)?;
                self.load_seg_real(m.reg, v as u16);
            }
            0x8F => {
                // POP r/m. ESP is incremented before the address is computed
                // (a fault restores ESP in `step`).
                let v = self.pop(bus, osz)?;
                let m = self.modrm(bus)?;
                if m.reg != 0 {
                    return Err(UD);
                }
                self.write_rm(bus, &m, osz, v)?;
            }

            0x90 => {} // NOP (XCHG eAX, eAX)
            0x91..=0x97 => {
                let r = (op & 7) as usize;
                let a = self.reg(osz, EAX);
                let b = self.reg(osz, r);
                self.set_reg(osz, EAX, b);
                self.set_reg(osz, r, a);
            }
            0x98 => {
                // CBW / CWDE
                if self.op32 {
                    self.regs[EAX] = self.regs[EAX] as u16 as i16 as i32 as u32;
                } else {
                    let v = self.regs[EAX] as u8 as i8 as i16 as u16 as u32;
                    self.set_reg(16, EAX, v);
                }
            }
            0x99 => {
                // CWD / CDQ
                let neg = self.reg(osz, EAX) & super::alu::msb(osz) != 0;
                self.set_reg(osz, EDX, if neg { 0xFFFF_FFFF } else { 0 });
            }
            0x9A => {
                // CALL ptr16:16/32
                let ip = self.fetch_imm(bus, osz)?;
                let cs = self.fetch16(bus)?;
                self.cycles += 17;
                let (old_cs, old_ip) = (self.sregs[CS], self.eip);
                self.push(bus, osz, old_cs as u32)?;
                self.push(bus, osz, old_ip)?;
                self.jump_far(cs, ip)?;
            }
            0x9B => {
                // WAIT / FWAIT
                if self.cr0 & (CR0_MP | CR0_TS) == (CR0_MP | CR0_TS) {
                    return Err(NM);
                }
            }
            0x9C => {
                let v = self.flags_image(osz);
                self.push(bus, osz, v)?;
            }
            0x9D => {
                let v = self.pop(bus, osz)?;
                self.set_flags_image(osz, v);
            }
            0x9E => {
                // SAHF
                let ah = self.reg(8, 4);
                self.eflags = (self.eflags & !0xFF) | (ah & 0xD5) | 2;
            }
            0x9F => {
                // LAHF
                let f = self.eflags & 0xFF;
                self.set_reg(8, 4, f);
            }

            0xA0..=0xA3 => {
                // MOV between accumulator and moffs
                let sz = if op & 1 == 0 { 8 } else { osz };
                let off = self.fetch_imm(bus, if self.a32 { 32 } else { 16 })?;
                let seg = self.dseg();
                if op < 0xA2 {
                    let v = self.rd(bus, seg, off, sz)?;
                    self.set_reg(sz, EAX, v);
                } else {
                    let v = self.reg(sz, EAX);
                    self.wr(bus, seg, off, sz, v)?;
                }
            }
            0xA4 => self.string(bus, Str::Movs, 8)?,
            0xA5 => self.string(bus, Str::Movs, osz)?,
            0xA6 => self.string(bus, Str::Cmps, 8)?,
            0xA7 => self.string(bus, Str::Cmps, osz)?,
            0xA8 => {
                let b = self.fetch8(bus)? as u32;
                self.logic_f(8, self.reg(8, 0) & b);
            }
            0xA9 => {
                let b = self.fetch_imm(bus, osz)?;
                self.logic_f(osz, self.reg(osz, EAX) & b);
            }
            0xAA => self.string(bus, Str::Stos, 8)?,
            0xAB => self.string(bus, Str::Stos, osz)?,
            0xAC => self.string(bus, Str::Lods, 8)?,
            0xAD => self.string(bus, Str::Lods, osz)?,
            0xAE => self.string(bus, Str::Scas, 8)?,
            0xAF => self.string(bus, Str::Scas, osz)?,

            0xB0..=0xB7 => {
                let v = self.fetch8(bus)? as u32;
                self.set_reg(8, (op & 7) as usize, v);
            }
            0xB8..=0xBF => {
                let v = self.fetch_imm(bus, osz)?;
                self.set_reg(osz, (op & 7) as usize, v);
            }

            0xC0 | 0xC1 | 0xD0..=0xD3 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, sz)?;
                let count = match op {
                    0xC0 | 0xC1 => self.fetch8(bus)? as u32,
                    0xD0 | 0xD1 => 1,
                    _ => self.reg(8, ECX),
                };
                let r = self.shift(m.reg, sz, a, count);
                self.write_rm(bus, &m, sz, r)?;
            }
            0xC2 | 0xC3 => {
                // RET near [imm16]
                let n = if op == 0xC2 { self.fetch16(bus)? } else { 0 };
                let ip = self.stack_peek(bus, 0, osz)?;
                self.jump(ip)?;
                let sp = (self.regs[ESP] as u16)
                    .wrapping_add((osz / 8) as u16)
                    .wrapping_add(n);
                self.set_reg(16, ESP, sp as u32);
                self.cycles += 4;
            }
            0xC4 | 0xC5 => {
                let seg = if op == 0xC4 { ES } else { DS };
                self.load_far_ptr(bus, seg)?;
            }
            0xC6 | 0xC7 => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                if m.reg != 0 {
                    return Err(UD);
                }
                let v = self.fetch_imm(bus, sz)?;
                self.write_rm(bus, &m, sz, v)?;
            }
            0xC8 => self.enter(bus)?,
            0xC9 => {
                // LEAVE
                let bp = self.regs[EBP] & 0xFFFF;
                let v = self.rd(bus, SS, bp, osz)?;
                self.set_reg(16, ESP, bp.wrapping_add(osz / 8));
                self.set_reg(osz, EBP, v);
            }
            0xCA | 0xCB => {
                // RETF [imm16]
                let n = if op == 0xCA { self.fetch16(bus)? } else { 0 };
                let ip = self.stack_peek(bus, 0, osz)?;
                let cs = self.stack_peek(bus, osz / 8, osz)? as u16;
                self.jump_far(cs, ip)?;
                let sp = (self.regs[ESP] as u16)
                    .wrapping_add((osz / 4) as u16)
                    .wrapping_add(n);
                self.set_reg(16, ESP, sp as u32);
                self.cycles += 12;
            }
            0xCC => self.soft_int(bus, 3)?,
            0xCD => {
                let v = self.fetch8(bus)?;
                self.soft_int(bus, v)?;
            }
            0xCE => {
                if self.flag(OF) {
                    self.soft_int(bus, 4)?;
                }
            }
            0xCF => {
                // IRET
                let ip = self.stack_peek(bus, 0, osz)?;
                let cs = self.stack_peek(bus, osz / 8, osz)? as u16;
                let fl = self.stack_peek(bus, osz / 4, osz)?;
                self.jump_far(cs, ip)?;
                let sp = (self.regs[ESP] as u16).wrapping_add((3 * osz / 8) as u16);
                self.set_reg(16, ESP, sp as u32);
                self.set_flags_image(osz, fl);
                self.cycles += 14;
            }

            0xD4 => {
                // AAM
                let base = self.fetch8(bus)? as u32;
                if base == 0 {
                    return Err(DE);
                }
                let al = self.reg(8, 0);
                self.set_reg(8, 4, al / base);
                self.set_reg(8, 0, al % base);
                self.logic_f(8, al % base);
            }
            0xD5 => {
                // AAD
                let base = self.fetch8(bus)? as u32;
                let al = self.reg(8, 0);
                let ah = self.reg(8, 4);
                let r = self.add_f(8, al, (ah * base) & 0xFF, 0);
                self.set_reg(16, EAX, r);
            }
            0xD6 => {
                // SALC (undocumented): AL = CF ? FF : 00
                let v = if self.flag(CF) { 0xFF } else { 0 };
                self.set_reg(8, 0, v);
            }
            0xD7 => {
                // XLAT
                let off = (self.regs[EBX].wrapping_add(self.reg(8, 0))) & self.amask();
                let v = self.rd(bus, self.dseg(), off, 8)?;
                self.set_reg(8, 0, v);
            }
            0xD8..=0xDF => {
                // FPU escape. The 486SX has no FPU: with EM or TS set this
                // traps to #NM so software can emulate it; otherwise it's ignored.
                let _m = self.modrm(bus)?;
                if self.cr0 & (CR0_EM | CR0_TS) != 0 {
                    return Err(NM);
                }
            }

            0xE0..=0xE3 => {
                // LOOPNZ / LOOPZ / LOOP / JCXZ
                let d = self.fetch_simm8(bus)?;
                let csz = if self.a32 { 32 } else { 16 };
                let take = if op == 0xE3 {
                    self.reg(csz, ECX) == 0
                } else {
                    let c = self.reg(csz, ECX).wrapping_sub(1) & mask(csz);
                    self.set_reg(csz, ECX, c);
                    c != 0
                        && match op {
                            0xE0 => !self.flag(ZF),
                            0xE1 => self.flag(ZF),
                            _ => true,
                        }
                };
                if take {
                    self.jump(self.eip.wrapping_add(d))?;
                }
            }
            0xE4..=0xE7 | 0xEC..=0xEF => {
                let sz = if op & 1 == 0 { 8 } else { osz };
                let port = if op < 0xE8 {
                    self.fetch8(bus)? as u16
                } else {
                    self.regs[EDX] as u16
                };
                self.cycles += 10;
                if op & 2 == 0 {
                    let v = self.io_in(bus, port, sz);
                    self.set_reg(sz, EAX, v);
                } else {
                    let v = self.reg(sz, EAX);
                    self.io_out(bus, port, sz, v);
                }
            }
            0xE8 => {
                let d = self.fetch_imm(bus, osz)?;
                let ret = self.eip;
                let target = ret.wrapping_add(d) & mask(osz);
                if target > self.seg_limit[CS] {
                    return Err(GP);
                }
                self.push(bus, osz, ret)?;
                self.eip = target;
                self.cycles += 2;
            }
            0xE9 => {
                let d = self.fetch_imm(bus, osz)?;
                self.jump(self.eip.wrapping_add(d))?;
            }
            0xEA => {
                let ip = self.fetch_imm(bus, osz)?;
                let cs = self.fetch16(bus)?;
                self.jump_far(cs, ip)?;
                self.cycles += 12;
            }
            0xEB => {
                let d = self.fetch_simm8(bus)?;
                self.jump(self.eip.wrapping_add(d))?;
            }

            0xF1 => self.soft_int(bus, 1)?, // ICEBP
            0xF4 => self.halted = true,
            0xF5 => self.eflags ^= CF,
            0xF6 | 0xF7 => self.group3(bus, op)?,
            0xF8 => self.set_flag(CF, false),
            0xF9 => self.set_flag(CF, true),
            0xFA => self.set_flag(IF, false),
            0xFB => {
                if !self.flag(IF) {
                    self.irq_inhibit = true;
                }
                self.set_flag(IF, true);
            }
            0xFC => self.set_flag(DF, false),
            0xFD => self.set_flag(DF, true),
            0xFE => {
                let m = self.modrm(bus)?;
                let a = self.read_rm(bus, &m, 8)?;
                let r = match m.reg {
                    0 => self.inc_f(8, a),
                    1 => self.dec_f(8, a),
                    _ => return Err(UD),
                };
                self.write_rm(bus, &m, 8, r)?;
            }
            0xFF => self.group5(bus)?,

            _ => return Err(UD),
        }
        Ok(())
    }

    /// Evaluate condition code `cc` (the low nibble of Jcc/SETcc).
    pub(crate) fn cond(&self, cc: u8) -> bool {
        let r = match cc >> 1 {
            0 => self.flag(OF),
            1 => self.flag(CF),
            2 => self.flag(ZF),
            3 => self.flag(CF) || self.flag(ZF),
            4 => self.flag(SF),
            5 => self.flag(PF),
            6 => self.flag(SF) != self.flag(OF),
            _ => self.flag(ZF) || (self.flag(SF) != self.flag(OF)),
        };
        r ^ (cc & 1 != 0)
    }

    fn soft_int<B: Bus>(&mut self, bus: &mut B, vector: u8) -> R<()> {
        let ret = self.eip;
        self.interrupt(bus, vector, ret)
    }

    pub(crate) fn io_in<B: Bus>(&mut self, bus: &mut B, port: u16, sz: u32) -> u32 {
        match sz {
            8 => bus.io_read8(port) as u32,
            16 => bus.io_read16(port) as u32,
            _ => bus.io_read32(port),
        }
    }

    pub(crate) fn io_out<B: Bus>(&mut self, bus: &mut B, port: u16, sz: u32, v: u32) {
        match sz {
            8 => bus.io_write8(port, v as u8),
            16 => bus.io_write16(port, v as u16),
            _ => bus.io_write32(port, v),
        }
    }

    /// LES / LDS / LSS / LFS / LGS: load reg and a segment from m16:16/32.
    pub(crate) fn load_far_ptr<B: Bus>(&mut self, bus: &mut B, seg: usize) -> R<()> {
        let osz = self.osz();
        let m = self.modrm(bus)?;
        if m.is_reg() {
            return Err(UD);
        }
        let off = self.rd(bus, m.seg, m.off, osz)?;
        let sel_off = self.off_plus(&m, osz / 8);
        let sel = self.rd(bus, m.seg, sel_off, 16)? as u16;
        self.load_seg_real(seg, sel);
        self.set_reg(osz, m.reg, off);
        Ok(())
    }

    fn enter<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let osz = self.osz();
        let size = self.fetch16(bus)? as u32;
        let level = (self.fetch8(bus)? & 31) as u32;
        self.cycles += 10 + 4 * level as u64;
        self.push(bus, osz, self.reg(osz, EBP))?;
        let frame = self.regs[ESP] & 0xFFFF;
        if level > 0 {
            let mut bp = self.regs[EBP] & 0xFFFF;
            for _ in 1..level {
                bp = bp.wrapping_sub(osz / 8) & 0xFFFF;
                let v = self.rd(bus, SS, bp, osz)?;
                self.push(bus, osz, v)?;
            }
            self.push(bus, osz, frame)?;
        }
        let new_sp = (self.regs[ESP] & 0xFFFF).wrapping_sub(size) & 0xFFFF;
        self.set_reg(osz, EBP, frame);
        self.set_reg(16, ESP, new_sp);
        Ok(())
    }

    fn group3<B: Bus>(&mut self, bus: &mut B, op: u8) -> R<()> {
        let sz = if op == 0xF6 { 8 } else { self.osz() };
        let m = self.modrm(bus)?;
        let a = self.read_rm(bus, &m, sz)?;
        match m.reg {
            0 | 1 => {
                let b = self.fetch_imm(bus, sz)?;
                self.logic_f(sz, a & b);
            }
            2 => self.write_rm(bus, &m, sz, !a & mask(sz))?,
            3 => {
                let r = self.sub_f(sz, 0, a, 0);
                self.write_rm(bus, &m, sz, r)?;
            }
            4 => self.mul_acc(false, sz, a),
            5 => self.mul_acc(true, sz, a),
            6 => self.div_acc(false, sz, a)?,
            _ => self.div_acc(true, sz, a)?,
        }
        Ok(())
    }

    fn group5<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let osz = self.osz();
        let m = self.modrm(bus)?;
        match m.reg {
            0 | 1 => {
                let a = self.read_rm(bus, &m, osz)?;
                let r = if m.reg == 0 { self.inc_f(osz, a) } else { self.dec_f(osz, a) };
                self.write_rm(bus, &m, osz, r)?;
            }
            2 => {
                // CALL near r/m
                let target = self.read_rm(bus, &m, osz)?;
                if target > self.seg_limit[CS] {
                    return Err(GP);
                }
                let ret = self.eip;
                self.push(bus, osz, ret)?;
                self.eip = target;
                self.cycles += 2;
            }
            3 | 5 => {
                // CALL far / JMP far m16:16/32
                if m.is_reg() {
                    return Err(UD);
                }
                let ip = self.rd(bus, m.seg, m.off, osz)?;
                let cs_off = self.off_plus(&m, osz / 8);
                let cs = self.rd(bus, m.seg, cs_off, 16)? as u16;
                if m.reg == 3 {
                    let (old_cs, old_ip) = (self.sregs[CS], self.eip);
                    self.push(bus, osz, old_cs as u32)?;
                    self.push(bus, osz, old_ip)?;
                }
                self.jump_far(cs, ip)?;
                self.cycles += 12;
            }
            4 => {
                let target = self.read_rm(bus, &m, osz)?;
                self.jump(target)?;
            }
            6 => {
                let v = self.read_rm(bus, &m, osz)?;
                self.push(bus, osz, v)?;
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    // ---------------------------------------------------------------- strings

    fn string<B: Bus>(&mut self, bus: &mut B, kind: Str, sz: u32) -> R<()> {
        let asz = if self.a32 { 32 } else { 16 };
        let step = sz / 8;
        let delta = if self.flag(DF) { step.wrapping_neg() } else { step };
        let src_seg = self.dseg();
        let mut done = 0;

        loop {
            if self.rep != Rep::None {
                if self.reg(asz, ECX) == 0 {
                    break;
                }
                if done == REP_BATCH {
                    // Restart the instruction later so interrupts can run.
                    self.eip = self.start_eip;
                    break;
                }
            }
            let si = self.reg(asz, ESI);
            let di = self.reg(asz, EDI);
            let mut cmp_eq = None;
            match kind {
                Str::Movs => {
                    let v = self.rd(bus, src_seg, si, sz)?;
                    self.wr(bus, ES, di, sz, v)?;
                }
                Str::Cmps => {
                    let a = self.rd(bus, src_seg, si, sz)?;
                    let b = self.rd(bus, ES, di, sz)?;
                    self.sub_f(sz, a, b, 0);
                    cmp_eq = Some(self.flag(ZF));
                }
                Str::Stos => {
                    let v = self.reg(sz, EAX);
                    self.wr(bus, ES, di, sz, v)?;
                }
                Str::Lods => {
                    let v = self.rd(bus, src_seg, si, sz)?;
                    self.set_reg(sz, EAX, v);
                }
                Str::Scas => {
                    let b = self.rd(bus, ES, di, sz)?;
                    self.sub_f(sz, self.reg(sz, EAX), b, 0);
                    cmp_eq = Some(self.flag(ZF));
                }
                Str::Ins => {
                    self.check(ES, di, step)?;
                    let v = self.io_in(bus, self.regs[EDX] as u16, sz);
                    self.wr(bus, ES, di, sz, v)?;
                }
                Str::Outs => {
                    let v = self.rd(bus, src_seg, si, sz)?;
                    self.io_out(bus, self.regs[EDX] as u16, sz, v);
                }
            }
            if matches!(kind, Str::Movs | Str::Cmps | Str::Lods | Str::Outs) {
                self.set_reg(asz, ESI, si.wrapping_add(delta));
            }
            if matches!(kind, Str::Movs | Str::Cmps | Str::Stos | Str::Scas | Str::Ins) {
                self.set_reg(asz, EDI, di.wrapping_add(delta));
            }
            self.cycles += 3;

            if self.rep == Rep::None {
                break;
            }
            let c = self.reg(asz, ECX).wrapping_sub(1);
            self.set_reg(asz, ECX, c);
            done += 1;
            if let Some(eq) = cmp_eq {
                if (self.rep == Rep::Repe && !eq) || (self.rep == Rep::Repne && eq) {
                    break;
                }
            }
        }
        Ok(())
    }

    // ---------------------------------------------------------------- BCD

    fn daa(&mut self) {
        let al = self.reg(8, 0);
        let cf = self.flag(CF);
        let mut r = al;
        if al & 0xF > 9 || self.flag(AF) {
            r = r.wrapping_add(6);
            self.set_flag(AF, true);
        } else {
            self.set_flag(AF, false);
        }
        if al > 0x99 || cf {
            r = r.wrapping_add(0x60);
            self.set_flag(CF, true);
        } else {
            self.set_flag(CF, false);
        }
        self.set_reg(8, 0, r);
        self.szp(8, r);
    }

    fn das(&mut self) {
        let al = self.reg(8, 0);
        let old_cf = self.flag(CF);
        let mut r = al;
        let mut cf = false;
        if al & 0xF > 9 || self.flag(AF) {
            r = r.wrapping_sub(6) & 0xFF;
            cf = old_cf || al < 6;
            self.set_flag(AF, true);
        } else {
            self.set_flag(AF, false);
        }
        if al > 0x99 || old_cf {
            r = r.wrapping_sub(0x60) & 0xFF;
            cf = true;
        }
        self.set_flag(CF, cf);
        self.set_reg(8, 0, r);
        self.szp(8, r);
    }

    fn aaa(&mut self) {
        let adjust = self.reg(8, 0) & 0xF > 9 || self.flag(AF);
        if adjust {
            let ax = self.reg(16, EAX).wrapping_add(0x106);
            self.set_reg(16, EAX, ax);
        }
        self.set_flag(AF, adjust);
        self.set_flag(CF, adjust);
        let al = self.reg(8, 0) & 0xF;
        self.set_reg(8, 0, al);
    }

    fn aas(&mut self) {
        let adjust = self.reg(8, 0) & 0xF > 9 || self.flag(AF);
        if adjust {
            let ax = self.reg(16, EAX).wrapping_sub(6);
            self.set_reg(16, EAX, ax);
            let ah = self.reg(8, 4).wrapping_sub(1);
            self.set_reg(8, 4, ah);
        }
        self.set_flag(AF, adjust);
        self.set_flag(CF, adjust);
        let al = self.reg(8, 0) & 0xF;
        self.set_reg(8, 0, al);
    }
}
