//! Tokens for R++.

use alloc::string::String;
use alloc::vec::Vec;

use crate::Error;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok<'a> {
    Ident(&'a str),
    /// Value and optional type suffix (`5u8`).
    Int(u64, Option<String>),
    Str(Vec<u8>),
    ByteStr(Vec<u8>),
    Char(u32),
    Byte(u8),
    Punct(&'static str),
    /// `'a` (ignored by the parser: R++ has no borrow checker).
    Lifetime,
    Eof,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Span {
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub struct Token<'a> {
    pub tok: Tok<'a>,
    pub span: Span,
}

const PUNCTS: &[&str] = &[
    "..=", "<<=", ">>=", "::", "->", "=>", "==", "!=", "<=", ">=", "&&", "||", "+=", "-=", "*=", "/=", "%=", "^=",
    "&=", "|=", "<<", ">>", "..", "+", "-", "*", "/", "%", "^", "!", "&", "|", "=", "<", ">", "@", ".", ",", ";",
    ":", "#", "$", "?", "{", "}", "[", "]", "(", ")",
];

struct Cursor<'a> {
    b: &'a [u8],
    i: usize,
    line: u32,
    col: u32,
}

impl Cursor<'_> {
    fn adv(&mut self, n: usize) {
        for _ in 0..n {
            if self.i >= self.b.len() {
                return;
            }
            if self.b[self.i] == b'\n' {
                self.line += 1;
                self.col = 1;
            } else {
                self.col += 1;
            }
            self.i += 1;
        }
    }
    fn peek(&self, k: usize) -> Option<u8> {
        self.b.get(self.i + k).copied()
    }
    fn span(&self) -> Span {
        Span { line: self.line, col: self.col }
    }
}

pub fn lex(src: &str) -> Result<Vec<Token<'_>>, Error> {
    let mut c = Cursor { b: src.as_bytes(), i: 0, line: 1, col: 1 };
    // Roughly one token per 4 bytes of source: reserve once rather than
    // doubling (freed doublings fragment the IDE's heap).
    let mut out = Vec::with_capacity(src.len() / 4 + 16);
    while let Some(ch) = c.peek(0) {
        let span = c.span();
        let err = |m: &str| Error::new(span, m);
        if ch.is_ascii_whitespace() {
            c.adv(1);
            continue;
        }
        if ch == b'/' && c.peek(1) == Some(b'/') {
            while c.peek(0).is_some_and(|x| x != b'\n') {
                c.adv(1);
            }
            continue;
        }
        if ch == b'/' && c.peek(1) == Some(b'*') {
            let mut depth = 0;
            loop {
                match (c.peek(0), c.peek(1)) {
                    (Some(b'/'), Some(b'*')) => {
                        depth += 1;
                        c.adv(2);
                    }
                    (Some(b'*'), Some(b'/')) => {
                        depth -= 1;
                        c.adv(2);
                        if depth == 0 {
                            break;
                        }
                    }
                    (Some(_), _) => c.adv(1),
                    (None, _) => return Err(err("unterminated block comment")),
                }
            }
            continue;
        }
        if ch.is_ascii_digit() {
            let radix = match (ch, c.peek(1)) {
                (b'0', Some(b'x')) => 16,
                (b'0', Some(b'b')) => 2,
                (b'0', Some(b'o')) => 8,
                _ => 10,
            };
            if radix != 10 {
                c.adv(2);
            }
            let mut v: u64 = 0;
            let mut any = false;
            while let Some(d) = c.peek(0) {
                if d == b'_' {
                    c.adv(1);
                    continue;
                }
                let dv = match d {
                    b'0'..=b'9' => (d - b'0') as u64,
                    b'a'..=b'f' => (d - b'a' + 10) as u64,
                    b'A'..=b'F' => (d - b'A' + 10) as u64,
                    _ => break,
                };
                if dv >= radix {
                    break;
                }
                v = v.wrapping_mul(radix).wrapping_add(dv);
                any = true;
                c.adv(1);
            }
            if !any {
                return Err(err("malformed number"));
            }
            let mut suffix = None;
            if matches!(c.peek(0), Some(b'i' | b'u')) {
                let s = c.i;
                while c.peek(0).is_some_and(|x| x.is_ascii_alphanumeric()) {
                    c.adv(1);
                }
                suffix = Some(String::from(&src[s..c.i]));
            }
            out.push(Token { tok: Tok::Int(v, suffix), span });
            continue;
        }
        if ch == b'b' && c.peek(1) == Some(b'"') {
            c.adv(1);
            let s = string_lit(&mut c).map_err(err)?;
            out.push(Token { tok: Tok::ByteStr(s), span });
            continue;
        }
        if ch == b'b' && c.peek(1) == Some(b'\'') {
            c.adv(2);
            let (v, n) = char_body(&c.b[c.i..]).map_err(err)?;
            c.adv(n);
            if v > 255 {
                return Err(err("byte literal out of range"));
            }
            out.push(Token { tok: Tok::Byte(v as u8), span });
            continue;
        }
        if ch.is_ascii_alphabetic() || ch == b'_' {
            let s = c.i;
            while c.peek(0).is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_') {
                c.adv(1);
            }
            out.push(Token { tok: Tok::Ident(&src[s..c.i]), span });
            continue;
        }
        if ch == b'"' {
            let s = string_lit(&mut c).map_err(err)?;
            out.push(Token { tok: Tok::Str(s), span });
            continue;
        }
        if ch == b'\''
            && c.peek(1).is_some_and(|x| x.is_ascii_alphabetic() || x == b'_')
            && c.peek(2) != Some(b'\'')
        {
            c.adv(1);
            while c.peek(0).is_some_and(|x| x.is_ascii_alphanumeric() || x == b'_') {
                c.adv(1);
            }
            out.push(Token { tok: Tok::Lifetime, span });
            continue;
        }
        if ch == b'\'' {
            c.adv(1);
            let (v, n) = char_body(&c.b[c.i..]).map_err(err)?;
            c.adv(n);
            out.push(Token { tok: Tok::Char(v), span });
            continue;
        }
        if ch >= 0x80 {
            return Err(err("non-ASCII character outside a string"));
        }
        match PUNCTS.iter().find(|p| c.b[c.i..].starts_with(p.as_bytes())) {
            Some(p) => {
                c.adv(p.len());
                out.push(Token { tok: Tok::Punct(p), span });
            }
            None => return Err(err("unexpected character")),
        }
    }
    out.push(Token { tok: Tok::Eof, span: c.span() });
    Ok(out)
}

/// An escape after the backslash: value and bytes consumed.
fn escape(b: &[u8]) -> Result<(u32, usize), &'static str> {
    Ok(match b.first() {
        Some(b'n') => (10, 1),
        Some(b'r') => (13, 1),
        Some(b't') => (9, 1),
        Some(b'0') => (0, 1),
        Some(b'\\') => (b'\\' as u32, 1),
        Some(b'\'') => (b'\'' as u32, 1),
        Some(b'"') => (b'"' as u32, 1),
        Some(b'x') => {
            let h = b.get(1..3).ok_or("bad \\x escape")?;
            let s = core::str::from_utf8(h).map_err(|_| "bad \\x escape")?;
            (u32::from_str_radix(s, 16).map_err(|_| "bad \\x escape")?, 3)
        }
        _ => return Err("unknown escape"),
    })
}

/// Body of a char literal after the opening quote: value and bytes consumed
/// (including the closing quote).
fn char_body(b: &[u8]) -> Result<(u32, usize), &'static str> {
    let (v, n) = match b.first() {
        Some(b'\\') => {
            let (v, n) = escape(&b[1..])?;
            (v, n + 1)
        }
        Some(&c) if c < 0x80 => (c as u32, 1),
        Some(_) => {
            let end = b.len().min(4);
            let s = match core::str::from_utf8(&b[..end]) {
                Ok(s) => s,
                Err(e) => core::str::from_utf8(&b[..e.valid_up_to()]).unwrap_or(""),
            };
            let ch = s.chars().next().ok_or("bad character literal")?;
            (ch as u32, ch.len_utf8())
        }
        None => return Err("unterminated character literal"),
    };
    if b.get(n) != Some(&b'\'') {
        return Err("unterminated character literal");
    }
    Ok((v, n + 1))
}

fn string_lit(c: &mut Cursor) -> Result<Vec<u8>, &'static str> {
    let mut out = Vec::new();
    c.adv(1);
    loop {
        match c.peek(0) {
            None => return Err("unterminated string"),
            Some(b'"') => {
                c.adv(1);
                return Ok(out);
            }
            Some(b'\\') => {
                // Backslash-newline skips the line break and leading spaces.
                if c.peek(1) == Some(b'\n') || (c.peek(1) == Some(b'\r') && c.peek(2) == Some(b'\n')) {
                    c.adv(1);
                    while c.peek(0).is_some_and(|x| x.is_ascii_whitespace()) {
                        c.adv(1);
                    }
                    continue;
                }
                let (v, n) = escape(&c.b[c.i + 1..])?;
                if v < 0x80 {
                    out.push(v as u8);
                } else {
                    let mut buf = [0u8; 4];
                    let ch = char::from_u32(v).ok_or("bad escape")?;
                    out.extend_from_slice(ch.encode_utf8(&mut buf).as_bytes());
                }
                c.adv(n + 1);
            }
            Some(b'\r') => c.adv(1),
            Some(x) => {
                out.push(x);
                c.adv(1);
            }
        }
    }
}
