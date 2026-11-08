//! Tokenizer for one line of NASM-syntax source.

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Tok {
    /// Identifier, keyword, register or `$` / `$$`.
    Ident(String),
    Num(i64),
    Str(Vec<u8>),
    Punct(&'static str),
}

impl Tok {
    pub fn is(&self, p: &str) -> bool {
        matches!(self, Tok::Punct(q) if *q == p)
    }

    pub fn ident(&self) -> Option<&str> {
        match self {
            Tok::Ident(s) => Some(s),
            _ => None,
        }
    }
}

const PUNCT2: [&str; 4] = ["<<", ">>", "//", "%%"];
const PUNCT1: [&str; 16] = ["+", "-", "*", "/", "%", "(", ")", "[", "]", ",", ":", "&", "|", "^", "~", "!"];

fn ident_start(c: char) -> bool {
    c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '?' | '@')
}

fn ident_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '#' | '@' | '~' | '?')
}

/// Tokenize a line, stopping at a `;` comment.
pub fn lex(line: &str) -> Result<Vec<Tok>, String> {
    let chars: Vec<char> = line.chars().collect();
    let mut toks = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c == ';' {
            break;
        } else if c == '\'' || c == '"' || c == '`' {
            let (bytes, next) = lex_string(&chars, i)?;
            toks.push(Tok::Str(bytes));
            i = next;
        } else if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            let text: String = chars[start..i].iter().collect();
            toks.push(Tok::Num(parse_number(&text)?));
        } else if c == '$' {
            if chars.get(i + 1) == Some(&'$') {
                toks.push(Tok::Ident("$$".into()));
                i += 2;
            } else if chars.get(i + 1).is_some_and(|c| c.is_ascii_digit()) {
                // $1F: old-style hex
                let start = i + 1;
                i += 1;
                while i < chars.len() && (chars[i].is_ascii_hexdigit() || chars[i] == '_') {
                    i += 1;
                }
                let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
                let v = i64::from_str_radix(&text, 16).map_err(|_| format!("bad number '${text}'"))?;
                toks.push(Tok::Num(v));
            } else if chars.get(i + 1).is_some_and(|c| ident_start(*c)) {
                // $name: identifier escaped from keyword interpretation
                let start = i + 1;
                i += 1;
                while i < chars.len() && ident_char(chars[i]) {
                    i += 1;
                }
                toks.push(Tok::Ident(chars[start..i].iter().collect()));
            } else {
                toks.push(Tok::Ident("$".into()));
                i += 1;
            }
        } else if ident_start(c) {
            let start = i;
            while i < chars.len() && ident_char(chars[i]) {
                i += 1;
            }
            toks.push(Tok::Ident(chars[start..i].iter().collect()));
        } else {
            let two: String = chars[i..(i + 2).min(chars.len())].iter().collect();
            if let Some(p) = PUNCT2.iter().find(|p| **p == two) {
                toks.push(Tok::Punct(p));
                i += 2;
            } else if let Some(p) = PUNCT1.iter().find(|p| p.starts_with(c)) {
                toks.push(Tok::Punct(p));
                i += 1;
            } else {
                return Err(format!("unexpected character '{c}'"));
            }
        }
    }
    Ok(toks)
}

fn lex_string(chars: &[char], start: usize) -> Result<(Vec<u8>, usize), String> {
    let quote = chars[start];
    let mut out = Vec::new();
    let mut i = start + 1;
    loop {
        let Some(&c) = chars.get(i) else {
            return Err("unterminated string".into());
        };
        i += 1;
        if c == quote {
            return Ok((out, i));
        }
        if quote == '`' && c == '\\' {
            let e = *chars.get(i).ok_or("unterminated string")?;
            i += 1;
            let b = match e {
                'n' => b'\n',
                'r' => b'\r',
                't' => b'\t',
                '0' => 0,
                'e' => 27,
                'a' => 7,
                'b' => 8,
                'x' => {
                    let hex: String = chars[i..(i + 2).min(chars.len())].iter().collect();
                    i += hex.len();
                    u8::from_str_radix(&hex, 16).map_err(|_| "bad \\x escape")?
                }
                other => other as u8,
            };
            out.push(b);
            continue;
        }
        let mut buf = [0u8; 4];
        out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
    }
}

/// Parse a NASM numeric literal: decimal, 0x/0h hex, h-suffix hex, 0b/b-suffix
/// binary, 0o/0q/o/q-suffix octal, with `_` separators.
pub fn parse_number(text: &str) -> Result<i64, String> {
    let t: String = text.chars().filter(|c| *c != '_').collect::<String>().to_ascii_lowercase();
    let bad = || format!("bad number '{text}'");
    let radix = |s: &str, r: u32| i64::from_str_radix(s, r).map_err(|_| bad());
    if let Some(rest) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0h")) {
        return radix(rest, 16);
    }
    if let Some(rest) = t.strip_prefix("0b").or_else(|| t.strip_prefix("0y")) {
        return radix(rest, 2);
    }
    if let Some(rest) = t.strip_prefix("0o").or_else(|| t.strip_prefix("0q")) {
        return radix(rest, 8);
    }
    if let Some(rest) = t.strip_prefix("0d").or_else(|| t.strip_prefix("0t")) {
        return radix(rest, 10);
    }
    if let Some(rest) = t.strip_suffix('h').or_else(|| t.strip_suffix('x')) {
        return radix(rest, 16);
    }
    if let Some(rest) = t.strip_suffix('q').or_else(|| t.strip_suffix('o')) {
        return radix(rest, 8);
    }
    if t.chars().all(|c| c.is_ascii_digit()) {
        return radix(&t, 10);
    }
    if let Some(rest) = t.strip_suffix('b').or_else(|| t.strip_suffix('y')) {
        if rest.chars().all(|c| c == '0' || c == '1') {
            return radix(rest, 2);
        }
    }
    if let Some(rest) = t.strip_suffix('d').or_else(|| t.strip_suffix('t')) {
        return radix(rest, 10);
    }
    Err(bad())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers() {
        assert_eq!(parse_number("0x1F"), Ok(31));
        assert_eq!(parse_number("1Fh"), Ok(31));
        assert_eq!(parse_number("0FFh"), Ok(255));
        assert_eq!(parse_number("101b"), Ok(5));
        assert_eq!(parse_number("0b101"), Ok(5));
        assert_eq!(parse_number("17q"), Ok(15));
        assert_eq!(parse_number("1_000"), Ok(1000));
        assert_eq!(parse_number("0ABh"), Ok(0xAB));
    }

    #[test]
    fn tokens() {
        let t = lex("mov ax, [es:di+2] ; hi").unwrap();
        assert_eq!(t.len(), 10);
        let t = lex("times 510-($-$$) db 0").unwrap();
        assert!(t.contains(&Tok::Ident("$$".into())));
        assert_eq!(lex("db 'a,b', 0").unwrap()[1], Tok::Str(b"a,b".to_vec()));
    }
}
