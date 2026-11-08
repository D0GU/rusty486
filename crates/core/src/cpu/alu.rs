//! Arithmetic, logic and flag computation.

use super::{Cpu, Fault, R, DE, EAX, EDX};

pub mod flags {
    pub const CF: u32 = 1 << 0;
    pub const PF: u32 = 1 << 2;
    pub const AF: u32 = 1 << 4;
    pub const ZF: u32 = 1 << 6;
    pub const SF: u32 = 1 << 7;
    pub const TF: u32 = 1 << 8;
    pub const IF: u32 = 1 << 9;
    pub const DF: u32 = 1 << 10;
    pub const OF: u32 = 1 << 11;
    pub const AC: u32 = 1 << 18;
}
use flags::*;

pub(crate) fn mask(sz: u32) -> u32 {
    match sz {
        8 => 0xFF,
        16 => 0xFFFF,
        _ => 0xFFFF_FFFF,
    }
}

pub(crate) fn msb(sz: u32) -> u32 {
    1 << (sz - 1)
}

/// Sign-extend the low `sz` bits of `v` to an i64.
pub(crate) fn sext(v: u32, sz: u32) -> i64 {
    match sz {
        8 => v as u8 as i8 as i64,
        16 => v as u16 as i16 as i64,
        _ => v as i32 as i64,
    }
}

impl Cpu {
    pub(crate) fn flag(&self, f: u32) -> bool {
        self.eflags & f != 0
    }

    pub(crate) fn set_flag(&mut self, f: u32, on: bool) {
        if on {
            self.eflags |= f;
        } else {
            self.eflags &= !f;
        }
    }

    /// Set SF, ZF and PF from a result.
    pub(crate) fn szp(&mut self, sz: u32, r: u32) {
        let r = r & mask(sz);
        self.set_flag(ZF, r == 0);
        self.set_flag(SF, r & msb(sz) != 0);
        self.set_flag(PF, (r as u8).count_ones() % 2 == 0);
    }

    pub(crate) fn add_f(&mut self, sz: u32, a: u32, b: u32, c: u32) -> u32 {
        let m = mask(sz);
        let wide = a as u64 + b as u64 + c as u64;
        let r = wide as u32 & m;
        self.set_flag(CF, wide > m as u64);
        self.set_flag(OF, (a ^ r) & (b ^ r) & msb(sz) != 0);
        self.set_flag(AF, (a ^ b ^ r) & 0x10 != 0);
        self.szp(sz, r);
        r
    }

    pub(crate) fn sub_f(&mut self, sz: u32, a: u32, b: u32, c: u32) -> u32 {
        let r = a.wrapping_sub(b).wrapping_sub(c) & mask(sz);
        self.set_flag(CF, (b as u64 + c as u64) > a as u64);
        self.set_flag(OF, (a ^ b) & (a ^ r) & msb(sz) != 0);
        self.set_flag(AF, (a ^ b ^ r) & 0x10 != 0);
        self.szp(sz, r);
        r
    }

    pub(crate) fn logic_f(&mut self, sz: u32, r: u32) -> u32 {
        let r = r & mask(sz);
        self.eflags &= !(CF | OF | AF);
        self.szp(sz, r);
        r
    }

    /// The eight classic ALU operations, by their opcode index
    /// (ADD OR ADC SBB AND SUB XOR CMP).
    pub(crate) fn alu(&mut self, op: u32, sz: u32, a: u32, b: u32) -> u32 {
        let cf = self.flag(CF) as u32;
        match op & 7 {
            0 => self.add_f(sz, a, b, 0),
            1 => self.logic_f(sz, a | b),
            2 => self.add_f(sz, a, b, cf),
            3 => self.sub_f(sz, a, b, cf),
            4 => self.logic_f(sz, a & b),
            5 | 7 => self.sub_f(sz, a, b, 0),
            _ => self.logic_f(sz, a ^ b),
        }
    }

    pub(crate) fn inc_f(&mut self, sz: u32, a: u32) -> u32 {
        let cf = self.flag(CF);
        let r = self.add_f(sz, a, 1, 0);
        self.set_flag(CF, cf);
        r
    }

    pub(crate) fn dec_f(&mut self, sz: u32, a: u32) -> u32 {
        let cf = self.flag(CF);
        let r = self.sub_f(sz, a, 1, 0);
        self.set_flag(CF, cf);
        r
    }

    /// Rotates and shifts (group 2), selected by `op` = ModRM reg field.
    pub(crate) fn shift(&mut self, op: usize, sz: u32, a: u32, count: u32) -> u32 {
        let count = count & 0x1F;
        if count == 0 {
            return a;
        }
        self.cycles += 2;
        let m = mask(sz);
        let hb = msb(sz);
        match op {
            0 => {
                // ROL
                let c = count % sz;
                let r = if c == 0 { a } else { ((a << c) | (a >> (sz - c))) & m };
                let cf = r & 1 != 0;
                self.set_flag(CF, cf);
                self.set_flag(OF, (r & hb != 0) ^ cf);
                r
            }
            1 => {
                // ROR
                let c = count % sz;
                let r = if c == 0 { a } else { ((a >> c) | (a << (sz - c))) & m };
                self.set_flag(CF, r & hb != 0);
                self.set_flag(OF, ((r ^ (r << 1)) & hb) != 0);
                r
            }
            2 => {
                // RCL
                let c = if sz == 32 { count } else { count % (sz + 1) };
                let mut r = a;
                let mut cf = self.flag(CF) as u32;
                for _ in 0..c {
                    let out = (r & hb != 0) as u32;
                    r = ((r << 1) | cf) & m;
                    cf = out;
                }
                self.set_flag(CF, cf != 0);
                self.set_flag(OF, (r & hb != 0) ^ (cf != 0));
                r
            }
            3 => {
                // RCR
                let c = if sz == 32 { count } else { count % (sz + 1) };
                let mut r = a;
                let mut cf = self.flag(CF) as u32;
                for _ in 0..c {
                    let out = r & 1;
                    r = (r >> 1) | (cf << (sz - 1));
                    cf = out;
                }
                self.set_flag(CF, cf != 0);
                self.set_flag(OF, ((r ^ (r << 1)) & hb) != 0);
                r
            }
            4 | 6 => {
                // SHL / SAL
                let wide = (a as u64) << count;
                let r = wide as u32 & m;
                let cf = (wide >> sz) & 1 != 0;
                self.set_flag(CF, cf);
                self.set_flag(OF, (r & hb != 0) ^ cf);
                self.set_flag(AF, false);
                self.szp(sz, r);
                r
            }
            5 => {
                // SHR
                let r = ((a as u64) >> count) as u32;
                self.set_flag(CF, ((a as u64) >> (count - 1)) & 1 != 0);
                self.set_flag(OF, a & hb != 0);
                self.set_flag(AF, false);
                self.szp(sz, r);
                r
            }
            _ => {
                // SAR
                let sa = sext(a, sz);
                let r = (sa >> count) as u32 & m;
                self.set_flag(CF, (sa >> (count - 1)) & 1 != 0);
                self.set_flag(OF, false);
                self.set_flag(AF, false);
                self.szp(sz, r);
                r
            }
        }
    }

    /// SHLD (left = true) / SHRD: shift `dst`, filling from `src`.
    pub(crate) fn double_shift(&mut self, left: bool, sz: u32, dst: u32, src: u32, count: u32) -> u32 {
        let count = count & 0x1F;
        if count == 0 {
            return dst;
        }
        let m = mask(sz);
        let hb = msb(sz);
        let (r, cf) = if left {
            // Concatenate dst:src (and, for 16-bit, dst again) and shift left.
            let wide: u64 = if sz == 16 {
                (dst as u64) << 48 | (src as u64) << 32 | (dst as u64) << 16
            } else {
                (dst as u64) << 32 | src as u64
            };
            let shifted = wide << count;
            let r = (shifted >> (64 - sz)) as u32 & m;
            let cf = (wide >> (64 - count)) & 1 != 0;
            (r, cf)
        } else {
            let wide: u64 = if sz == 16 {
                (dst as u64) | (src as u64) << 16 | (dst as u64) << 32
            } else {
                (src as u64) << 32 | dst as u64
            };
            let r = (wide >> count) as u32 & m;
            let cf = (wide >> (count - 1)) & 1 != 0;
            (r, cf)
        };
        self.set_flag(CF, cf);
        self.set_flag(OF, (r ^ dst) & hb != 0);
        self.set_flag(AF, false);
        self.szp(sz, r);
        r
    }

    /// MUL / IMUL with the accumulator (F6/F7 /4, /5).
    pub(crate) fn mul_acc(&mut self, signed: bool, sz: u32, src: u32) {
        self.cycles += 12;
        let a = self.reg(sz, EAX);
        let overflow;
        match sz {
            8 => {
                let r = if signed {
                    (sext(a, 8) * sext(src, 8)) as u32 & 0xFFFF
                } else {
                    a * src
                };
                self.set_reg(16, EAX, r);
                overflow = if signed { sext(r, 16) != sext(r, 8) } else { r > 0xFF };
                self.szp(8, r);
            }
            16 => {
                let r = if signed {
                    (sext(a, 16) * sext(src, 16)) as u32
                } else {
                    a * src
                };
                self.set_reg(16, EAX, r);
                self.set_reg(16, EDX, r >> 16);
                overflow = if signed { sext(r, 32) != sext(r, 16) } else { r > 0xFFFF };
                self.szp(16, r);
            }
            _ => {
                let r: u64 = if signed {
                    (sext(a, 32) * sext(src, 32)) as u64
                } else {
                    a as u64 * src as u64
                };
                self.regs[EAX] = r as u32;
                self.regs[EDX] = (r >> 32) as u32;
                overflow = if signed {
                    r as i64 != r as u32 as i32 as i64
                } else {
                    r >> 32 != 0
                };
                self.szp(32, r as u32);
            }
        }
        self.set_flag(CF, overflow);
        self.set_flag(OF, overflow);
        self.set_flag(AF, false);
    }

    /// Two/three-operand IMUL: returns the truncated product, sets CF/OF.
    pub(crate) fn imul_f(&mut self, sz: u32, a: u32, b: u32) -> u32 {
        self.cycles += 12;
        let full = sext(a, sz) * sext(b, sz);
        let r = full as u32 & mask(sz);
        let overflow = sext(r, sz) != full;
        self.set_flag(CF, overflow);
        self.set_flag(OF, overflow);
        self.set_flag(AF, false);
        self.szp(sz, r);
        r
    }

    /// DIV / IDIV with the accumulator (F6/F7 /6, /7).
    pub(crate) fn div_acc(&mut self, signed: bool, sz: u32, src: u32) -> R<()> {
        self.cycles += 24;
        if src & mask(sz) == 0 {
            return Err(DE);
        }
        match sz {
            8 => {
                let n = self.reg(16, EAX);
                let (q, r) = div_checked(signed, n as u64, src as u64, 16, 8)?;
                self.set_reg(8, 0, q); // AL
                self.set_reg(8, 4, r); // AH
            }
            16 => {
                let n = self.reg(16, EDX) << 16 | self.reg(16, EAX);
                let (q, r) = div_checked(signed, n as u64, src as u64, 32, 16)?;
                self.set_reg(16, EAX, q);
                self.set_reg(16, EDX, r);
            }
            _ => {
                let n = (self.regs[EDX] as u64) << 32 | self.regs[EAX] as u64;
                let (q, r) = div_checked(signed, n, src as u64, 64, 32)?;
                self.regs[EAX] = q;
                self.regs[EDX] = r;
            }
        }
        Ok(())
    }
}

/// Divide an `nbits` dividend by an `sz`-bit divisor, failing with #DE if the
/// quotient doesn't fit in `sz` bits.
fn div_checked(signed: bool, n: u64, d: u64, nbits: u32, sz: u32) -> R<(u32, u32)> {
    if signed {
        let n = sign_extend_wide(n, nbits);
        let d = sign_extend_wide(d, sz);
        let q = n / d;
        let r = n % d;
        let lo = -(1i128 << (sz - 1));
        let hi = (1i128 << (sz - 1)) - 1;
        if q < lo || q > hi {
            return Err(DE);
        }
        Ok((q as u32 & mask(sz), r as u32 & mask(sz)))
    } else {
        let q = n / d;
        if q > mask(sz) as u64 {
            return Err(Fault(0));
        }
        Ok((q as u32, (n % d) as u32))
    }
}

fn sign_extend_wide(v: u64, bits: u32) -> i128 {
    let shift = 128 - bits;
    ((v as i128) << shift) >> shift
}
