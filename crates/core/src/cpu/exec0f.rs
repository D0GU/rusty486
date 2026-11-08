//! Two-byte (0F xx) opcode execution: the 386 and 486 additions.

use super::alu::{mask, sext};
use super::flags::*;
use super::modrm::ModRm;
use super::*;

impl Cpu {
    pub(crate) fn exec_0f<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let osz = self.osz();
        let op = self.fetch8(bus)?;
        match op {
            0x00 => return Err(UD), // SLDT/STR/LLDT/LTR/VERR/VERW: protected mode only
            0x01 => self.group7(bus)?,
            0x06 => self.cr0 &= !CR0_TS, // CLTS
            0x08 | 0x09 => self.cycles += 4, // INVD / WBINVD: no cache to flush
            0x20 | 0x22 => {
                // MOV r32, CRn / MOV CRn, r32 (always register form)
                let m = self.modrm(bus)?;
                if op == 0x20 {
                    let v = match m.reg {
                        0 => self.cr0,
                        2 => self.cr2,
                        3 => self.cr3,
                        _ => return Err(UD),
                    };
                    self.regs[m.rm] = v;
                } else {
                    let v = self.regs[m.rm];
                    match m.reg {
                        0 => self.cr0 = v | 0x10, // ET is hardwired on the 486
                        2 => self.cr2 = v,
                        3 => self.cr3 = v,
                        _ => return Err(UD),
                    }
                }
            }
            0x21 | 0x23 => {
                // MOV r32, DRn / MOV DRn, r32
                let m = self.modrm(bus)?;
                let r = match m.reg {
                    4 => 6,
                    5 => 7,
                    r => r,
                };
                if op == 0x21 {
                    self.regs[m.rm] = self.dr[r];
                } else {
                    self.dr[r] = self.regs[m.rm];
                }
            }
            0x80..=0x8F => {
                let d = self.fetch_imm(bus, osz)?;
                if self.cond(op & 0xF) {
                    self.jump(self.eip.wrapping_add(d))?;
                }
            }
            0x90..=0x9F => {
                let m = self.modrm(bus)?;
                let v = self.cond(op & 0xF) as u32;
                self.write_rm(bus, &m, 8, v)?;
            }
            0xA0 | 0xA8 => {
                let v = self.sregs[if op == 0xA0 { FS } else { GS }] as u32;
                self.push(bus, osz, v)?;
            }
            0xA1 | 0xA9 => self.pop_sreg(bus, if op == 0xA1 { FS } else { GS })?,
            0xA2 if self.has_cpuid => self.cpuid(),
            0xA3 | 0xAB | 0xB3 | 0xBB => {
                let m = self.modrm(bus)?;
                let bit = self.reg(osz, m.reg);
                let kind = match op {
                    0xA3 => 0,
                    0xAB => 1,
                    0xB3 => 2,
                    _ => 3,
                };
                self.bit_op(bus, &m, kind, bit, true)?;
            }
            0xBA => {
                let m = self.modrm(bus)?;
                if m.reg < 4 {
                    return Err(UD);
                }
                let bit = self.fetch8(bus)? as u32;
                self.bit_op(bus, &m, m.reg - 4, bit, false)?;
            }
            0xA4 | 0xA5 | 0xAC | 0xAD => {
                let m = self.modrm(bus)?;
                let dst = self.read_rm(bus, &m, osz)?;
                let src = self.reg(osz, m.reg);
                let count = if op & 1 == 0 {
                    self.fetch8(bus)? as u32
                } else {
                    self.reg(8, ECX)
                };
                let r = self.double_shift(op < 0xAC, osz, dst, src, count);
                self.write_rm(bus, &m, osz, r)?;
            }
            0xAF => {
                let m = self.modrm(bus)?;
                let b = self.read_rm(bus, &m, osz)?;
                let r = self.imul_f(osz, self.reg(osz, m.reg), b);
                self.set_reg(osz, m.reg, r);
            }
            0xB0 | 0xB1 => {
                // CMPXCHG (486)
                let sz = if op == 0xB0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let dst = self.read_rm(bus, &m, sz)?;
                let acc = self.reg(sz, EAX);
                self.sub_f(sz, acc, dst, 0);
                if acc == dst {
                    let src = self.reg(sz, m.reg);
                    self.write_rm(bus, &m, sz, src)?;
                } else {
                    // The 486 always writes back (the old value) to memory.
                    self.write_rm(bus, &m, sz, dst)?;
                    self.set_reg(sz, EAX, dst);
                }
            }
            0xB2 => self.load_far_ptr(bus, SS)?,
            0xB4 => self.load_far_ptr(bus, FS)?,
            0xB5 => self.load_far_ptr(bus, GS)?,
            0xB6 | 0xB7 | 0xBE | 0xBF => {
                // MOVZX / MOVSX
                let ssz = if op & 1 == 0 { 8 } else { 16 };
                let m = self.modrm(bus)?;
                let v = self.read_rm(bus, &m, ssz)?;
                let v = if op >= 0xBE { sext(v, ssz) as u32 } else { v };
                self.set_reg(osz, m.reg, v & mask(osz));
            }
            0xBC | 0xBD => {
                // BSF / BSR
                let m = self.modrm(bus)?;
                let v = self.read_rm(bus, &m, osz)?;
                if v == 0 {
                    self.set_flag(ZF, true);
                } else {
                    self.set_flag(ZF, false);
                    let idx = if op == 0xBC {
                        v.trailing_zeros()
                    } else {
                        31 - v.leading_zeros()
                    };
                    self.cycles += idx as u64;
                    self.set_reg(osz, m.reg, idx);
                }
            }
            0xC0 | 0xC1 => {
                // XADD (486)
                let sz = if op == 0xC0 { 8 } else { osz };
                let m = self.modrm(bus)?;
                let dst = self.read_rm(bus, &m, sz)?;
                let src = self.reg(sz, m.reg);
                let sum = self.add_f(sz, dst, src, 0);
                self.set_reg(sz, m.reg, dst);
                self.write_rm(bus, &m, sz, sum)?;
            }
            0xC8..=0xCF => {
                // BSWAP (486)
                let r = (op & 7) as usize;
                self.regs[r] = self.regs[r].swap_bytes();
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    /// 0F 01: SGDT, SIDT, LGDT, LIDT, SMSW, LMSW, INVLPG.
    fn group7<B: Bus>(&mut self, bus: &mut B) -> R<()> {
        let m = self.modrm(bus)?;
        match m.reg {
            0..=3 => {
                if m.is_reg() {
                    return Err(UD);
                }
                let base_off = self.off_plus(&m, 2);
                match m.reg {
                    0 | 1 => {
                        let t = if m.reg == 0 { self.gdtr } else { self.idtr };
                        // A 16-bit operand stores only 24 bits of base on the 386/486.
                        let base = if self.op32 { t.base } else { t.base & 0x00FF_FFFF };
                        self.wr(bus, m.seg, m.off, 16, t.limit as u32)?;
                        self.wr(bus, m.seg, base_off, 32, base)?;
                    }
                    _ => {
                        let limit = self.rd(bus, m.seg, m.off, 16)? as u16;
                        let mut base = self.rd(bus, m.seg, base_off, 32)?;
                        if !self.op32 {
                            base &= 0x00FF_FFFF;
                        }
                        let t = DescTable { base, limit };
                        if m.reg == 2 {
                            self.gdtr = t;
                        } else {
                            self.idtr = t;
                        }
                    }
                }
            }
            4 => {
                // SMSW
                if m.is_reg() {
                    let v = self.cr0;
                    self.set_reg(self.osz(), m.rm, v);
                } else {
                    self.wr(bus, m.seg, m.off, 16, self.cr0 & 0xFFFF)?;
                }
            }
            6 => {
                // LMSW: loads PE/MP/EM/TS; PE can be set but not cleared.
                let v = self.read_rm(bus, &m, 16)?;
                self.cr0 = (self.cr0 & !0xE) | (v & 0xF) | (self.cr0 & CR0_PE);
            }
            7 => {
                // INVLPG (486): no TLB yet
                if m.is_reg() {
                    return Err(UD);
                }
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    /// BT (0) / BTS (1) / BTR (2) / BTC (3).
    /// With a register bit offset and a memory operand, the offset is signed
    /// and can address bits outside the operand.
    fn bit_op<B: Bus>(&mut self, bus: &mut B, m: &ModRm, kind: usize, bit: u32, reg_offset: bool) -> R<()> {
        let osz = self.osz();
        let (seg, off, bit) = if m.is_reg() || !reg_offset {
            (m.seg, m.off, bit & (osz - 1))
        } else {
            let disp = (sext(bit, osz) >> if osz == 32 { 5 } else { 4 }) * (osz as i64 / 8);
            let off = m.off.wrapping_add(disp as u32) & self.amask();
            (m.seg, off, bit & (osz - 1))
        };
        let v = if m.is_reg() {
            self.reg(osz, m.rm)
        } else {
            self.rd(bus, seg, off, osz)?
        };
        self.set_flag(CF, (v >> bit) & 1 != 0);
        let r = match kind {
            1 => v | 1 << bit,
            2 => v & !(1 << bit),
            3 => v ^ 1 << bit,
            _ => return Ok(()),
        };
        if m.is_reg() {
            self.set_reg(osz, m.rm, r);
        } else {
            self.wr(bus, seg, off, osz, r)?;
        }
        Ok(())
    }

    fn cpuid(&mut self) {
        match self.regs[EAX] {
            0 => {
                self.regs[EAX] = 1;
                // "GenuineIntel"
                self.regs[EBX] = u32::from_le_bytes(*b"Genu");
                self.regs[EDX] = u32::from_le_bytes(*b"ineI");
                self.regs[ECX] = u32::from_le_bytes(*b"ntel");
            }
            _ => {
                self.regs[EAX] = 0x0422; // family 4, model 2 (486SX), stepping 2
                self.regs[EBX] = 0;
                self.regs[ECX] = 0;
                self.regs[EDX] = 0; // no FPU (SX)
            }
        }
    }
}
