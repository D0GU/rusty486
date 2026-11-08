//! rusty486-asm: a NASM-syntax assembler for 16/32-bit x86 (8086 to 486),
//! producing flat binaries like `nasm -f bin`.
//!
//! Supported: labels (including `.local` labels), `equ`, `org`, `bits 16/32`,
//! `db/dw/dd/dq`, `resb/resw/resd`, `times`, `align`, `$`/`$$` expressions,
//! `%define`, `%macro` and `%rep`, and the integer instruction set of the 486.
//! Short/long forms (jumps, displacements, immediates) are optimised over
//! multiple passes, matching NASM's default `-Ox` output.

mod encode;
pub mod expr;
pub mod lexer;
mod operand;
mod preproc;
pub mod regs;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use expr::{Env, Expr, Parser};
use lexer::Tok;
use operand::Operand;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AsmError {
    /// 1-based source line.
    pub line: usize,
    pub message: String,
}

impl fmt::Display for AsmError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "line {}: {}", self.line, self.message)
    }
}

/// Where each source line's bytes ended up.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LineInfo {
    pub line: usize,
    /// Address (origin-relative `$`) of the first byte.
    pub addr: u32,
    pub len: u32,
}

#[derive(Clone, Debug, Default)]
pub struct Output {
    /// The `org` address of the first byte.
    pub origin: u32,
    pub bytes: Vec<u8>,
    pub lines: Vec<LineInfo>,
    pub symbols: BTreeMap<String, i64>,
}

/// Whether `word` is an instruction mnemonic (lower-case), for highlighting.
pub fn is_mnemonic(word: &str) -> bool {
    encode::is_mnemonic(word) || PREFIXES.contains(&word)
}

/// Whether `word` is an assembler directive (lower-case), for highlighting.
pub fn is_directive_word(word: &str) -> bool {
    is_directive(word) || matches!(word, "byte" | "word" | "dword" | "qword" | "short" | "near" | "far" | "strict")
}

/// Assemble NASM-syntax source into a flat binary.
pub fn assemble(src: &str) -> Result<Output, Vec<AsmError>> {
    let lines = preproc::preprocess(src)?;
    // Bad lines become empty statements so later errors are reported too.
    let (stmts, mut errors) = parse_all(&lines);
    match Assembler::new(stmts).run() {
        Ok(out) if errors.is_empty() => Ok(out),
        Ok(_) => Err(errors),
        Err(more) => {
            errors.extend(more);
            errors.sort_by_key(|e| e.line);
            Err(errors)
        }
    }
}

// ------------------------------------------------------------------ statements

#[derive(Clone, Debug)]
pub(crate) enum DataItem {
    Expr(Expr),
    Bytes(Vec<u8>),
    Uninit,
}

#[derive(Clone, Debug)]
pub(crate) enum Stmt {
    Empty,
    Equ(String, Expr),
    Org(Expr),
    Bits(Expr),
    Data { size: u8, items: Vec<DataItem> },
    Res { size: u8, count: Expr },
    Align { n: Expr, fill: u8 },
    Times { count: Expr, inner: Box<Stmt> },
    Insn { prefixes: Vec<String>, mnemonic: String, ops: Vec<Operand> },
}

pub(crate) struct Parsed {
    line: usize,
    label: Option<String>,
    stmt: Stmt,
}

const PREFIXES: [&str; 15] = [
    "rep", "repe", "repz", "repne", "repnz", "lock", "es", "cs", "ss", "ds", "fs", "gs", "o16", "o32", "a32",
];

fn is_directive(w: &str) -> bool {
    matches!(
        w,
        "db" | "dw" | "dd" | "dq" | "resb" | "resw" | "resd" | "resq" | "times" | "equ" | "org" | "bits"
            | "use16" | "use32" | "cpu" | "align" | "alignb" | "section" | "segment" | "global"
            | "extern" | "default"
    )
}

fn is_keyword(w: &str) -> bool {
    let l = w.to_ascii_lowercase();
    encode::is_mnemonic(&l) || is_directive(&l) || PREFIXES.contains(&l.as_str())
}

fn parse_all(lines: &[preproc::Line]) -> (Vec<Parsed>, Vec<AsmError>) {
    let mut out = Vec::new();
    let mut errors = Vec::new();
    let mut scope = String::new();
    for l in lines {
        match parse_line(&l.text, &mut scope) {
            Ok((label, stmt)) => out.push(Parsed { line: l.line, label, stmt }),
            Err(message) => {
                errors.push(AsmError { line: l.line, message });
                out.push(Parsed { line: l.line, label: None, stmt: Stmt::Empty });
            }
        }
    }
    (out, errors)
}

fn parse_line(text: &str, scope: &mut String) -> Result<(Option<String>, Stmt), String> {
    let mut toks = lexer::lex(text)?;
    // [bits 16] / [org 0x100] style directives.
    if toks.first().is_some_and(|t| t.is("["))
        && toks.last().is_some_and(|t| t.is("]"))
        && toks.get(1).and_then(Tok::ident).is_some_and(|w| is_directive(&w.to_ascii_lowercase()))
    {
        toks = toks[1..toks.len() - 1].to_vec();
    }
    let mut t: &[Tok] = &toks;
    let mut label = None;

    if let Some(Tok::Ident(w)) = t.first() {
        let colon = t.get(1).is_some_and(|x| x.is(":"));
        // Without a colon, a word is a label only if a directive or
        // instruction follows (or nothing), so typos aren't taken as labels.
        let bare_label = !is_keyword(w)
            && w != "$"
            && w != "$$"
            && t.get(1).is_none_or(|n| n.ident().is_some_and(is_keyword));
        if !colon && !is_keyword(w) && !bare_label && !regs::is_reg(w) {
            return Err(format!("unknown instruction '{w}'"));
        }
        if colon || bare_label {
            let name = if w.starts_with('.') && !w.starts_with("..") {
                format!("{scope}{w}")
            } else {
                w.clone()
            };
            if regs::is_reg(w) {
                return Err(format!("'{w}' is a register name, not a label"));
            }
            t = &t[if colon { 2 } else { 1 }..];
            // `name equ value` defines a constant, not a label.
            if t.first().and_then(Tok::ident).is_some_and(|w| w.eq_ignore_ascii_case("equ")) {
                let mut p = Parser::new(&t[1..], scope);
                let e = p.parse()?;
                if !p.at_end() {
                    return Err("unexpected tokens after equ".into());
                }
                return Ok((None, Stmt::Equ(name, e)));
            }
            if !w.starts_with('.') && !w.starts_with("..@") {
                *scope = w.clone();
            }
            label = Some(name);
        }
    }
    let stmt = parse_stmt(t, scope)?;
    Ok((label, stmt))
}

fn parse_expr_all(t: &[Tok], scope: &str) -> Result<Expr, String> {
    let mut p = Parser::new(t, scope);
    let e = p.parse()?;
    if !p.at_end() {
        return Err("unexpected tokens after expression".into());
    }
    Ok(e)
}

fn parse_stmt(t: &[Tok], scope: &str) -> Result<Stmt, String> {
    let Some(first) = t.first() else {
        return Ok(Stmt::Empty);
    };
    let Some(word) = first.ident() else {
        return Err("expected an instruction or directive".into());
    };
    let w = word.to_ascii_lowercase();
    let rest = &t[1..];
    let data_size = |w: &str| match w {
        "db" | "resb" => 8,
        "dw" | "resw" => 16,
        "dd" | "resd" => 32,
        _ => 64,
    };
    match w.as_str() {
        "org" => Ok(Stmt::Org(parse_expr_all(rest, scope)?)),
        "bits" => Ok(Stmt::Bits(parse_expr_all(rest, scope)?)),
        "use16" => Ok(Stmt::Bits(Expr::Num(16))),
        "use32" => Ok(Stmt::Bits(Expr::Num(32))),
        "cpu" | "section" | "segment" | "global" | "extern" | "default" => Ok(Stmt::Empty),
        "db" | "dw" | "dd" | "dq" => {
            let mut items = Vec::new();
            for part in operand::split(rest) {
                match part {
                    [Tok::Str(s)] => items.push(DataItem::Bytes(s.clone())),
                    [Tok::Ident(q)] if q == "?" => items.push(DataItem::Uninit),
                    [] => return Err("empty data item".into()),
                    _ => items.push(DataItem::Expr(parse_expr_all(part, scope)?)),
                }
            }
            Ok(Stmt::Data { size: data_size(&w), items })
        }
        "resb" | "resw" | "resd" | "resq" => {
            Ok(Stmt::Res { size: data_size(&w), count: parse_expr_all(rest, scope)? })
        }
        "align" | "alignb" => {
            let parts = operand::split(rest);
            let n = parse_expr_all(parts.first().ok_or("align needs a value")?, scope)?;
            Ok(Stmt::Align { n, fill: if w == "align" { 0x90 } else { 0 } })
        }
        "times" => {
            let mut p = Parser::new(rest, scope);
            let count = p.parse()?;
            let inner = parse_stmt(&rest[p.pos..], scope)?;
            Ok(Stmt::Times { count, inner: Box::new(inner) })
        }
        "equ" => Err("equ needs a label".into()),
        _ => {
            let mut prefixes = Vec::new();
            let mut i = 0;
            while let Some(Tok::Ident(p)) = t.get(i) {
                let l = p.to_ascii_lowercase();
                if PREFIXES.contains(&l.as_str()) && t.len() > i + 1 && !t[i + 1].is(":") {
                    prefixes.push(l);
                    i += 1;
                } else {
                    break;
                }
            }
            let Some(Tok::Ident(m)) = t.get(i) else {
                if !prefixes.is_empty() && t.len() == i {
                    // A bare prefix line, e.g. "rep" alone.
                    let m = prefixes.pop().unwrap();
                    return Ok(Stmt::Insn { prefixes, mnemonic: m, ops: vec![] });
                }
                return Err("expected an instruction".into());
            };
            let mnemonic = m.to_ascii_lowercase();
            if !encode::is_mnemonic(&mnemonic) {
                return Err(format!("unknown instruction '{m}'"));
            }
            let ops = operand::split(&t[i + 1..])
                .into_iter()
                .map(|o| operand::parse(o, scope))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(Stmt::Insn { prefixes, mnemonic, ops })
        }
    }
}

// ------------------------------------------------------------------ passes

/// Encoding choices that must use the long form, keyed by (statement, slot).
/// Choices only ever grow, so the passes converge.
pub(crate) type Grow = HashSet<(usize, u8)>;

struct Assembler {
    stmts: Vec<Parsed>,
}

pub(crate) struct PassEnv<'a> {
    pub cur: &'a HashMap<String, i64>,
    pub prev: &'a HashMap<String, i64>,
    pub here: i64,
    pub base: i64,
}

impl Env for PassEnv<'_> {
    fn symbol(&self, name: &str) -> Option<i64> {
        self.cur.get(name).or_else(|| self.prev.get(name)).copied()
    }
    fn here(&self) -> i64 {
        self.here
    }
    fn base(&self) -> i64 {
        self.base
    }
}

impl Assembler {
    fn new(stmts: Vec<Parsed>) -> Self {
        Assembler { stmts }
    }

    fn run(self) -> Result<Output, Vec<AsmError>> {
        let mut prev: HashMap<String, i64> = HashMap::new();
        let mut grow = Grow::new();
        for pass in 0..40 {
            let before = grow.len();
            let (syms, _, _) = self.pass(&prev, &mut grow, false);
            let stable = pass > 0 && syms == prev && grow.len() == before;
            prev = syms;
            if stable {
                break;
            }
        }
        let (symbols, out, errors) = self.pass(&prev, &mut grow, true);
        if errors.is_empty() {
            let mut out = out;
            out.symbols = symbols.into_iter().filter(|(k, _)| !k.starts_with("..@")).collect();
            Ok(out)
        } else {
            Err(errors)
        }
    }

    /// One assembly pass. In the final pass, unresolved symbols and range
    /// problems are errors.
    fn pass(
        &self,
        prev: &HashMap<String, i64>,
        grow: &mut Grow,
        last: bool,
    ) -> (HashMap<String, i64>, Output, Vec<AsmError>) {
        let mut syms: HashMap<String, i64> = HashMap::new();
        let mut out = Output::default();
        let mut errors = Vec::new();
        let mut origin: i64 = 0;
        let mut bits: u8 = 16;
        let mut started = false;

        for (idx, p) in self.stmts.iter().enumerate() {
            let here = origin + out.bytes.len() as i64;
            if let Some(l) = &p.label {
                if last && syms.contains_key(l) {
                    errors.push(AsmError { line: p.line, message: format!("label '{l}' redefined") });
                }
                syms.insert(l.clone(), here);
            }
            let start = out.bytes.len();
            if let Stmt::Equ(name, e) = &p.stmt {
                let r = expr::eval(e, &PassEnv { cur: &syms, prev, here, base: origin });
                match r {
                    Ok(v) if v.known => {
                        if last && syms.contains_key(name) {
                            errors.push(AsmError { line: p.line, message: format!("'{name}' redefined") });
                        }
                        syms.insert(name.clone(), v.v);
                    }
                    Ok(_) if last => errors.push(AsmError {
                        line: p.line,
                        message: format!("'{name}' depends on an undefined symbol"),
                    }),
                    Ok(_) => {}
                    Err(m) => errors.push(AsmError { line: p.line, message: m }),
                }
                continue;
            }
            let env = PassEnv { cur: &syms, prev, here, base: origin };
            let mut err = |m: String| errors.push(AsmError { line: p.line, message: m });
            match &p.stmt {
                Stmt::Empty | Stmt::Equ(..) => {}
                Stmt::Org(e) => match expr::eval(e, &env) {
                    Ok(v) if v.known => {
                        if started && !out.bytes.is_empty() {
                            err("org must come before any code or data".into());
                        }
                        origin = v.v;
                        out.origin = v.v as u32;
                    }
                    Ok(_) => err("org needs a constant".into()),
                    Err(m) => err(m),
                },
                Stmt::Bits(e) => match expr::eval(e, &env) {
                    Ok(v) if v.v == 16 || v.v == 32 => bits = v.v as u8,
                    Ok(_) => err("bits must be 16 or 32".into()),
                    Err(m) => err(m),
                },
                stmt => {
                    started = true;
                    let mut ctx = encode::Ctx { env: &env, bits, last, grow, stmt: idx, out: Vec::new() };
                    match emit(&mut ctx, stmt) {
                        Ok(()) => out.bytes.extend_from_slice(&ctx.out),
                        Err(m) => err(m),
                    }
                }
            }
            let len = out.bytes.len() - start;
            if len > 0 {
                out.lines.push(LineInfo { line: p.line, addr: (here as u32), len: len as u32 });
            }
        }
        (syms, out, errors)
    }
}

fn emit(ctx: &mut encode::Ctx, stmt: &Stmt) -> Result<(), String> {
    match stmt {
        Stmt::Data { size, items } => {
            let n = (*size / 8) as usize;
            for item in items {
                match item {
                    DataItem::Bytes(b) => {
                        ctx.out.extend_from_slice(b);
                        let pad = (n - b.len() % n) % n;
                        ctx.out.extend(std::iter::repeat_n(0, pad));
                    }
                    DataItem::Uninit => ctx.out.extend(std::iter::repeat_n(0, n)),
                    DataItem::Expr(e) => {
                        let v = ctx.value(e)?;
                        ctx.out.extend_from_slice(&v.to_le_bytes()[..n]);
                    }
                }
            }
            Ok(())
        }
        Stmt::Res { size, count } => {
            let c = ctx.const_value(count, "resb count")?;
            ctx.out.extend(std::iter::repeat_n(0, c.max(0) as usize * (*size / 8) as usize));
            Ok(())
        }
        Stmt::Align { n, fill } => {
            let n = ctx.const_value(n, "align value")?;
            if n <= 0 || n & (n - 1) != 0 {
                return Err("align value must be a power of two".into());
            }
            let here = ctx.env.here;
            let pad = (n - here.rem_euclid(n)) % n;
            ctx.out.extend(std::iter::repeat_n(*fill, pad as usize));
            Ok(())
        }
        Stmt::Times { count, inner } => {
            let c = ctx.const_value(count, "times count")?;
            if c < 0 {
                return Err("times count is negative".into());
            }
            if c > 16 * 1024 * 1024 {
                return Err("times count too large".into());
            }
            // Encode once and repeat (instructions inside `times` don't depend on `$`).
            let mut one = encode::Ctx { env: ctx.env, bits: ctx.bits, last: ctx.last, grow: ctx.grow, stmt: ctx.stmt, out: Vec::new() };
            emit(&mut one, inner)?;
            let bytes = one.out;
            for _ in 0..c {
                ctx.out.extend_from_slice(&bytes);
            }
            Ok(())
        }
        Stmt::Insn { prefixes, mnemonic, ops } => ctx.instruction(prefixes, mnemonic, ops),
        Stmt::Empty | Stmt::Equ(..) | Stmt::Org(_) | Stmt::Bits(_) => Ok(()),
    }
}
