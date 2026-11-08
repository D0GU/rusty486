//! Expression parsing and evaluation, NASM precedence:
//! `|` < `^` < `&` < `<< >>` < `+ -` < `* / // % %%` < unary.

use crate::lexer::Tok;
use crate::regs::{self, Reg};

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Num(i64),
    Sym(String),
    /// `$`: address of the current line.
    Here,
    /// `$$`: start of the section.
    Base,
    /// A register (only valid inside memory references).
    Reg(Reg),
    Neg(Box<Expr>),
    Not(Box<Expr>),
    LogNot(Box<Expr>),
    Bin(BinOp, Box<Expr>, Box<Expr>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Or,
    Xor,
    And,
    Shl,
    Shr,
    Add,
    Sub,
    Mul,
    Div,
    SDiv,
    Mod,
    SMod,
}

/// An evaluated value. `known` is false if it depends on a symbol that isn't
/// defined yet (forward reference in an early pass).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Val {
    pub v: i64,
    pub known: bool,
}

impl Val {
    pub fn known(v: i64) -> Self {
        Val { v, known: true }
    }
}

/// Evaluation environment.
pub trait Env {
    fn symbol(&self, name: &str) -> Option<i64>;
    fn here(&self) -> i64;
    fn base(&self) -> i64;
}

pub struct Parser<'a> {
    pub toks: &'a [Tok],
    pub pos: usize,
    /// Whether register names become `Expr::Reg` (inside `[...]`).
    pub allow_regs: bool,
    /// Prefix for local labels (`.name` -> `global.name`).
    pub scope: &'a str,
}

impl<'a> Parser<'a> {
    pub fn new(toks: &'a [Tok], scope: &'a str) -> Self {
        Parser { toks, pos: 0, allow_regs: false, scope }
    }

    pub fn peek(&self) -> Option<&'a Tok> {
        self.toks.get(self.pos)
    }

    pub fn at_end(&self) -> bool {
        self.pos >= self.toks.len()
    }

    fn eat(&mut self, p: &str) -> bool {
        if self.peek().is_some_and(|t| t.is(p)) {
            self.pos += 1;
            true
        } else {
            false
        }
    }

    pub fn parse(&mut self) -> Result<Expr, String> {
        self.binary(0)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, String> {
        const LEVELS: [&[(&str, BinOp)]; 6] = [
            &[("|", BinOp::Or)],
            &[("^", BinOp::Xor)],
            &[("&", BinOp::And)],
            &[("<<", BinOp::Shl), (">>", BinOp::Shr)],
            &[("+", BinOp::Add), ("-", BinOp::Sub)],
            &[
                ("*", BinOp::Mul),
                ("/", BinOp::Div),
                ("//", BinOp::SDiv),
                ("%", BinOp::Mod),
                ("%%", BinOp::SMod),
            ],
        ];
        if level == LEVELS.len() {
            return self.unary();
        }
        let mut lhs = self.binary(level + 1)?;
        'outer: loop {
            for (p, op) in LEVELS[level] {
                if self.eat(p) {
                    let rhs = self.binary(level + 1)?;
                    lhs = Expr::Bin(*op, Box::new(lhs), Box::new(rhs));
                    continue 'outer;
                }
            }
            return Ok(lhs);
        }
    }

    fn unary(&mut self) -> Result<Expr, String> {
        if self.eat("-") {
            return Ok(Expr::Neg(Box::new(self.unary()?)));
        }
        if self.eat("+") {
            return self.unary();
        }
        if self.eat("~") {
            return Ok(Expr::Not(Box::new(self.unary()?)));
        }
        if self.eat("!") {
            return Ok(Expr::LogNot(Box::new(self.unary()?)));
        }
        self.primary()
    }

    fn primary(&mut self) -> Result<Expr, String> {
        let tok = self.peek().ok_or("expected an expression")?;
        self.pos += 1;
        match tok {
            Tok::Num(n) => Ok(Expr::Num(*n)),
            Tok::Str(s) => {
                if s.len() > 8 {
                    return Err("string constant too long for a number".into());
                }
                let mut v: i64 = 0;
                for (i, b) in s.iter().enumerate() {
                    v |= (*b as i64) << (8 * i);
                }
                Ok(Expr::Num(v))
            }
            Tok::Punct("(") => {
                let e = self.parse()?;
                if !self.eat(")") {
                    return Err("missing ')'".into());
                }
                Ok(e)
            }
            Tok::Ident(name) => {
                if name == "$" {
                    return Ok(Expr::Here);
                }
                if name == "$$" {
                    return Ok(Expr::Base);
                }
                if let Some(r) = regs::reg(name) {
                    if self.allow_regs {
                        return Ok(Expr::Reg(r));
                    }
                    return Err(format!("register '{name}' not allowed here"));
                }
                Ok(Expr::Sym(qualify(name, self.scope)))
            }
            Tok::Punct(p) => Err(format!("unexpected '{p}'")),
        }
    }
}

/// Resolve a local label (`.loop`) against the current global label.
pub fn qualify(name: &str, scope: &str) -> String {
    if name.starts_with('.') && !name.starts_with("..") {
        format!("{scope}{name}")
    } else {
        name.to_string()
    }
}

pub fn eval(e: &Expr, env: &dyn Env) -> Result<Val, String> {
    Ok(match e {
        Expr::Num(n) => Val::known(*n),
        Expr::Here => Val::known(env.here()),
        Expr::Base => Val::known(env.base()),
        Expr::Sym(s) => match env.symbol(s) {
            Some(v) => Val::known(v),
            None => Val { v: 0, known: false },
        },
        Expr::Reg(_) => return Err("register in an expression".into()),
        Expr::Neg(a) => map1(eval(a, env)?, |v| v.wrapping_neg()),
        Expr::Not(a) => map1(eval(a, env)?, |v| !v),
        Expr::LogNot(a) => map1(eval(a, env)?, |v| (v == 0) as i64),
        Expr::Bin(op, a, b) => {
            let (a, b) = (eval(a, env)?, eval(b, env)?);
            let known = a.known && b.known;
            let (x, y) = (a.v, b.v);
            let v = match op {
                BinOp::Or => x | y,
                BinOp::Xor => x ^ y,
                BinOp::And => x & y,
                BinOp::Shl => x.wrapping_shl(y as u32),
                BinOp::Shr => ((x as u64) >> (y as u32 & 63)) as i64,
                BinOp::Add => x.wrapping_add(y),
                BinOp::Sub => x.wrapping_sub(y),
                BinOp::Mul => x.wrapping_mul(y),
                BinOp::Div | BinOp::SDiv | BinOp::Mod | BinOp::SMod if y == 0 => {
                    if known {
                        return Err("division by zero".into());
                    }
                    0
                }
                BinOp::Div => ((x as u64) / (y as u64)) as i64,
                BinOp::SDiv => x.wrapping_div(y),
                BinOp::Mod => ((x as u64) % (y as u64)) as i64,
                BinOp::SMod => x.wrapping_rem(y),
            };
            Val { v, known }
        }
    })
}

fn map1(a: Val, f: impl Fn(i64) -> i64) -> Val {
    Val { v: f(a.v), known: a.known }
}

/// Whether an expression refers to any symbol (or `$`/`$$`).
pub fn has_symbols(e: &Expr) -> bool {
    match e {
        Expr::Sym(_) | Expr::Here | Expr::Base => true,
        Expr::Neg(a) | Expr::Not(a) | Expr::LogNot(a) => has_symbols(a),
        Expr::Bin(_, a, b) => has_symbols(a) || has_symbols(b),
        _ => false,
    }
}

/// Whether an expression mentions a register.
pub fn has_reg(e: &Expr) -> bool {
    match e {
        Expr::Reg(_) => true,
        Expr::Neg(a) | Expr::Not(a) | Expr::LogNot(a) => has_reg(a),
        Expr::Bin(_, a, b) => has_reg(a) || has_reg(b),
        _ => false,
    }
}

/// Split a memory expression into register terms (register, multiplier) and
/// a displacement expression.
pub fn linearize(e: &Expr) -> Result<(Vec<(Reg, i64)>, Expr), String> {
    match e {
        Expr::Reg(r) => Ok((vec![(*r, 1)], Expr::Num(0))),
        Expr::Bin(BinOp::Add, a, b) => {
            let (mut ra, da) = linearize(a)?;
            let (rb, db) = linearize(b)?;
            ra.extend(rb);
            Ok((ra, Expr::Bin(BinOp::Add, Box::new(da), Box::new(db))))
        }
        Expr::Bin(BinOp::Sub, a, b) => {
            if has_reg(b) {
                return Err("cannot subtract a register in an address".into());
            }
            let (ra, da) = linearize(a)?;
            Ok((ra, Expr::Bin(BinOp::Sub, Box::new(da), b.clone())))
        }
        Expr::Bin(BinOp::Mul, a, b) => {
            let (reg_side, k_side) = match (has_reg(a), has_reg(b)) {
                (true, false) => (a, b),
                (false, true) => (b, a),
                (false, false) => return Ok((vec![], e.clone())),
                _ => return Err("cannot multiply two registers".into()),
            };
            let k = const_eval(k_side).ok_or("register scale must be a constant")?;
            let (regs, disp) = linearize(reg_side)?;
            if !matches!(disp, Expr::Num(0)) && const_eval(&disp) != Some(0) {
                return Err("unsupported address expression".into());
            }
            Ok((regs.into_iter().map(|(r, s)| (r, s * k)).collect(), Expr::Num(0)))
        }
        e if has_reg(e) => Err("unsupported address expression".into()),
        e => Ok((vec![], e.clone())),
    }
}

/// Evaluate an expression made only of numbers.
fn const_eval(e: &Expr) -> Option<i64> {
    struct NoEnv;
    impl Env for NoEnv {
        fn symbol(&self, _: &str) -> Option<i64> {
            None
        }
        fn here(&self) -> i64 {
            0
        }
        fn base(&self) -> i64 {
            0
        }
    }
    eval(e, &NoEnv).ok().filter(|v| v.known).map(|v| v.v)
}
