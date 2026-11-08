//! ModR/M + SIB decoding and effective-address calculation.

use super::{Cpu, R, DS, EBP, EBX, EDI, ESI, SS};
use crate::bus::Bus;

/// A decoded ModR/M operand.
#[derive(Clone, Copy, Debug)]
pub(crate) struct ModRm {
    pub md: u8,
    pub reg: usize,
    pub rm: usize,
    /// Segment for the memory operand (after overrides).
    pub seg: usize,
    /// Offset of the memory operand (already wrapped to the address size).
    pub off: u32,
}

impl ModRm {
    pub fn is_reg(&self) -> bool {
        self.md == 3
    }
}

impl Cpu {
    pub(crate) fn modrm<B: Bus>(&mut self, bus: &mut B) -> R<ModRm> {
        let b = self.fetch8(bus)?;
        let mut m = ModRm {
            md: b >> 6,
            reg: ((b >> 3) & 7) as usize,
            rm: (b & 7) as usize,
            seg: DS,
            off: 0,
        };
        if m.is_reg() {
            return Ok(m);
        }
        self.cycles += 1;

        let (off, seg) = if !self.a32 {
            let r = |i: usize| self.regs[i] & 0xFFFF;
            let (base, seg) = match m.rm {
                0 => (r(EBX) + r(ESI), DS),
                1 => (r(EBX) + r(EDI), DS),
                2 => (r(EBP) + r(ESI), SS),
                3 => (r(EBP) + r(EDI), SS),
                4 => (r(ESI), DS),
                5 => (r(EDI), DS),
                6 if m.md == 0 => (self.fetch16(bus)? as u32, DS),
                6 => (r(EBP), SS),
                _ => (r(EBX), DS),
            };
            let disp = match m.md {
                1 => self.fetch_simm8(bus)?,
                2 => self.fetch16(bus)? as u32,
                _ => 0,
            };
            (base.wrapping_add(disp) & 0xFFFF, seg)
        } else {
            let mut seg = DS;
            let base = if m.rm == 4 {
                let sib = self.fetch8(bus)?;
                let scale = sib >> 6;
                let index = ((sib >> 3) & 7) as usize;
                let b = (sib & 7) as usize;
                let mut off = if b == 5 && m.md == 0 {
                    self.fetch32(bus)?
                } else {
                    if b == 4 || b == 5 {
                        seg = SS;
                    }
                    // Undocumented: with no index (100b) but a non-zero scale,
                    // the scale is applied to the base register instead.
                    if index == 4 {
                        self.regs[b] << scale
                    } else {
                        self.regs[b]
                    }
                };
                if index != 4 {
                    off = off.wrapping_add(self.regs[index] << scale);
                }
                off
            } else if m.rm == 5 && m.md == 0 {
                self.fetch32(bus)?
            } else {
                if m.rm == 5 {
                    seg = SS;
                }
                self.regs[m.rm]
            };
            let disp = match m.md {
                1 => self.fetch_simm8(bus)?,
                2 => self.fetch32(bus)?,
                _ => 0,
            };
            (base.wrapping_add(disp), seg)
        };

        m.off = off;
        m.seg = self.seg_ovr.unwrap_or(seg);
        Ok(m)
    }

    pub(crate) fn read_rm<B: Bus>(&mut self, bus: &mut B, m: &ModRm, sz: u32) -> R<u32> {
        if m.is_reg() {
            Ok(self.reg(sz, m.rm))
        } else {
            self.rd(bus, m.seg, m.off, sz)
        }
    }

    pub(crate) fn write_rm<B: Bus>(&mut self, bus: &mut B, m: &ModRm, sz: u32, v: u32) -> R<()> {
        if m.is_reg() {
            self.set_reg(sz, m.rm, v);
            Ok(())
        } else {
            self.wr(bus, m.seg, m.off, sz, v)
        }
    }

    /// Offset `delta` bytes past a memory operand, wrapped to the address size.
    pub(crate) fn off_plus(&self, m: &ModRm, delta: u32) -> u32 {
        m.off.wrapping_add(delta) & self.amask()
    }
}
