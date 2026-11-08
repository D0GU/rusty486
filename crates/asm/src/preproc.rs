//! A small NASM-style preprocessor: `%define`, `%undef`, `%macro`/`%endmacro`
//! (with `%1`..`%9`, `%0` and `%%local` labels) and `%rep`/`%endrep`.

use std::collections::HashMap;

use crate::AsmError;

/// A source line after preprocessing, tagged with its original line number.
#[derive(Clone, Debug)]
pub struct Line {
    pub line: usize,
    pub text: String,
}

struct Macro {
    min: usize,
    max: usize,
    body: Vec<String>,
}

pub fn preprocess(src: &str) -> Result<Vec<Line>, Vec<AsmError>> {
    let mut pp = Pre { defines: HashMap::new(), macros: HashMap::new(), out: Vec::new(), errors: Vec::new(), uniq: 0 };
    let lines: Vec<(usize, String)> =
        src.lines().enumerate().map(|(i, l)| (i + 1, l.to_string())).collect();
    pp.run(&lines, 0);
    if pp.errors.is_empty() {
        Ok(pp.out)
    } else {
        Err(pp.errors)
    }
}

struct Pre {
    defines: HashMap<String, String>,
    macros: HashMap<String, Macro>,
    out: Vec<Line>,
    errors: Vec<AsmError>,
    uniq: usize,
}

/// Split off the first whitespace-delimited word.
fn first_word(s: &str) -> (&str, &str) {
    let s = s.trim_start();
    let end = s.find(char::is_whitespace).unwrap_or(s.len());
    (&s[..end], s[end..].trim_start())
}

/// Remove a trailing `;` comment (outside strings).
fn strip_comment(s: &str) -> &str {
    let mut quote = None;
    for (i, c) in s.char_indices() {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '\'' || c == '"' || c == '`' => quote = Some(c),
            None if c == ';' => return &s[..i],
            None => {}
        }
    }
    s
}

/// Split macro arguments on top-level commas.
fn split_args(s: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut depth, mut quote) = (0i32, None);
    for c in s.chars() {
        match quote {
            Some(q) => {
                if c == q {
                    quote = None;
                }
                cur.push(c);
            }
            None => match c {
                '\'' | '"' | '`' => {
                    quote = Some(c);
                    cur.push(c);
                }
                '(' | '[' | '{' => {
                    depth += 1;
                    cur.push(c);
                }
                ')' | ']' | '}' => {
                    depth -= 1;
                    cur.push(c);
                }
                ',' if depth == 0 => out.push(std::mem::take(&mut cur).trim().to_string()),
                _ => cur.push(c),
            },
        }
    }
    if !cur.trim().is_empty() || !out.is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

impl Pre {
    fn err(&mut self, line: usize, msg: impl Into<String>) {
        self.errors.push(AsmError { line, message: msg.into() });
    }

    fn run(&mut self, lines: &[(usize, String)], depth: usize) {
        if depth > 32 {
            if let Some((l, _)) = lines.first() {
                self.err(*l, "macro expansion too deep");
            }
            return;
        }
        let mut i = 0;
        while i < lines.len() {
            let (ln, raw) = (&lines[i].0, &lines[i].1);
            let ln = *ln;
            i += 1;
            let code = strip_comment(raw);
            let (word, rest) = first_word(code);
            let lower = word.to_ascii_lowercase();
            match lower.as_str() {
                "%define" | "%idefine" | "%xdefine" => {
                    let (name, value) = first_word(rest);
                    if name.is_empty() {
                        self.err(ln, "%define needs a name");
                    } else {
                        let value = self.substitute(value.trim());
                        self.defines.insert(name.to_string(), value);
                    }
                }
                "%undef" => {
                    self.defines.remove(first_word(rest).0);
                }
                "%macro" | "%imacro" => {
                    let (name, params) = first_word(rest);
                    let (min, max) = match params.split_once('-') {
                        Some((a, b)) => (a.trim().parse().unwrap_or(0), b.trim().parse().unwrap_or(9)),
                        None => {
                            let n = params.trim().parse().unwrap_or(0);
                            (n, n)
                        }
                    };
                    let mut body = Vec::new();
                    let mut closed = false;
                    while i < lines.len() {
                        let l = &lines[i].1;
                        i += 1;
                        if first_word(strip_comment(l)).0.eq_ignore_ascii_case("%endmacro") {
                            closed = true;
                            break;
                        }
                        body.push(l.clone());
                    }
                    if !closed {
                        self.err(ln, "%macro without %endmacro");
                    }
                    self.macros.insert(name.to_ascii_lowercase(), Macro { min, max, body });
                }
                "%rep" => {
                    let count_src = self.substitute(rest.trim());
                    let mut body = Vec::new();
                    let mut nest = 0;
                    let mut closed = false;
                    while i < lines.len() {
                        let w = first_word(strip_comment(&lines[i].1)).0.to_ascii_lowercase();
                        if w == "%rep" {
                            nest += 1;
                        } else if w == "%endrep" {
                            if nest == 0 {
                                i += 1;
                                closed = true;
                                break;
                            }
                            nest -= 1;
                        }
                        body.push(lines[i].clone());
                        i += 1;
                    }
                    if !closed {
                        self.err(ln, "%rep without %endrep");
                        continue;
                    }
                    match crate::lexer::parse_number(count_src.trim()) {
                        Ok(n) if (0..=100_000).contains(&n) => {
                            for _ in 0..n {
                                self.run(&body, depth + 1);
                            }
                        }
                        _ => self.err(ln, "%rep count must be a number"),
                    }
                }
                "%endmacro" | "%endrep" => self.err(ln, format!("{word} without a matching start")),
                w if w.starts_with('%') => {
                    self.err(ln, format!("unsupported preprocessor directive '{word}'"))
                }
                _ => {
                    let text = self.substitute(raw);
                    if !self.expand_macro(ln, &text, depth) {
                        self.out.push(Line { line: ln, text });
                    }
                }
            }
        }
    }

    /// Expand a macro invocation on this line, if there is one.
    fn expand_macro(&mut self, ln: usize, text: &str, depth: usize) -> bool {
        let code = strip_comment(text);
        let (mut word, mut rest) = first_word(code);
        let mut label = None;
        // Allow "label: macro args".
        if let Some(l) = word.strip_suffix(':') {
            label = Some(l.to_string());
            (word, rest) = first_word(rest);
        }
        let Some(m) = self.macros.get(&word.to_ascii_lowercase()) else {
            return false;
        };
        let args = split_args(rest);
        if args.len() < m.min || args.len() > m.max {
            self.err(ln, format!("macro '{word}' expects {}..{} arguments, got {}", m.min, m.max, args.len()));
            return true;
        }
        self.uniq += 1;
        let id = self.uniq;
        let mut body: Vec<(usize, String)> = Vec::new();
        if let Some(l) = label {
            body.push((ln, format!("{l}:")));
        }
        for b in &m.body {
            let mut s = b.replace("%%", &format!("..@{id}."));
            s = s.replace("%0", &args.len().to_string());
            for (k, a) in args.iter().enumerate().rev() {
                s = s.replace(&format!("%{}", k + 1), a);
            }
            body.push((ln, s));
        }
        self.run(&body, depth + 1);
        true
    }

    /// Replace `%define`d names (whole identifiers outside strings/comments).
    fn substitute(&self, s: &str) -> String {
        if self.defines.is_empty() {
            return s.to_string();
        }
        let mut cur = s.to_string();
        for _ in 0..16 {
            let next = self.substitute_once(&cur);
            if next == cur {
                break;
            }
            cur = next;
        }
        cur
    }

    fn substitute_once(&self, s: &str) -> String {
        let chars: Vec<char> = s.chars().collect();
        let mut out = String::new();
        let mut i = 0;
        let ident = |c: char| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '$' | '#' | '@' | '~' | '?');
        while i < chars.len() {
            let c = chars[i];
            if c == ';' {
                out.extend(&chars[i..]);
                break;
            }
            if c == '\'' || c == '"' || c == '`' {
                let start = i;
                i += 1;
                while i < chars.len() && chars[i] != c {
                    i += 1;
                }
                i = (i + 1).min(chars.len());
                out.extend(&chars[start..i]);
                continue;
            }
            if ident(c) && !c.is_ascii_digit() {
                let start = i;
                while i < chars.len() && ident(chars[i]) {
                    i += 1;
                }
                let word: String = chars[start..i].iter().collect();
                match self.defines.get(&word) {
                    Some(v) => out.push_str(v),
                    None => out.push_str(&word),
                }
                continue;
            }
            if c.is_ascii_digit() {
                // Don't substitute inside numbers like 0x10h.
                while i < chars.len() && ident(chars[i]) {
                    out.push(chars[i]);
                    i += 1;
                }
                continue;
            }
            out.push(c);
            i += 1;
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defines_and_macros() {
        let src = "%define PORT 0x300\n%define CMD PORT+1\n%macro SET 2\n mov %1, %2\n%endmacro\n SET dx, CMD ; c\n";
        let out = preprocess(src).unwrap();
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].line, 6);
        assert!(out[0].text.contains("mov dx, 0x300+1"), "{}", out[0].text);
    }

    #[test]
    fn rep() {
        let out = preprocess("%rep 3\nnop\n%endrep\n").unwrap();
        assert_eq!(out.len(), 3);
    }
}
