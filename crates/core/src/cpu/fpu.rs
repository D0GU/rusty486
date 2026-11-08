//! x87 floating-point unit for the fantasy R1024 CPU model.
//!
//! The 486SX has no FPU; the R1024 does. Registers hold `f64` values rather
//! than 80-bit extended precision (a fantasy-CPU simplification: 80-bit
//! loads and stores convert, and results are rounded to double precision).
//! Exceptions only set status flags; unmasked exceptions are not raised.

use super::modrm::ModRm;
use super::*;

// Status word bits.
const IE: u16 = 1 << 0;
const ZE: u16 = 1 << 2;
const SF: u16 = 1 << 6;
const C0: u16 = 1 << 8;
const C1: u16 = 1 << 9;
const C2: u16 = 1 << 10;
const C3: u16 = 1 << 14;
const CC: u16 = C0 | C1 | C2 | C3;

/// The FPU register file and control state.
#[derive(Clone, Debug)]
pub struct Fpu {
    /// Physical registers R0-R7 (ST(i) is R[(top + i) & 7]).
    pub r: [f64; 8],
    /// Physical register in use (tag != empty).
    pub valid: [bool; 8],
    pub top: usize,
    /// Control word.
    pub cw: u16,
    /// Status word (TOP is kept separately in `top`).
    pub sw: u16,
}

impl Default for Fpu {
    fn default() -> Self {
        Fpu { r: [0.0; 8], valid: [false; 8], top: 0, cw: 0x037F, sw: 0 }
    }
}

impl Fpu {
    pub fn init(&mut self) {
        *self = Fpu::default();
    }

    pub fn status(&self) -> u16 {
        (self.sw & !0x3800) | ((self.top as u16) << 11)
    }

    fn phys(&self, i: usize) -> usize {
        (self.top + i) & 7
    }

    /// ST(i); an empty register reads as NaN (stack underflow).
    pub fn st(&mut self, i: usize) -> f64 {
        let p = self.phys(i);
        if !self.valid[p] {
            self.sw |= IE | SF;
            self.sw &= !C1;
            return f64::NAN;
        }
        self.r[p]
    }

    fn set_st(&mut self, i: usize, v: f64) {
        let p = self.phys(i);
        self.r[p] = v;
        self.valid[p] = true;
    }

    fn push(&mut self, v: f64) {
        self.top = (self.top + 7) & 7;
        if self.valid[self.top] {
            self.sw |= IE | SF | C1; // stack overflow
        }
        self.r[self.top] = v;
        self.valid[self.top] = true;
    }

    fn pop(&mut self) {
        self.valid[self.top] = false;
        self.top = (self.top + 1) & 7;
    }

    /// Two-bit tag of a physical register: valid, zero, special or empty.
    fn tag(&self, p: usize) -> u16 {
        if !self.valid[p] {
            3
        } else if self.r[p] == 0.0 {
            1
        } else if self.r[p].is_finite() && self.r[p].is_normal() {
            0
        } else {
            2
        }
    }

    pub fn tag_word(&self) -> u16 {
        (0..8).fold(0, |tw, p| tw | self.tag(p) << (2 * p))
    }

    fn set_tag_word(&mut self, tw: u16) {
        for p in 0..8 {
            self.valid[p] = (tw >> (2 * p)) & 3 != 3;
        }
    }

    /// Round to an integer using the control word's rounding mode.
    fn round(&self, v: f64) -> f64 {
        match (self.cw >> 10) & 3 {
            0 => {
                let r = v.round();
                // Round half to even.
                if (v - v.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
                    r - v.signum()
                } else {
                    r
                }
            }
            1 => v.floor(),
            2 => v.ceil(),
            _ => v.trunc(),
        }
    }

    /// Compare a with b and set C0/C2/C3.
    fn compare(&mut self, a: f64, b: f64, unordered_ok: bool) {
        self.sw &= !CC;
        if a.is_nan() || b.is_nan() {
            if !unordered_ok || a.is_nan() && !is_quiet(a) || b.is_nan() && !is_quiet(b) {
                self.sw |= IE;
            }
            self.sw |= C0 | C2 | C3;
        } else if a < b {
            self.sw |= C0;
        } else if a == b {
            self.sw |= C3;
        }
    }

    /// The eight basic operations, by ModR/M reg field: add, mul, com, comp,
    /// sub, subr, div, divr. Returns None for the comparisons.
    fn arith(&mut self, kind: usize, a: f64, b: f64) -> Option<f64> {
        Some(match kind {
            0 => a + b,
            1 => a * b,
            2 | 3 => {
                self.compare(a, b, false);
                return None;
            }
            4 => a - b,
            5 => b - a,
            6 | 7 => {
                let (n, d) = if kind == 6 { (a, b) } else { (b, a) };
                if d == 0.0 && n.is_finite() && n != 0.0 {
                    self.sw |= ZE;
                }
                n / d
            }
            _ => unreachable!(),
        })
    }
}

fn is_quiet(v: f64) -> bool {
    v.to_bits() & (1 << 51) != 0
}

/// Convert an 80-bit extended real to f64 (rounding to nearest).
pub fn ext_to_f64(mant: u64, se: u16) -> f64 {
    let sign = if se & 0x8000 != 0 { -1.0 } else { 1.0 };
    let exp = (se & 0x7FFF) as i32;
    if exp == 0x7FFF {
        return if mant << 1 == 0 { sign * f64::INFINITY } else { f64::NAN };
    }
    if mant == 0 {
        return sign * 0.0;
    }
    // value = mant * 2^(exp - 16383 - 63)
    let e = exp - 16383 - 63;
    sign * scale(mant as f64, e)
}

/// x * 2^e without intermediate overflow or underflow.
fn scale(mut x: f64, mut e: i32) -> f64 {
    while e > 1000 {
        x *= 2f64.powi(1000);
        e -= 1000;
        if x.is_infinite() {
            return x;
        }
    }
    while e < -1000 {
        x *= 2f64.powi(-1000);
        e += 1000;
        if x == 0.0 {
            return x;
        }
    }
    x * 2f64.powi(e)
}

/// Convert an f64 to an 80-bit extended real: (mantissa, sign+exponent).
pub fn f64_to_ext(v: f64) -> (u64, u16) {
    let bits = v.to_bits();
    let sign = ((bits >> 63) as u16) << 15;
    let exp = ((bits >> 52) & 0x7FF) as i32;
    let frac = bits & ((1 << 52) - 1);
    if exp == 0x7FF {
        // Infinity or NaN (keep the quiet bit and payload).
        return (1 << 63 | frac << 11, sign | 0x7FFF);
    }
    if exp == 0 {
        if frac == 0 {
            return (0, sign);
        }
        // Denormal double: normalise (always representable as extended).
        let shift = frac.leading_zeros() as i32; // frac < 2^52, so shift >= 12
        let m = frac << shift;
        let e = 16383 - 1022 - (shift - 11);
        return (m, sign | e as u16);
    }
    (1 << 63 | frac << 11, sign | (exp - 1023 + 16383) as u16)
}

impl Cpu {
    /// Read `n` bytes (up to 10) at a memory operand.
    fn fpu_read_bytes<B: Bus>(&mut self, bus: &mut B, m: &ModRm, n: u32) -> R<[u8; 10]> {
        self.check(m.seg, m.off, n)?;
        let mut out = [0u8; 10];
        for k in 0..n {
            let off = self.off_plus(m, k);
            out[k as usize] = self.rd(bus, m.seg, off, 8)? as u8;
        }
        Ok(out)
    }

    fn fpu_write_bytes<B: Bus>(&mut self, bus: &mut B, m: &ModRm, bytes: &[u8]) -> R<()> {
        self.probe_write(bus, m.seg, m.off, bytes.len() as u32)?;
        for (k, &b) in bytes.iter().enumerate() {
            let off = self.off_plus(m, k as u32);
            self.wr(bus, m.seg, off, 8, b as u32)?;
        }
        Ok(())
    }

    fn fpu_read_u64<B: Bus>(&mut self, bus: &mut B, m: &ModRm) -> R<u64> {
        let b = self.fpu_read_bytes(bus, m, 8)?;
        Ok(u64::from_le_bytes(b[..8].try_into().unwrap()))
    }

    /// Store an integer with the current rounding mode; out-of-range values
    /// store the "integer indefinite" (most negative) value.
    fn fpu_to_int(&mut self, v: f64, bits: u32) -> i64 {
        let r = self.fpu.round(v);
        let max = (1i128 << (bits - 1)) as f64;
        if r.is_nan() || r >= max || r < -max {
            self.fpu.sw |= IE;
            return i64::MIN >> (64 - bits);
        }
        r as i64
    }

    /// The x87 environment (FSTENV / FSAVE header): 14 or 28 bytes.
    fn fpu_env_bytes(&self) -> Vec<u8> {
        let (cw, sw, tw) = (self.fpu.cw, self.fpu.status(), self.fpu.tag_word());
        if self.op32 {
            let mut v = vec![0u8; 28];
            v[0..2].copy_from_slice(&cw.to_le_bytes());
            v[4..6].copy_from_slice(&sw.to_le_bytes());
            v[8..10].copy_from_slice(&tw.to_le_bytes());
            v
        } else {
            let mut v = vec![0u8; 14];
            v[0..2].copy_from_slice(&cw.to_le_bytes());
            v[2..4].copy_from_slice(&sw.to_le_bytes());
            v[4..6].copy_from_slice(&tw.to_le_bytes());
            v
        }
    }

    /// FLDENV / FRSTOR header. Returns the environment size.
    fn fpu_load_env<B: Bus>(&mut self, bus: &mut B, m: &ModRm) -> R<u32> {
        let n = if self.op32 { 28 } else { 14 };
        let (cw, sw, tw) = if self.op32 {
            let b = self.fpu_read_bytes(bus, m, 10)?;
            (
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[4], b[5]]),
                u16::from_le_bytes([b[8], b[9]]),
            )
        } else {
            let b = self.fpu_read_bytes(bus, m, 6)?;
            (
                u16::from_le_bytes([b[0], b[1]]),
                u16::from_le_bytes([b[2], b[3]]),
                u16::from_le_bytes([b[4], b[5]]),
            )
        };
        self.fpu.cw = cw;
        self.fpu.sw = sw & !0x3800;
        self.fpu.top = ((sw >> 11) & 7) as usize;
        self.fpu.set_tag_word(tw);
        Ok(n)
    }

    /// Execute an x87 instruction (opcode D8-DF; the ModR/M byte is next).
    pub(crate) fn fpu_exec<B: Bus>(&mut self, bus: &mut B, op: u8) -> R<()> {
        let m = self.modrm(bus)?;
        if self.cr0 & (CR0_EM | CR0_TS) != 0 {
            return Err(NM);
        }
        self.cycles += 3;
        if m.is_reg() {
            return self.fpu_reg(bus, op, m.reg, m.rm);
        }
        match (op, m.reg) {
            // Arithmetic with a memory operand: m32real, m32int, m64real, m16int.
            (0xD8 | 0xDA | 0xDC | 0xDE, k) => {
                let src = match op {
                    0xD8 => f32::from_bits(self.rd(bus, m.seg, m.off, 32)?) as f64,
                    0xDA => self.rd(bus, m.seg, m.off, 32)? as i32 as f64,
                    0xDC => f64::from_bits(self.fpu_read_u64(bus, &m)?),
                    _ => self.rd(bus, m.seg, m.off, 16)? as u16 as i16 as f64,
                };
                let a = self.fpu.st(0);
                if let Some(v) = self.fpu.arith(k, a, src) {
                    self.fpu.set_st(0, v);
                } else if k == 3 {
                    self.fpu.pop();
                }
            }
            (0xD9, 0) => {
                let v = f32::from_bits(self.rd(bus, m.seg, m.off, 32)?) as f64;
                self.fpu.push(v);
            }
            (0xD9, 2 | 3) => {
                let v = self.fpu.st(0) as f32;
                self.wr(bus, m.seg, m.off, 32, v.to_bits())?;
                if m.reg == 3 {
                    self.fpu.pop();
                }
            }
            (0xD9, 4) => {
                self.fpu_load_env(bus, &m)?;
            }
            (0xD9, 5) => self.fpu.cw = self.rd(bus, m.seg, m.off, 16)? as u16,
            (0xD9, 6) => {
                let env = self.fpu_env_bytes();
                self.fpu_write_bytes(bus, &m, &env)?;
                self.fpu.cw |= 0x3F; // FSTENV masks all exceptions
            }
            (0xD9, 7) => self.wr(bus, m.seg, m.off, 16, self.fpu.cw as u32)?,
            (0xDB, 0) => {
                let v = self.rd(bus, m.seg, m.off, 32)? as i32 as f64;
                self.fpu.push(v);
            }
            (0xDB, 2 | 3) => {
                let v = self.fpu.st(0);
                let i = self.fpu_to_int(v, 32);
                self.wr(bus, m.seg, m.off, 32, i as u32)?;
                if m.reg == 3 {
                    self.fpu.pop();
                }
            }
            (0xDB, 5) => {
                let b = self.fpu_read_bytes(bus, &m, 10)?;
                let mant = u64::from_le_bytes(b[..8].try_into().unwrap());
                let v = ext_to_f64(mant, u16::from_le_bytes([b[8], b[9]]));
                self.fpu.push(v);
            }
            (0xDB, 7) => {
                let (mant, se) = f64_to_ext(self.fpu.st(0));
                let mut b = [0u8; 10];
                b[..8].copy_from_slice(&mant.to_le_bytes());
                b[8..].copy_from_slice(&se.to_le_bytes());
                self.fpu_write_bytes(bus, &m, &b)?;
                self.fpu.pop();
            }
            (0xDD, 0) => {
                let v = f64::from_bits(self.fpu_read_u64(bus, &m)?);
                self.fpu.push(v);
            }
            (0xDD, 2 | 3) => {
                let v = self.fpu.st(0);
                self.fpu_write_bytes(bus, &m, &v.to_bits().to_le_bytes())?;
                if m.reg == 3 {
                    self.fpu.pop();
                }
            }
            (0xDD, 4) => {
                // FRSTOR
                let n = self.fpu_load_env(bus, &m)?;
                for i in 0..8 {
                    let rm = ModRm { off: self.off_plus(&m, n + 10 * i), ..m };
                    let b = self.fpu_read_bytes(bus, &rm, 10)?;
                    let mant = u64::from_le_bytes(b[..8].try_into().unwrap());
                    let p = self.fpu.phys(i as usize);
                    self.fpu.r[p] = ext_to_f64(mant, u16::from_le_bytes([b[8], b[9]]));
                }
            }
            (0xDD, 6) => {
                // FSAVE: environment, ST(0)-ST(7), then FINIT.
                let mut bytes = self.fpu_env_bytes();
                for i in 0..8 {
                    let p = self.fpu.phys(i);
                    let (mant, se) = f64_to_ext(self.fpu.r[p]);
                    bytes.extend_from_slice(&mant.to_le_bytes());
                    bytes.extend_from_slice(&se.to_le_bytes());
                }
                self.fpu_write_bytes(bus, &m, &bytes)?;
                self.fpu.init();
            }
            (0xDD, 7) => self.wr(bus, m.seg, m.off, 16, self.fpu.status() as u32)?,
            (0xDF, 0) => {
                let v = self.rd(bus, m.seg, m.off, 16)? as u16 as i16 as f64;
                self.fpu.push(v);
            }
            (0xDF, 2 | 3) => {
                let v = self.fpu.st(0);
                let i = self.fpu_to_int(v, 16);
                self.wr(bus, m.seg, m.off, 16, i as u32 & 0xFFFF)?;
                if m.reg == 3 {
                    self.fpu.pop();
                }
            }
            (0xDF, 4) => {
                // FBLD: 18 packed BCD digits and a sign byte.
                let b = self.fpu_read_bytes(bus, &m, 10)?;
                let mut v = 0f64;
                for k in (0..9).rev() {
                    v = v * 100.0 + ((b[k] >> 4) * 10 + (b[k] & 15)) as f64;
                }
                self.fpu.push(if b[9] & 0x80 != 0 { -v } else { v });
            }
            (0xDF, 5) => {
                let v = self.fpu_read_u64(bus, &m)? as i64 as f64;
                self.fpu.push(v);
            }
            (0xDF, 6) => {
                // FBSTP
                let v = self.fpu.st(0);
                let r = self.fpu.round(v);
                let mut b = [0u8; 10];
                if r.is_nan() || r.abs() >= 1e18 {
                    self.fpu.sw |= IE;
                    b[7] = 0xC0;
                    b[8] = 0xFF;
                    b[9] = 0xFF;
                } else {
                    let mut n = r.abs() as u64;
                    for byte in b.iter_mut().take(9) {
                        let lo = n % 10;
                        n /= 10;
                        let hi = n % 10;
                        n /= 10;
                        *byte = (hi << 4 | lo) as u8;
                    }
                    if r < 0.0 {
                        b[9] = 0x80;
                    }
                }
                self.fpu_write_bytes(bus, &m, &b)?;
                self.fpu.pop();
            }
            (0xDF, 7) => {
                let v = self.fpu.st(0);
                let i = self.fpu_to_int(v, 64);
                self.fpu_write_bytes(bus, &m, &i.to_le_bytes())?;
                self.fpu.pop();
            }
            _ => return Err(UD),
        }
        Ok(())
    }

    /// Register forms (ModR/M mod = 3).
    fn fpu_reg<B: Bus>(&mut self, _bus: &mut B, op: u8, reg: usize, i: usize) -> R<()> {
        let x = &mut self.fpu;
        match (op, reg) {
            (0xD8, k) => {
                let (a, b) = (x.st(0), x.st(i));
                if let Some(v) = x.arith(k, a, b) {
                    x.set_st(0, v);
                } else if k == 3 {
                    x.pop();
                }
            }
            (0xDC | 0xDE, k) => {
                // ST(i) = ST(i) op ST(0); Intel swaps the sub/subr and
                // div/divr encodings in this direction.
                let k = match k {
                    4 => 5,
                    5 => 4,
                    6 => 7,
                    7 => 6,
                    k => k,
                };
                if op == 0xDE && k == 3 {
                    // FCOMPP (DE D9) / FCOMP alias
                    let (a, b) = (x.st(0), x.st(1));
                    x.compare(a, b, false);
                    x.pop();
                    x.pop();
                    return Ok(());
                }
                let (a, b) = (x.st(i), x.st(0));
                match x.arith(k, a, b) {
                    Some(v) => x.set_st(i, v),
                    None => {
                        let (a, b) = (x.st(0), x.st(i));
                        x.compare(a, b, false);
                        if k == 3 {
                            x.pop();
                        }
                    }
                }
                if op == 0xDE && k != 2 {
                    x.pop();
                }
            }
            (0xD9, 0) => {
                let v = x.st(i);
                x.push(v);
            }
            (0xD9, 1) => {
                // FXCH
                let (a, b) = (x.st(0), x.st(i));
                x.set_st(0, b);
                x.set_st(i, a);
                x.sw &= !C1;
            }
            (0xD9, 2) if i == 0 => {} // FNOP
            (0xD9, 3) | (0xDD, 3) | (0xDF, 3) => {
                // FSTP ST(i) (D9 D8+i and DF D8+i are undocumented aliases)
                let v = x.st(0);
                x.set_st(i, v);
                x.pop();
            }
            (0xDD, 2) => {
                let v = x.st(0);
                x.set_st(i, v);
            }
            (0xDD, 0) | (0xDF, 0) => {
                // FFREE (DF C0+i: FFREEP)
                let p = x.phys(i);
                x.valid[p] = false;
                if op == 0xDF {
                    x.pop();
                }
            }
            (0xDD, 4 | 5) => {
                let (a, b) = (x.st(0), x.st(i));
                x.compare(a, b, true);
                if reg == 5 {
                    x.pop();
                }
            }
            (0xDA, 5) if i == 1 => {
                // FUCOMPP
                let (a, b) = (x.st(0), x.st(1));
                x.compare(a, b, true);
                x.pop();
                x.pop();
            }
            (0xDB, 4) => match i {
                0 | 1 | 4 => {} // FENI, FDISI, FSETPM: no-ops on a 486
                2 => x.sw &= !0x80FF, // FCLEX
                3 => x.init(),
                _ => return Err(UD),
            },
            (0xDF, 4) if i == 0 => {
                let sw = x.status();
                self.set_reg(16, EAX, sw as u32);
            }
            (0xD9, 4..=7) => return self.fpu_d9(reg << 3 | i),
            _ => return Err(UD),
        }
        Ok(())
    }

    /// D9 E0-FF: constants, transcendental and miscellaneous operations.
    fn fpu_d9(&mut self, sub: usize) -> R<()> {
        let x = &mut self.fpu;
        use std::f64::consts::*;
        match sub {
            0x20 => {
                let v = x.st(0);
                x.set_st(0, -v);
            }
            0x21 => {
                let v = x.st(0);
                x.set_st(0, v.abs());
            }
            0x24 => {
                let v = x.st(0);
                x.compare(v, 0.0, false);
            }
            0x25 => {
                // FXAM
                let p = x.phys(0);
                let v = x.r[p];
                x.sw &= !CC;
                if v.is_sign_negative() {
                    x.sw |= C1;
                }
                x.sw |= if !x.valid[p] {
                    C3 | C0
                } else if v.is_nan() {
                    C0
                } else if v.is_infinite() {
                    C2 | C0
                } else if v == 0.0 {
                    C3
                } else if !v.is_normal() {
                    C3 | C2
                } else {
                    C2
                };
            }
            0x28..=0x2E => {
                let c = [1.0, LOG2_10, LOG2_E, PI, LOG10_2, LN_2, 0.0][sub - 0x28];
                x.push(c);
            }
            0x30 => {
                let v = x.st(0);
                x.set_st(0, v.exp2() - 1.0);
            }
            0x31 | 0x39 => {
                // FYL2X / FYL2XP1: ST(1) = ST(1) * log2(ST(0) [+ 1]), pop.
                let (a, b) = (x.st(0), x.st(1));
                let l = if sub == 0x31 { a.log2() } else { a.ln_1p() / LN_2 };
                if sub == 0x31 && a == 0.0 {
                    x.sw |= ZE;
                }
                x.set_st(1, b * l);
                x.pop();
            }
            0x32 => {
                // FPTAN
                let v = x.st(0);
                x.sw &= !C2;
                x.set_st(0, v.tan());
                x.push(1.0);
            }
            0x33 => {
                let (a, b) = (x.st(0), x.st(1));
                x.set_st(1, b.atan2(a));
                x.pop();
            }
            0x34 => {
                // FXTRACT: ST(0) = exponent, push significand.
                let v = x.st(0);
                if v == 0.0 {
                    x.sw |= ZE;
                    x.set_st(0, f64::NEG_INFINITY);
                    x.push(v);
                } else if !v.is_finite() {
                    x.set_st(0, if v.is_nan() { v } else { f64::INFINITY });
                    x.push(v);
                } else {
                    let (m, e) = frexp(v);
                    x.set_st(0, (e - 1) as f64);
                    x.push(m * 2.0);
                }
            }
            0x35 | 0x38 => {
                // FPREM1 / FPREM: partial remainder (always complete here).
                let (a, b) = (x.st(0), x.st(1));
                let q = a / b;
                let q = if sub == 0x38 {
                    q.trunc()
                } else {
                    let r = q.round();
                    if (q - q.trunc()).abs() == 0.5 && r % 2.0 != 0.0 {
                        r - q.signum()
                    } else {
                        r
                    }
                };
                let r = if sub == 0x38 { a % b } else { a - q * b };
                let qi = q.abs() as u64;
                x.sw &= !CC;
                if qi & 1 != 0 {
                    x.sw |= C1;
                }
                if qi & 2 != 0 {
                    x.sw |= C3;
                }
                if qi & 4 != 0 {
                    x.sw |= C0;
                }
                x.set_st(0, r);
            }
            0x36 => x.top = (x.top + 7) & 7,
            0x37 => x.top = (x.top + 1) & 7,
            0x3A => {
                let v = x.st(0);
                if v < 0.0 {
                    x.sw |= IE;
                }
                x.set_st(0, v.sqrt());
            }
            0x3B => {
                let v = x.st(0);
                x.sw &= !C2;
                x.set_st(0, v.sin());
                x.push(v.cos());
            }
            0x3C => {
                let v = x.st(0);
                let r = x.round(v);
                x.set_st(0, r);
            }
            0x3D => {
                let (a, b) = (x.st(0), x.st(1));
                x.set_st(0, scale(a, b.trunc().clamp(-100_000.0, 100_000.0) as i32));
            }
            0x3E | 0x3F => {
                let v = x.st(0);
                x.sw &= !C2;
                x.set_st(0, if sub == 0x3E { v.sin() } else { v.cos() });
            }
            _ => return Err(UD),
        }
        Ok(())
    }
}

/// Split v into a mantissa in [0.5, 1) and a power of two.
fn frexp(v: f64) -> (f64, i32) {
    if v == 0.0 || !v.is_finite() {
        return (v, 0);
    }
    let bits = v.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32;
    if exp == 0 {
        // Denormal: scale up first.
        let (m, e) = frexp(v * 2f64.powi(64));
        return (m, e - 64);
    }
    let m = f64::from_bits((bits & !(0x7FF << 52)) | (1022 << 52));
    (m, exp - 1022)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extended_round_trip() {
        for v in [0.0, -0.0, 1.0, -2.5, 3.141592653589793, 1e300, -1e-300, 5e-324, f64::INFINITY] {
            let (m, se) = f64_to_ext(v);
            let back = ext_to_f64(m, se);
            assert_eq!(back.to_bits(), v.to_bits(), "{v}");
        }
        // 1.0 as extended: exponent 3FFF, mantissa 8000...
        assert_eq!(f64_to_ext(1.0), (1 << 63, 0x3FFF));
        assert!(ext_to_f64(1 << 63 | 1, 0x7FFF).is_nan());
    }

    #[test]
    fn rounding_modes() {
        let mut f = Fpu::default();
        assert_eq!(f.round(2.5), 2.0);
        assert_eq!(f.round(3.5), 4.0);
        assert_eq!(f.round(-2.5), -2.0);
        f.cw |= 3 << 10;
        assert_eq!(f.round(-2.7), -2.0);
        f.cw = (f.cw & !0xC00) | 1 << 10;
        assert_eq!(f.round(-2.2), -3.0);
    }

    #[test]
    fn frexp_works() {
        assert_eq!(frexp(8.0), (0.5, 4));
        assert_eq!(frexp(-0.75), (-0.75, 0));
    }
}
