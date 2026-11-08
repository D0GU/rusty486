//! Instruction operand parsing.

use crate::expr::{self, Expr, Parser};
use crate::lexer::Tok;
use crate::regs::{self, Reg};

/// How a jump/immediate was qualified.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Dist {
    #[default]
    None,
    Short,
    Near,
    Far,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Mem {
    /// Explicit size in bits (byte/word/dword), if given.
    pub size: Option<u8>,
    /// Explicit segment override (0-5).
    pub seg: Option<u8>,
    /// Register terms (register, multiplier).
    pub regs: Vec<(Reg, i64)>,
    pub disp: Expr,
    /// `far [mem]` (indirect far jump/call).
    pub far: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Operand {
    Reg(Reg),
    Mem(Mem),
    Imm { expr: Expr, size: Option<u8>, dist: Dist },
    /// `seg:off` immediate far pointer.
    FarPtr { seg: Expr, off: Expr },
}

impl Operand {
    pub fn gpr(&self) -> Option<(u8, u8)> {
        match self {
            Operand::Reg(Reg::Gpr(s, n)) => Some((*s, *n)),
            _ => None,
        }
    }
}

fn size_keyword(s: &str) -> Option<u8> {
    match s.to_ascii_lowercase().as_str() {
        "byte" => Some(8),
        "word" => Some(16),
        "dword" => Some(32),
        "qword" => Some(64),
        _ => None,
    }
}

/// Parse one operand from its tokens.
pub fn parse(toks: &[Tok], scope: &str) -> Result<Operand, String> {
    let mut i = 0;
    let mut size = None;
    let mut dist = Dist::None;
    let mut strict = false;
    while let Some(Tok::Ident(w)) = toks.get(i) {
        if let Some(s) = size_keyword(w) {
            size = Some(s);
        } else {
            match w.to_ascii_lowercase().as_str() {
                "short" => dist = Dist::Short,
                // Like NASM, plain `near` is only a hint; `strict near` forces it.
                "near" if strict => dist = Dist::Near,
                "near" => {}
                "far" => dist = Dist::Far,
                "strict" => strict = true,
                _ => break,
            }
        }
        i += 1;
    }
    let rest = &toks[i..];
    if rest.is_empty() {
        return Err("missing operand".into());
    }

    // Segment prefix written outside the brackets: es:[di]
    let mut seg = None;
    let mut body = rest;
    if let (Some(Tok::Ident(s)), Some(t), Some(Tok::Punct("["))) = (rest.first(), rest.get(1), rest.get(2)) {
        if t.is(":") {
            if let Some(Reg::Seg(n)) = regs::reg(s) {
                seg = Some(n);
                body = &rest[2..];
            }
        }
    }

    if body[0].is("[") {
        if !body.last().is_some_and(|t| t.is("]")) {
            return Err("missing ']'".into());
        }
        let mut inner = &body[1..body.len() - 1];
        if let (Some(Tok::Ident(s)), Some(t)) = (inner.first(), inner.get(1)) {
            if t.is(":") {
                match regs::reg(s) {
                    Some(Reg::Seg(n)) => {
                        seg = Some(n);
                        inner = &inner[2..];
                    }
                    _ => return Err(format!("'{s}' is not a segment register")),
                }
            }
        }
        if inner.is_empty() {
            return Err("empty memory reference".into());
        }
        let mut p = Parser::new(inner, scope);
        p.allow_regs = true;
        let e = p.parse()?;
        if !p.at_end() {
            return Err("unexpected tokens in memory reference".into());
        }
        let (regs, disp) = expr::linearize(&e)?;
        return Ok(Operand::Mem(Mem { size, seg, regs, disp, far: dist == Dist::Far }));
    }

    if rest.len() == 1 {
        if let Tok::Ident(name) = &rest[0] {
            if let Some(r) = regs::reg(name) {
                return Ok(Operand::Reg(r));
            }
        }
    }

    let mut p = Parser::new(rest, scope);
    let e = p.parse()?;
    if p.peek().is_some_and(|t| t.is(":")) {
        p.pos += 1;
        let off = p.parse()?;
        if !p.at_end() {
            return Err("unexpected tokens after far pointer".into());
        }
        return Ok(Operand::FarPtr { seg: e, off });
    }
    if !p.at_end() {
        return Err("unexpected tokens after operand".into());
    }
    Ok(Operand::Imm { expr: e, size, dist })
}

/// Split operand tokens on top-level commas.
pub fn split(toks: &[Tok]) -> Vec<&[Tok]> {
    let mut out = Vec::new();
    let (mut depth, mut start) = (0i32, 0);
    for (i, t) in toks.iter().enumerate() {
        if t.is("[") || t.is("(") {
            depth += 1;
        } else if t.is("]") || t.is(")") {
            depth -= 1;
        } else if t.is(",") && depth == 0 {
            out.push(&toks[start..i]);
            start = i + 1;
        }
    }
    if start < toks.len() || !out.is_empty() {
        out.push(&toks[start..]);
    }
    out
}
