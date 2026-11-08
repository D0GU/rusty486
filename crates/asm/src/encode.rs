//! Instruction encoding.

use crate::expr::{self, Expr, Val};
use crate::operand::{Dist, Mem, Operand};
use crate::regs::Reg;
use crate::{Grow, PassEnv};

const SLOT_IMM: u8 = 0;
const SLOT_DISP: u8 = 1;
const SLOT_JMP: u8 = 2;

pub(crate) struct Ctx<'a, 'e> {
    pub env: &'a PassEnv<'e>,
    pub bits: u8,
    pub last: bool,
    pub grow: &'a mut Grow,
    pub stmt: usize,
    pub out: Vec<u8>,
}

/// An instruction being built: prefixes, opcode, ModR/M+SIB+disp, immediate.
#[derive(Default)]
struct Insn {
    explicit: Vec<u8>,
    seg: Option<u8>,
    /// Operand-size prefix implied by the mnemonic itself (stosw, cwde...).
    fixed_o32: bool,
    o32: bool,
    a32: bool,
    op: Vec<u8>,
    modrm: Vec<u8>,
    imm: Vec<u8>,
}

impl Insn {
    /// Prefix order matches NASM: mnemonic-implied 66h, segment override,
    /// 67h, explicit prefixes (lock/rep), then operand-derived 66h.
    fn bytes(&self) -> Vec<u8> {
        let mut b = Vec::new();
        if self.fixed_o32 {
            b.push(0x66);
        }
        if let Some(s) = self.seg {
            b.push(SEG_PREFIX[s as usize]);
        }
        if self.a32 {
            b.push(0x67);
        }
        b.extend_from_slice(&self.explicit);
        if self.o32 && !self.fixed_o32 {
            b.push(0x66);
        }
        b.extend_from_slice(&self.op);
        b.extend_from_slice(&self.modrm);
        b.extend_from_slice(&self.imm);
        b
    }

    fn prefix_len(&self) -> usize {
        self.explicit.len()
            + self.seg.is_some() as usize
            + self.a32 as usize
            + (self.o32 || self.fixed_o32) as usize
    }
}

const SEG_PREFIX: [u8; 6] = [0x26, 0x2E, 0x36, 0x3E, 0x64, 0x65];

const ALU: [&str; 8] = ["add", "or", "adc", "sbb", "and", "sub", "xor", "cmp"];
const SHIFTS: [&str; 8] = ["rol", "ror", "rcl", "rcr", "shl", "shr", "sal", "sar"];
const GRP3: [&str; 5] = ["not", "neg", "mul", "div", "idiv"];

/// Condition-code suffixes and their numbers.
fn cc(s: &str) -> Option<u8> {
    Some(match s {
        "o" => 0,
        "no" => 1,
        "b" | "c" | "nae" => 2,
        "ae" | "nb" | "nc" => 3,
        "e" | "z" => 4,
        "ne" | "nz" => 5,
        "be" | "na" => 6,
        "a" | "nbe" => 7,
        "s" => 8,
        "ns" => 9,
        "p" | "pe" => 10,
        "np" | "po" => 11,
        "l" | "nge" => 12,
        "ge" | "nl" => 13,
        "le" | "ng" => 14,
        "g" | "nle" => 15,
        _ => return None,
    })
}

/// Instructions with no operands: (mnemonic, bytes, operand size or 0).
const SIMPLE: &[(&str, &[u8], u8)] = &[
    ("nop", &[0x90], 0),
    ("hlt", &[0xF4], 0),
    ("cli", &[0xFA], 0),
    ("sti", &[0xFB], 0),
    ("cld", &[0xFC], 0),
    ("std", &[0xFD], 0),
    ("clc", &[0xF8], 0),
    ("stc", &[0xF9], 0),
    ("cmc", &[0xF5], 0),
    ("int3", &[0xCC], 0),
    ("into", &[0xCE], 0),
    ("int1", &[0xF1], 0),
    ("icebp", &[0xF1], 0),
    // Size 0 = current default operand size (no prefix).
    ("iret", &[0xCF], 0),
    ("iretw", &[0xCF], 16),
    ("iretd", &[0xCF], 32),
    ("leave", &[0xC9], 0),
    ("pusha", &[0x60], 0),
    ("pushaw", &[0x60], 16),
    ("pushad", &[0x60], 32),
    ("popa", &[0x61], 0),
    ("popaw", &[0x61], 16),
    ("popad", &[0x61], 32),
    ("pushf", &[0x9C], 0),
    ("pushfw", &[0x9C], 16),
    ("pushfd", &[0x9C], 32),
    ("popf", &[0x9D], 0),
    ("popfw", &[0x9D], 16),
    ("popfd", &[0x9D], 32),
    ("cbw", &[0x98], 16),
    ("cwde", &[0x98], 32),
    ("cwd", &[0x99], 16),
    ("cdq", &[0x99], 32),
    ("sahf", &[0x9E], 0),
    ("lahf", &[0x9F], 0),
    ("xlat", &[0xD7], 0),
    ("xlatb", &[0xD7], 0),
    ("aaa", &[0x37], 0),
    ("aas", &[0x3F], 0),
    ("daa", &[0x27], 0),
    ("das", &[0x2F], 0),
    ("salc", &[0xD6], 0),
    ("wait", &[0x9B], 0),
    ("fwait", &[0x9B], 0),
    ("cpuid", &[0x0F, 0xA2], 0),
    ("clts", &[0x0F, 0x06], 0),
    ("invd", &[0x0F, 0x08], 0),
    ("wbinvd", &[0x0F, 0x09], 0),
    ("movsb", &[0xA4], 0),
    ("movsw", &[0xA5], 16),
    ("movsd", &[0xA5], 32),
    ("cmpsb", &[0xA6], 0),
    ("cmpsw", &[0xA7], 16),
    ("cmpsd", &[0xA7], 32),
    ("stosb", &[0xAA], 0),
    ("stosw", &[0xAB], 16),
    ("stosd", &[0xAB], 32),
    ("lodsb", &[0xAC], 0),
    ("lodsw", &[0xAD], 16),
    ("lodsd", &[0xAD], 32),
    ("scasb", &[0xAE], 0),
    ("scasw", &[0xAF], 16),
    ("scasd", &[0xAF], 32),
    ("insb", &[0x6C], 0),
    ("insw", &[0x6D], 16),
    ("insd", &[0x6D], 32),
    ("outsb", &[0x6E], 0),
    ("outsw", &[0x6F], 16),
    ("outsd", &[0x6F], 32),
];

const OTHER: &[&str] = &[
    "mov", "test", "xchg", "inc", "dec", "imul", "push", "pop", "jmp", "call", "ret", "retn", "retf",
    "int", "in", "out", "lea", "lds", "les", "lss", "lfs", "lgs", "bound", "enter", "aam", "aad",
    "movzx", "movsx", "bt", "bts", "btr", "btc", "bsf", "bsr", "shld", "shrd", "xadd", "cmpxchg",
    "bswap", "lgdt", "lidt", "sgdt", "sidt", "lmsw", "smsw", "invlpg", "sldt", "str", "lldt", "ltr",
    "verr", "verw", "lar", "lsl", "arpl", "loop", "loope", "loopz", "loopne", "loopnz", "jcxz",
    "jecxz", "rep", "repe", "repz", "repne", "repnz", "lock",
];

pub fn is_mnemonic(m: &str) -> bool {
    ALU.contains(&m)
        || SHIFTS.contains(&m)
        || GRP3.contains(&m)
        || SIMPLE.iter().any(|s| s.0 == m)
        || OTHER.contains(&m)
        || m.strip_prefix('j').and_then(cc).is_some()
        || m.strip_prefix("set").and_then(cc).is_some()
}

fn mask(size: u8) -> i64 {
    match size {
        8 => 0xFF,
        16 => 0xFFFF,
        _ => 0xFFFF_FFFF,
    }
}

/// Does `v` (as an operand of `size` bits) survive sign-extension from a byte?
fn fits_sbyte(v: i64, size: u8) -> bool {
    let x = v & mask(size);
    ((x as u8 as i8) as i64) & mask(size) == x
}

fn fits_i8(v: i64) -> bool {
    (-128..=127).contains(&v)
}

impl Ctx<'_, '_> {
    // -------------------------------------------------------------- values

    fn val(&self, e: &Expr) -> Result<Val, String> {
        let v = expr::eval(e, self.env)?;
        if !v.known && self.last {
            return Err(match first_unknown(e, self.env) {
                Some(s) => format!("undefined symbol '{s}'"),
                None => "expression depends on an undefined symbol".into(),
            });
        }
        Ok(v)
    }

    pub fn value(&self, e: &Expr) -> Result<i64, String> {
        Ok(self.val(e)?.v)
    }

    /// A value that must be known to lay out code (times counts, etc.).
    /// Early passes may see forward references, treated as 0.
    pub fn const_value(&self, e: &Expr, what: &str) -> Result<i64, String> {
        let v = expr::eval(e, self.env)?;
        if !v.known && self.last {
            return Err(format!("{what} must not depend on undefined symbols"));
        }
        Ok(v.v)
    }

    /// Decide whether to use the long form for a slot. `fits` is whether the
    /// short form works with the current value (None = unknown yet).
    fn long(&mut self, slot: u8, fits: Option<bool>) -> bool {
        let key = (self.stmt, slot);
        if self.grow.contains(&key) {
            return true;
        }
        if fits == Some(false) {
            self.grow.insert(key);
            return true;
        }
        false
    }

    fn imm_bytes(&self, v: i64, size: u8) -> Vec<u8> {
        v.to_le_bytes()[..(size / 8) as usize].to_vec()
    }

    // -------------------------------------------------------------- helpers

    fn osz(&self, i: &mut Insn, size: u8) {
        if size != 8 && (size == 32) != (self.bits == 32) {
            i.o32 = true;
        }
    }

    fn reg_rm(&self, i: &mut Insn, reg: u8, rm: u8) {
        i.modrm = vec![0xC0 | (reg & 7) << 3 | (rm & 7)];
    }

    /// Encode a ModR/M memory operand with `reg` in the reg field.
    fn mem(&mut self, i: &mut Insn, m: &Mem, reg: u8) -> Result<(), String> {
        if let Some(s) = m.seg {
            i.seg = Some(s);
        }
        let dv = self.val(&m.disp)?;
        let asize = match m.regs.first() {
            Some((Reg::Gpr(s @ (16 | 32), _), _)) => *s,
            Some(_) => return Err("invalid register in address".into()),
            None => self.bits,
        };
        if m.regs.iter().any(|(r, _)| !matches!(r, Reg::Gpr(s, _) if *s == asize)) {
            return Err("mixed or invalid registers in address".into());
        }
        if asize != self.bits {
            i.a32 = true;
        }
        let disp_is_zero = dv.known && dv.v == 0;
        let const_disp = matches!(m.disp, Expr::Num(_)) || !expr::has_symbols(&m.disp);
        // Short displacement choice (disp8) with the grow-only optimiser.
        let fits8 = if dv.known { Some(fits_i8(dv.v)) } else { None };

        if asize == 16 {
            let mut base = None;
            let mut index = None;
            for (r, k) in &m.regs {
                let Reg::Gpr(_, n) = r else { unreachable!() };
                if *k != 1 {
                    return Err("16-bit addresses cannot scale registers".into());
                }
                match n {
                    3 | 5 if base.is_none() => base = Some(*n),
                    6 | 7 if index.is_none() => index = Some(*n),
                    _ => return Err("invalid 16-bit address registers".into()),
                }
            }
            let rm = match (base, index) {
                (Some(3), Some(6)) => 0,
                (Some(3), Some(7)) => 1,
                (Some(5), Some(6)) => 2,
                (Some(5), Some(7)) => 3,
                (None, Some(6)) => 4,
                (None, Some(7)) => 5,
                (Some(5), None) => 6,
                (Some(3), None) => 7,
                (None, None) => {
                    i.modrm = vec![(reg & 7) << 3 | 6];
                    i.modrm.extend_from_slice(&(dv.v as u16).to_le_bytes());
                    return Ok(());
                }
                _ => return Err("invalid 16-bit address".into()),
            };
            let md = if disp_is_zero && const_disp && rm != 6 {
                0
            } else if !self.long(SLOT_DISP, fits8) {
                1
            } else {
                2
            };
            i.modrm = vec![md << 6 | (reg & 7) << 3 | rm];
            match md {
                1 => i.modrm.push(dv.v as u8),
                2 => i.modrm.extend_from_slice(&(dv.v as u16).to_le_bytes()),
                _ => {}
            }
            return Ok(());
        }

        // 32-bit addressing: merge repeated registers, then pick base/index.
        let mut terms: Vec<(u8, i64)> = Vec::new();
        for (r, k) in &m.regs {
            let Reg::Gpr(_, n) = r else { unreachable!() };
            match terms.iter_mut().find(|t| t.0 == *n) {
                Some(t) => t.1 += k,
                None => terms.push((*n, *k)),
            }
        }
        terms.retain(|t| t.1 != 0);
        let (mut base, mut index): (Option<u8>, Option<(u8, i64)>) = match terms.as_slice() {
            [] => (None, None),
            [(r, 1)] => (Some(*r), None),
            [(r, 2)] => (Some(*r), Some((*r, 1))),
            [(r, k @ (3 | 5 | 9))] => (Some(*r), Some((*r, k - 1))),
            [(r, k @ (4 | 8))] => (None, Some((*r, *k))),
            [(a, 1), (b, k @ (1 | 2 | 4 | 8))] => (Some(*a), Some((*b, *k))),
            [(a, k @ (2 | 4 | 8)), (b, 1)] => (Some(*b), Some((*a, *k))),
            _ => return Err("invalid effective address".into()),
        };
        if let Some((4, k)) = index {
            // ESP can't be an index; swap with the base if unscaled.
            match base {
                Some(b) if k == 1 && b != 4 => {
                    index = Some((b, 1));
                    base = Some(4);
                }
                _ => return Err("ESP cannot be used as an index register".into()),
            }
        }
        let scale_bits = |k: i64| match k {
            1 => 0u8,
            2 => 1,
            4 => 2,
            _ => 3,
        };
        match (base, index) {
            (None, None) => {
                i.modrm = vec![(reg & 7) << 3 | 5];
                i.modrm.extend_from_slice(&(dv.v as u32).to_le_bytes());
            }
            (None, Some((x, k))) => {
                i.modrm = vec![(reg & 7) << 3 | 4, scale_bits(k) << 6 | x << 3 | 5];
                i.modrm.extend_from_slice(&(dv.v as u32).to_le_bytes());
            }
            (Some(b), idx) => {
                let md = if disp_is_zero && const_disp && b != 5 {
                    0
                } else if !self.long(SLOT_DISP, fits8) {
                    1
                } else {
                    2
                };
                let need_sib = idx.is_some() || b == 4;
                if need_sib {
                    let (x, k) = idx.unwrap_or((4, 1));
                    i.modrm = vec![md << 6 | (reg & 7) << 3 | 4, scale_bits(k) << 6 | x << 3 | b];
                } else {
                    i.modrm = vec![md << 6 | (reg & 7) << 3 | b];
                }
                match md {
                    1 => i.modrm.push(dv.v as u8),
                    2 => i.modrm.extend_from_slice(&(dv.v as u32).to_le_bytes()),
                    _ => {}
                }
            }
        }
        Ok(())
    }

    /// ModR/M for a register-or-memory operand.
    fn rm(&mut self, i: &mut Insn, op: &Operand, reg: u8) -> Result<(), String> {
        match op {
            Operand::Reg(Reg::Gpr(_, n)) => {
                self.reg_rm(i, reg, *n);
                Ok(())
            }
            Operand::Mem(m) => self.mem(i, m, reg),
            _ => Err("expected a register or memory operand".into()),
        }
    }

    /// Size of a register/memory operand (memory needs an explicit size).
    fn rm_size(&self, op: &Operand) -> Option<u8> {
        match op {
            Operand::Reg(Reg::Gpr(s, _)) => Some(*s),
            Operand::Mem(m) => m.size,
            _ => None,
        }
    }

    fn emit(&mut self, i: &Insn) {
        self.out.extend_from_slice(&i.bytes());
    }

    // -------------------------------------------------------------- instructions

    pub fn instruction(&mut self, prefixes: &[String], m: &str, ops: &[Operand]) -> Result<(), String> {
        let mut i = Insn::default();
        for p in prefixes {
            match p.as_str() {
                "rep" | "repe" | "repz" => i.explicit.push(0xF3),
                "repne" | "repnz" => i.explicit.push(0xF2),
                "lock" => i.explicit.push(0xF0),
                "es" => i.explicit.push(0x26),
                "cs" => i.explicit.push(0x2E),
                "ss" => i.explicit.push(0x36),
                "ds" => i.explicit.push(0x3E),
                "fs" => i.explicit.push(0x64),
                "gs" => i.explicit.push(0x65),
                "o16" => i.o32 = self.bits == 32,
                "o32" => i.o32 = self.bits == 16,
                "a32" => i.a32 = self.bits == 16,
                _ => {}
            }
        }
        // Bare prefix used as an instruction (e.g. "rep" on its own line).
        match m {
            "rep" | "repe" | "repz" => i.op.push(0xF3),
            "repne" | "repnz" => i.op.push(0xF2),
            "lock" => i.op.push(0xF0),
            _ => {}
        }
        if !i.op.is_empty() {
            self.emit(&i);
            return Ok(());
        }

        if let Some(&(_, bytes, size)) = SIMPLE.iter().find(|s| s.0 == m) {
            if !ops.is_empty() {
                return Err(format!("'{m}' takes no operands"));
            }
            if size != 0 && (size == 32) != (self.bits == 32) {
                i.fixed_o32 = true;
            }
            i.op = bytes.to_vec();
            self.emit(&i);
            return Ok(());
        }
        if let Some(n) = ALU.iter().position(|x| *x == m) {
            return self.alu(i, n as u8, ops);
        }
        if let Some(n) = SHIFTS.iter().position(|x| *x == m) {
            let n = if m == "sal" { 4 } else if m == "sar" { 7 } else { n as u8 };
            return self.shift(i, n, ops);
        }
        if let Some(n) = GRP3.iter().position(|x| *x == m) {
            let ext = [2, 3, 4, 6, 7][n];
            let [a] = ops else { return Err(format!("'{m}' takes one operand")) };
            let size = self.rm_size(a).ok_or("operation size not specified")?;
            self.osz(&mut i, size);
            i.op = vec![if size == 8 { 0xF6 } else { 0xF7 }];
            self.rm(&mut i, a, ext)?;
            self.emit(&i);
            return Ok(());
        }
        if let Some(c) = m.strip_prefix('j').and_then(cc) {
            return self.jump(i, Jump::Cond(c), ops);
        }
        if let Some(c) = m.strip_prefix("set").and_then(cc) {
            let [a] = ops else { return Err("setcc takes one operand".into()) };
            if self.rm_size(a).is_some_and(|s| s != 8) {
                return Err("setcc needs a byte operand".into());
            }
            i.op = vec![0x0F, 0x90 + c];
            self.rm(&mut i, a, 0)?;
            self.emit(&i);
            return Ok(());
        }

        match m {
            "mov" => self.mov(i, ops),
            "test" => self.test(i, ops),
            "xchg" => self.xchg(i, ops),
            "inc" | "dec" => {
                let [a] = ops else { return Err(format!("'{m}' takes one operand")) };
                let ext = (m == "dec") as u8;
                let size = self.rm_size(a).ok_or("operation size not specified")?;
                self.osz(&mut i, size);
                match a.gpr() {
                    Some((s, n)) if s != 8 => i.op = vec![0x40 + ext * 8 + n],
                    _ => {
                        i.op = vec![if size == 8 { 0xFE } else { 0xFF }];
                        self.rm(&mut i, a, ext)?;
                    }
                }
                self.emit(&i);
                Ok(())
            }
            "imul" => self.imul(i, ops),
            "push" | "pop" => self.push_pop(i, m == "push", ops),
            "jmp" => self.jump(i, Jump::Jmp, ops),
            "call" => self.jump(i, Jump::Call, ops),
            "loop" => self.jump(i, Jump::Loop(0xE2), ops),
            "loope" | "loopz" => self.jump(i, Jump::Loop(0xE1), ops),
            "loopne" | "loopnz" => self.jump(i, Jump::Loop(0xE0), ops),
            "jcxz" => {
                i.a32 = self.bits == 32;
                self.jump(i, Jump::Loop(0xE3), ops)
            }
            "jecxz" => {
                i.a32 = self.bits == 16;
                self.jump(i, Jump::Loop(0xE3), ops)
            }
            "ret" | "retn" | "retf" => {
                let far = m == "retf";
                match ops {
                    [] => i.op = vec![if far { 0xCB } else { 0xC3 }],
                    [Operand::Imm { expr, .. }] => {
                        let v = self.value(expr)?;
                        i.op = vec![if far { 0xCA } else { 0xC2 }];
                        i.imm = self.imm_bytes(v, 16);
                    }
                    _ => return Err("ret takes an optional immediate".into()),
                }
                self.emit(&i);
                Ok(())
            }
            "int" => {
                let [Operand::Imm { expr, .. }] = ops else { return Err("int takes an immediate".into()) };
                let v = self.value(expr)?;
                i.op = vec![0xCD, v as u8];
                self.emit(&i);
                Ok(())
            }
            "in" | "out" => self.in_out(i, m == "in", ops),
            "lea" | "lds" | "les" | "lss" | "lfs" | "lgs" | "bound" | "lar" | "lsl" => {
                let [Operand::Reg(Reg::Gpr(s, r)), b] = ops else {
                    return Err(format!("'{m}' needs a register and a memory operand"));
                };
                if !matches!(b, Operand::Mem(_)) && !matches!(m, "lar" | "lsl") {
                    return Err(format!("'{m}' needs a memory operand"));
                }
                if *s == 8 {
                    return Err(format!("'{m}' needs a 16 or 32-bit register"));
                }
                self.osz(&mut i, *s);
                i.op = match m {
                    "lea" => vec![0x8D],
                    "lds" => vec![0xC5],
                    "les" => vec![0xC4],
                    "lss" => vec![0x0F, 0xB2],
                    "lfs" => vec![0x0F, 0xB4],
                    "lgs" => vec![0x0F, 0xB5],
                    "lar" => vec![0x0F, 0x02],
                    "lsl" => vec![0x0F, 0x03],
                    _ => vec![0x62],
                };
                self.rm(&mut i, b, *r)?;
                self.emit(&i);
                Ok(())
            }
            "enter" => {
                let [Operand::Imm { expr: a, .. }, Operand::Imm { expr: b, .. }] = ops else {
                    return Err("enter takes two immediates".into());
                };
                let (a, b) = (self.value(a)?, self.value(b)?);
                i.op = vec![0xC8];
                i.imm = self.imm_bytes(a, 16);
                i.imm.push(b as u8);
                self.emit(&i);
                Ok(())
            }
            "aam" | "aad" => {
                let base = match ops {
                    [] => 10,
                    [Operand::Imm { expr, .. }] => self.value(expr)?,
                    _ => return Err(format!("'{m}' takes an optional immediate")),
                };
                i.op = vec![if m == "aam" { 0xD4 } else { 0xD5 }, base as u8];
                self.emit(&i);
                Ok(())
            }
            "movzx" | "movsx" => {
                let [Operand::Reg(Reg::Gpr(ds, dr)), b] = ops else {
                    return Err(format!("'{m}' needs a register destination"));
                };
                let ss = self.rm_size(b).ok_or("operation size not specified")?;
                if *ds == 8 || ss >= *ds {
                    return Err(format!("invalid operand sizes for '{m}'"));
                }
                self.osz(&mut i, *ds);
                let base = if m == "movzx" { 0xB6 } else { 0xBE };
                i.op = vec![0x0F, base + (ss == 16) as u8];
                self.rm(&mut i, b, *dr)?;
                self.emit(&i);
                Ok(())
            }
            "bt" | "bts" | "btr" | "btc" => {
                let n = ["bt", "bts", "btr", "btc"].iter().position(|x| *x == m).unwrap() as u8;
                let [a, b] = ops else { return Err(format!("'{m}' takes two operands")) };
                let size = self.rm_size(a).or(b.gpr().map(|g| g.0)).ok_or("operation size not specified")?;
                self.osz(&mut i, size);
                match b {
                    Operand::Reg(Reg::Gpr(_, r)) => {
                        i.op = vec![0x0F, [0xA3, 0xAB, 0xB3, 0xBB][n as usize]];
                        self.rm(&mut i, a, *r)?;
                    }
                    Operand::Imm { expr, .. } => {
                        let v = self.value(expr)?;
                        i.op = vec![0x0F, 0xBA];
                        self.rm(&mut i, a, 4 + n)?;
                        i.imm = vec![v as u8];
                    }
                    _ => return Err(format!("invalid operands for '{m}'")),
                }
                self.emit(&i);
                Ok(())
            }
            "bsf" | "bsr" => {
                let [Operand::Reg(Reg::Gpr(s, r)), b] = ops else {
                    return Err(format!("'{m}' needs a register destination"));
                };
                self.osz(&mut i, *s);
                i.op = vec![0x0F, if m == "bsf" { 0xBC } else { 0xBD }];
                self.rm(&mut i, b, *r)?;
                self.emit(&i);
                Ok(())
            }
            "shld" | "shrd" => {
                let [a, Operand::Reg(Reg::Gpr(s, r)), c] = ops else {
                    return Err(format!("'{m}' needs r/m, register, count"));
                };
                self.osz(&mut i, *s);
                let base = if m == "shld" { 0xA4 } else { 0xAC };
                match c {
                    Operand::Imm { expr, .. } => {
                        let v = self.value(expr)?;
                        i.op = vec![0x0F, base];
                        self.rm(&mut i, a, *r)?;
                        i.imm = vec![v as u8];
                    }
                    Operand::Reg(Reg::Gpr(8, 1)) => {
                        i.op = vec![0x0F, base + 1];
                        self.rm(&mut i, a, *r)?;
                    }
                    _ => return Err(format!("'{m}' count must be an immediate or CL")),
                }
                self.emit(&i);
                Ok(())
            }
            "xadd" | "cmpxchg" | "arpl" => {
                let [a, Operand::Reg(Reg::Gpr(s, r))] = ops else {
                    return Err(format!("'{m}' needs r/m, register"));
                };
                self.osz(&mut i, *s);
                i.op = match m {
                    "xadd" => vec![0x0F, 0xC0 + (*s != 8) as u8],
                    "cmpxchg" => vec![0x0F, 0xB0 + (*s != 8) as u8],
                    _ => vec![0x63],
                };
                self.rm(&mut i, a, *r)?;
                self.emit(&i);
                Ok(())
            }
            "bswap" => {
                let [Operand::Reg(Reg::Gpr(32, r))] = ops else { return Err("bswap needs a 32-bit register".into()) };
                self.osz(&mut i, 32);
                i.op = vec![0x0F, 0xC8 + r];
                self.emit(&i);
                Ok(())
            }
            "lgdt" | "lidt" | "sgdt" | "sidt" | "invlpg" => {
                let [a @ Operand::Mem(_)] = ops else { return Err(format!("'{m}' needs a memory operand")) };
                let ext = match m {
                    "sgdt" => 0,
                    "sidt" => 1,
                    "lgdt" => 2,
                    "lidt" => 3,
                    _ => 7,
                };
                i.op = vec![0x0F, 0x01];
                self.rm(&mut i, a, ext)?;
                self.emit(&i);
                Ok(())
            }
            "lmsw" | "smsw" | "sldt" | "str" | "lldt" | "ltr" | "verr" | "verw" => {
                let [a] = ops else { return Err(format!("'{m}' takes one operand")) };
                // NASM only adds o32 for a 32-bit register in 16-bit code.
                if a.gpr().is_some_and(|g| g.0 == 32) && self.bits == 16 && matches!(m, "smsw" | "sldt" | "str") {
                    i.o32 = true;
                }
                let (op2, ext) = match m {
                    "smsw" => (0x01, 4),
                    "lmsw" => (0x01, 6),
                    "sldt" => (0x00, 0),
                    "str" => (0x00, 1),
                    "lldt" => (0x00, 2),
                    "ltr" => (0x00, 3),
                    "verr" => (0x00, 4),
                    _ => (0x00, 5),
                };
                i.op = vec![0x0F, op2];
                self.rm(&mut i, a, ext)?;
                self.emit(&i);
                Ok(())
            }
            _ => Err(format!("unknown instruction '{m}'")),
        }
    }

    fn alu(&mut self, mut i: Insn, n: u8, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err(format!("'{}' takes two operands", ALU[n as usize])) };
        match (a, b) {
            (Operand::Reg(Reg::Gpr(sa, ra)), Operand::Reg(Reg::Gpr(sb, rb))) => {
                if sa != sb {
                    return Err("operand sizes don't match".into());
                }
                self.osz(&mut i, *sa);
                i.op = vec![n * 8 + (*sa != 8) as u8];
                self.reg_rm(&mut i, *rb, *ra);
            }
            (Operand::Mem(m), Operand::Reg(Reg::Gpr(s, r))) => {
                self.osz(&mut i, *s);
                i.op = vec![n * 8 + (*s != 8) as u8];
                self.mem(&mut i, m, *r)?;
            }
            (Operand::Reg(Reg::Gpr(s, r)), Operand::Mem(m)) => {
                self.osz(&mut i, *s);
                i.op = vec![n * 8 + 2 + (*s != 8) as u8];
                self.mem(&mut i, m, *r)?;
            }
            (dst, Operand::Imm { expr, size: isz, .. }) => {
                let size = self.rm_size(dst).ok_or("operation size not specified")?;
                let v = self.val(expr)?;
                self.osz(&mut i, size);
                let fits = if *isz == Some(8) {
                    Some(true)
                } else if v.known {
                    Some(fits_sbyte(v.v, size))
                } else {
                    None
                };
                let acc = dst.gpr().is_some_and(|g| g.1 == 0);
                if size == 8 {
                    if acc {
                        i.op = vec![0x04 + n * 8];
                    } else {
                        i.op = vec![0x80];
                        self.rm(&mut i, dst, n)?;
                    }
                    i.imm = vec![v.v as u8];
                } else if !self.long(SLOT_IMM, fits) {
                    i.op = vec![0x83];
                    self.rm(&mut i, dst, n)?;
                    i.imm = vec![v.v as u8];
                } else {
                    if acc {
                        i.op = vec![0x05 + n * 8];
                    } else {
                        i.op = vec![0x81];
                        self.rm(&mut i, dst, n)?;
                    }
                    i.imm = self.imm_bytes(v.v, size);
                }
            }
            _ => return Err("invalid operands".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn test(&mut self, mut i: Insn, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err("test takes two operands".into()) };
        match (a, b) {
            (Operand::Reg(Reg::Gpr(sa, ra)), Operand::Reg(Reg::Gpr(sb, rb))) => {
                if sa != sb {
                    return Err("operand sizes don't match".into());
                }
                self.osz(&mut i, *sa);
                i.op = vec![0x84 + (*sa != 8) as u8];
                self.reg_rm(&mut i, *rb, *ra);
            }
            (Operand::Mem(m), Operand::Reg(Reg::Gpr(s, r))) | (Operand::Reg(Reg::Gpr(s, r)), Operand::Mem(m)) => {
                self.osz(&mut i, *s);
                i.op = vec![0x84 + (*s != 8) as u8];
                self.mem(&mut i, m, *r)?;
            }
            (dst, Operand::Imm { expr, .. }) => {
                let size = self.rm_size(dst).ok_or("operation size not specified")?;
                let v = self.value(expr)?;
                self.osz(&mut i, size);
                if dst.gpr().is_some_and(|g| g.1 == 0) {
                    i.op = vec![0xA8 + (size != 8) as u8];
                } else {
                    i.op = vec![0xF6 + (size != 8) as u8];
                    self.rm(&mut i, dst, 0)?;
                }
                i.imm = self.imm_bytes(v, size);
            }
            _ => return Err("invalid operands".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn mov(&mut self, mut i: Insn, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err("mov takes two operands".into()) };
        let absolute = |m: &Mem| m.regs.is_empty();
        match (a, b) {
            (Operand::Reg(Reg::Gpr(sa, ra)), Operand::Reg(Reg::Gpr(sb, rb))) => {
                if sa != sb {
                    return Err("operand sizes don't match".into());
                }
                self.osz(&mut i, *sa);
                i.op = vec![0x88 + (*sa != 8) as u8];
                self.reg_rm(&mut i, *rb, *ra);
            }
            (Operand::Mem(m), Operand::Reg(Reg::Gpr(s, r))) | (Operand::Reg(Reg::Gpr(s, r)), Operand::Mem(m)) => {
                let load = matches!(a, Operand::Reg(_));
                self.osz(&mut i, *s);
                if *r == 0 && absolute(m) {
                    // moffs form: A0-A3 with a plain address.
                    if let Some(sg) = m.seg {
                        i.seg = Some(sg);
                    }
                    i.op = vec![if load { 0xA0 } else { 0xA2 } + (*s != 8) as u8];
                    let v = self.value(&m.disp)?;
                    i.modrm = self.imm_bytes(v, self.bits);
                } else {
                    i.op = vec![if load { 0x8A } else { 0x88 } + (*s != 8) as u8];
                    self.mem(&mut i, m, *r)?;
                }
            }
            (Operand::Reg(Reg::Gpr(s, r)), Operand::Imm { expr, .. }) => {
                let v = self.value(expr)?;
                self.osz(&mut i, *s);
                i.op = vec![if *s == 8 { 0xB0 } else { 0xB8 } + r];
                i.imm = self.imm_bytes(v, *s);
            }
            (Operand::Mem(m), Operand::Imm { expr, size: isz, .. }) => {
                let size = m.size.or(*isz).ok_or("operation size not specified")?;
                let v = self.value(expr)?;
                self.osz(&mut i, size);
                i.op = vec![0xC6 + (size != 8) as u8];
                self.mem(&mut i, m, 0)?;
                i.imm = self.imm_bytes(v, size);
            }
            (dst, Operand::Reg(Reg::Seg(sr))) => {
                if let Some((s, _)) = dst.gpr() {
                    self.osz(&mut i, s);
                }
                i.op = vec![0x8C];
                self.rm(&mut i, dst, *sr)?;
            }
            (Operand::Reg(Reg::Seg(sr)), src) => {
                if *sr == 1 {
                    return Err("cannot load CS with mov".into());
                }
                i.op = vec![0x8E];
                self.rm(&mut i, src, *sr)?;
            }
            (Operand::Reg(Reg::Cr(c)), Operand::Reg(Reg::Gpr(32, r))) => {
                i.op = vec![0x0F, 0x22];
                self.reg_rm(&mut i, *c, *r);
            }
            (Operand::Reg(Reg::Gpr(32, r)), Operand::Reg(Reg::Cr(c))) => {
                i.op = vec![0x0F, 0x20];
                self.reg_rm(&mut i, *c, *r);
            }
            (Operand::Reg(Reg::Dr(d)), Operand::Reg(Reg::Gpr(32, r))) => {
                i.op = vec![0x0F, 0x23];
                self.reg_rm(&mut i, *d, *r);
            }
            (Operand::Reg(Reg::Gpr(32, r)), Operand::Reg(Reg::Dr(d))) => {
                i.op = vec![0x0F, 0x21];
                self.reg_rm(&mut i, *d, *r);
            }
            _ => return Err("invalid operands for mov".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn xchg(&mut self, mut i: Insn, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err("xchg takes two operands".into()) };
        match (a, b) {
            (Operand::Reg(Reg::Gpr(sa, ra)), Operand::Reg(Reg::Gpr(sb, rb))) => {
                if sa != sb {
                    return Err("operand sizes don't match".into());
                }
                self.osz(&mut i, *sa);
                if *sa != 8 && (*ra == 0 || *rb == 0) {
                    i.op = vec![0x90 + if *ra == 0 { *rb } else { *ra }];
                } else {
                    i.op = vec![0x86 + (*sa != 8) as u8];
                    self.reg_rm(&mut i, *ra, *rb);
                }
            }
            (Operand::Mem(m), Operand::Reg(Reg::Gpr(s, r))) | (Operand::Reg(Reg::Gpr(s, r)), Operand::Mem(m)) => {
                self.osz(&mut i, *s);
                i.op = vec![0x86 + (*s != 8) as u8];
                self.mem(&mut i, m, *r)?;
            }
            _ => return Err("invalid operands for xchg".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn shift(&mut self, mut i: Insn, n: u8, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err("shift takes two operands".into()) };
        let size = self.rm_size(a).ok_or("operation size not specified")?;
        self.osz(&mut i, size);
        let w = (size != 8) as u8;
        match b {
            Operand::Reg(Reg::Gpr(8, 1)) => {
                i.op = vec![0xD2 + w];
                self.rm(&mut i, a, n)?;
            }
            Operand::Imm { expr, .. } => {
                let v = self.val(expr)?;
                if v.known && v.v == 1 {
                    i.op = vec![0xD0 + w];
                    self.rm(&mut i, a, n)?;
                } else {
                    i.op = vec![0xC0 + w];
                    self.rm(&mut i, a, n)?;
                    i.imm = vec![v.v as u8];
                }
            }
            _ => return Err("shift count must be an immediate or CL".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn imul(&mut self, mut i: Insn, ops: &[Operand]) -> Result<(), String> {
        match ops {
            [a] => {
                let size = self.rm_size(a).ok_or("operation size not specified")?;
                self.osz(&mut i, size);
                i.op = vec![if size == 8 { 0xF6 } else { 0xF7 }];
                self.rm(&mut i, a, 5)?;
            }
            [Operand::Reg(Reg::Gpr(s, r)), src @ (Operand::Reg(_) | Operand::Mem(_))] => {
                self.osz(&mut i, *s);
                i.op = vec![0x0F, 0xAF];
                self.rm(&mut i, src, *r)?;
            }
            [d @ Operand::Reg(Reg::Gpr(s, r)), Operand::Imm { expr, size: isz, .. }]
            | [Operand::Reg(Reg::Gpr(s, r)), d @ (Operand::Reg(_) | Operand::Mem(_)), Operand::Imm { expr, size: isz, .. }] => {
                let v = self.val(expr)?;
                self.osz(&mut i, *s);
                let fits = if *isz == Some(8) {
                    Some(true)
                } else if v.known {
                    Some(fits_sbyte(v.v, *s))
                } else {
                    None
                };
                if !self.long(SLOT_IMM, fits) {
                    i.op = vec![0x6B];
                    self.rm(&mut i, d, *r)?;
                    i.imm = vec![v.v as u8];
                } else {
                    i.op = vec![0x69];
                    self.rm(&mut i, d, *r)?;
                    i.imm = self.imm_bytes(v.v, *s);
                }
            }
            _ => return Err("invalid operands for imul".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn push_pop(&mut self, mut i: Insn, push: bool, ops: &[Operand]) -> Result<(), String> {
        let [a] = ops else { return Err("push/pop take one operand".into()) };
        match a {
            Operand::Reg(Reg::Gpr(8, _)) => return Err("cannot push/pop an 8-bit register".into()),
            Operand::Reg(Reg::Gpr(s, r)) => {
                self.osz(&mut i, *s);
                i.op = vec![if push { 0x50 } else { 0x58 } + r];
            }
            Operand::Reg(Reg::Seg(s)) => {
                i.op = match (push, s) {
                    (true, 0) => vec![0x06],
                    (true, 1) => vec![0x0E],
                    (true, 2) => vec![0x16],
                    (true, 3) => vec![0x1E],
                    (true, 4) => vec![0x0F, 0xA0],
                    (true, _) => vec![0x0F, 0xA8],
                    (false, 0) => vec![0x07],
                    (false, 1) => return Err("cannot pop CS".into()),
                    (false, 2) => vec![0x17],
                    (false, 3) => vec![0x1F],
                    (false, 4) => vec![0x0F, 0xA1],
                    (false, _) => vec![0x0F, 0xA9],
                };
            }
            Operand::Mem(m) => {
                let size = m.size.ok_or("operation size not specified")?;
                self.osz(&mut i, size);
                i.op = vec![if push { 0xFF } else { 0x8F }];
                self.mem(&mut i, m, if push { 6 } else { 0 })?;
            }
            Operand::Imm { expr, size, .. } if push => {
                let v = self.val(expr)?;
                let osize = match size {
                    Some(16) => 16,
                    Some(32) => 32,
                    _ => self.bits,
                };
                self.osz(&mut i, osize);
                let fits = if *size == Some(8) {
                    Some(true)
                } else if v.known {
                    Some(fits_sbyte(v.v, osize))
                } else {
                    None
                };
                if !self.long(SLOT_IMM, fits) {
                    i.op = vec![0x6A];
                    i.imm = vec![v.v as u8];
                } else {
                    i.op = vec![0x68];
                    i.imm = self.imm_bytes(v.v, osize);
                }
            }
            _ => return Err("invalid operand".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn in_out(&mut self, mut i: Insn, is_in: bool, ops: &[Operand]) -> Result<(), String> {
        let [a, b] = ops else { return Err("in/out take two operands".into()) };
        let (acc, port) = if is_in { (a, b) } else { (b, a) };
        let Operand::Reg(Reg::Gpr(s, 0)) = acc else {
            return Err("in/out need AL, AX or EAX".into());
        };
        self.osz(&mut i, *s);
        let w = (*s != 8) as u8;
        match port {
            Operand::Imm { expr, .. } => {
                let v = self.value(expr)?;
                i.op = vec![if is_in { 0xE4 } else { 0xE6 } + w, v as u8];
            }
            Operand::Reg(Reg::Gpr(16, 2)) => i.op = vec![if is_in { 0xEC } else { 0xEE } + w],
            _ => return Err("port must be an immediate or DX".into()),
        }
        self.emit(&i);
        Ok(())
    }

    fn jump(&mut self, mut i: Insn, kind: Jump, ops: &[Operand]) -> Result<(), String> {
        let [a] = ops else { return Err("jump takes one operand".into()) };
        let here = self.env.here;
        match a {
            Operand::Imm { expr, dist, size } => {
                if *dist == Dist::Far {
                    return Err("far jumps need a SEG:OFFSET target".into());
                }
                let t = self.val(expr)?;
                let pre = i.prefix_len() as i64;
                let size = match size {
                    Some(s @ (16 | 32)) => *s,
                    _ => self.bits,
                };
                if size != self.bits {
                    i.o32 = true;
                }
                let pre = pre + (size != self.bits) as i64;
                let short_len = pre + 2;
                let rel_short = t.v - (here + short_len);
                let fits = if t.known { Some(fits_i8(rel_short)) } else { None };
                let can_short = !matches!(kind, Jump::Call);
                let must_short = matches!(kind, Jump::Loop(_)) || *dist == Dist::Short;
                let use_short = can_short
                    && (must_short || (*dist != Dist::Near && !self.long(SLOT_JMP, fits)));
                if use_short {
                    if self.last && t.known && !fits_i8(rel_short) {
                        return Err(format!("short jump out of range by {} bytes", rel_short.abs() - 127));
                    }
                    i.op = match kind {
                        Jump::Jmp => vec![0xEB],
                        Jump::Cond(c) => vec![0x70 + c],
                        Jump::Loop(op) => vec![op],
                        Jump::Call => unreachable!(),
                    };
                    i.imm = vec![rel_short as u8];
                } else {
                    i.op = match kind {
                        Jump::Jmp => vec![0xE9],
                        Jump::Call => vec![0xE8],
                        Jump::Cond(c) => vec![0x0F, 0x80 + c],
                        Jump::Loop(_) => unreachable!(),
                    };
                    let len = pre + i.op.len() as i64 + (size / 8) as i64;
                    let rel = t.v - (here + len);
                    i.imm = self.imm_bytes(rel, size);
                }
            }
            Operand::FarPtr { seg, off } => {
                let op = match kind {
                    Jump::Jmp => 0xEA,
                    Jump::Call => 0x9A,
                    _ => return Err("conditional jumps can't be far".into()),
                };
                let (s, o) = (self.value(seg)?, self.value(off)?);
                i.op = vec![op];
                i.imm = self.imm_bytes(o, self.bits);
                i.imm.extend_from_slice(&(s as u16).to_le_bytes());
            }
            Operand::Reg(Reg::Gpr(s, _)) | Operand::Mem(Mem { size: Some(s), far: false, .. })
                if matches!(kind, Jump::Jmp | Jump::Call) =>
            {
                self.osz(&mut i, *s);
                i.op = vec![0xFF];
                self.rm(&mut i, a, if matches!(kind, Jump::Jmp) { 4 } else { 2 })?;
            }
            Operand::Mem(m) if matches!(kind, Jump::Jmp | Jump::Call) => {
                if m.far {
                    if let Some(32) = m.size {
                        self.osz(&mut i, 32);
                    }
                    i.op = vec![0xFF];
                    self.mem(&mut i, m, if matches!(kind, Jump::Jmp) { 5 } else { 3 })?;
                } else {
                    // Default to the current bit size: jmp [bx]
                    i.op = vec![0xFF];
                    self.mem(&mut i, m, if matches!(kind, Jump::Jmp) { 4 } else { 2 })?;
                }
            }
            _ => return Err("invalid jump target".into()),
        }
        self.emit(&i);
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Jump {
    Jmp,
    Call,
    Cond(u8),
    Loop(u8),
}

fn first_unknown(e: &Expr, env: &PassEnv) -> Option<String> {
    use crate::expr::Env as _;
    match e {
        Expr::Sym(s) if env.symbol(s).is_none() => Some(s.clone()),
        Expr::Neg(a) | Expr::Not(a) | Expr::LogNot(a) => first_unknown(a, env),
        Expr::Bin(_, a, b) => first_unknown(a, env).or_else(|| first_unknown(b, env)),
        _ => None,
    }
}
