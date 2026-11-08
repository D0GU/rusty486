//! The text buffer of an edit window.

use alloc::string::String;
use alloc::vec;
use alloc::vec::Vec;

use crate::scr::{self, Screen};

pub const TAB: usize = 4;
const UNDO_DEPTH: usize = 40;
const UNDO_BYTES: usize = 256 * 1024;

#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Copy, PartialEq)]
enum LastEdit {
    None,
    Typing,
    Deleting,
}

/// The whole text as one block (lines joined with '\n').
struct Snapshot {
    text: Vec<u8>,
    cur: Pos,
}

pub struct Editor {
    pub lines: Vec<Vec<u8>>,
    pub cur: Pos,
    /// Selection anchor (the other end is the cursor).
    pub anchor: Option<Pos>,
    pub top: usize,
    pub left: usize,
    pub modified: bool,
    pub insert: bool,
    pub filename: String,
    /// Bumped on every change (the IDE compares it with the last build).
    pub version: u32,
    undo: Vec<Snapshot>,
    last: LastEdit,
    /// Column the cursor wants when moving vertically.
    want_col: usize,
}

const KEYWORDS: &[&str] = &[
    "as", "break", "const", "continue", "else", "enum", "false", "fn", "for", "if", "impl", "in", "let", "loop", "match",
    "mut", "pub", "return", "self", "Self", "static", "struct", "true", "unsafe", "use", "while", "i8", "u8", "i16", "u16",
    "i32", "u32", "isize", "usize", "bool", "char", "str",
];

impl Editor {
    pub fn new(filename: &str) -> Editor {
        Editor {
            lines: vec![Vec::new()],
            cur: Pos { line: 0, col: 0 },
            anchor: None,
            top: 0,
            left: 0,
            modified: false,
            insert: true,
            filename: String::from(filename),
            version: 0,
            undo: Vec::new(),
            last: LastEdit::None,
            want_col: 0,
        }
    }

    pub fn load(&mut self, data: &[u8]) {
        self.lines.clear();
        let mut line = Vec::new();
        for &c in data {
            match c {
                b'\n' => self.lines.push(core::mem::take(&mut line)),
                b'\r' | 0x1A => {}
                b'\t' => {
                    let n = TAB - line.len() % TAB;
                    line.extend(core::iter::repeat_n(b' ', n));
                }
                c => line.push(c),
            }
        }
        self.lines.push(line);
        self.cur = Pos { line: 0, col: 0 };
        self.anchor = None;
        self.top = 0;
        self.left = 0;
        self.modified = false;
        self.undo.clear();
        self.version += 1;
    }

    /// The text with DOS line endings (trailing spaces trimmed).
    pub fn save_bytes(&self) -> Vec<u8> {
        let mut out = Vec::new();
        let mut n = self.lines.len();
        while n > 1 && self.lines[n - 1].is_empty() {
            n -= 1;
        }
        for l in &self.lines[..n] {
            let mut e = l.len();
            while e > 0 && l[e - 1] == b' ' {
                e -= 1;
            }
            out.extend_from_slice(&l[..e]);
            out.extend_from_slice(b"\r\n");
        }
        out
    }

    pub fn text(&self) -> String {
        let mut s = String::new();
        for l in &self.lines {
            s.push_str(&String::from_utf8_lossy(l));
            s.push('\n');
        }
        s
    }

    fn line_len(&self, l: usize) -> usize {
        self.lines[l].len()
    }

    // ------------------------------------------------------------ undo

    fn snapshot(&mut self, kind: LastEdit) {
        if kind != LastEdit::None && kind == self.last {
            return;
        }
        let mut text = Vec::with_capacity(self.lines.iter().map(|l| l.len() + 1).sum());
        for (i, l) in self.lines.iter().enumerate() {
            if i > 0 {
                text.push(b'\n');
            }
            text.extend_from_slice(l);
        }
        // Keep at most UNDO_DEPTH steps and UNDO_BYTES of history.
        let mut total: usize = self.undo.iter().map(|s| s.text.len()).sum::<usize>() + text.len();
        while !self.undo.is_empty() && (self.undo.len() >= UNDO_DEPTH || total > UNDO_BYTES) {
            total -= self.undo.remove(0).text.len();
        }
        self.undo.push(Snapshot { text, cur: self.cur });
        self.last = kind;
    }

    fn changed(&mut self) {
        self.modified = true;
        self.version += 1;
    }

    pub fn undo(&mut self) -> bool {
        match self.undo.pop() {
            Some(s) => {
                self.lines = s.text.split(|&c| c == b'\n').map(|l| l.to_vec()).collect();
                self.cur = s.cur;
                self.anchor = None;
                self.last = LastEdit::None;
                self.changed();
                true
            }
            None => false,
        }
    }

    // ------------------------------------------------------------ selection

    pub fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        if a == self.cur {
            return None;
        }
        let (s, e) = if (a.line, a.col) < (self.cur.line, self.cur.col) { (a, self.cur) } else { (self.cur, a) };
        Some((s, e))
    }

    fn clip_col(&self, p: Pos) -> Pos {
        Pos { line: p.line, col: p.col.min(self.line_len(p.line)) }
    }

    pub fn selected_text(&self) -> Option<Vec<u8>> {
        let (s, e) = self.selection()?;
        let (s, e) = (self.clip_col(s), self.clip_col(e));
        let mut out = Vec::new();
        for l in s.line..=e.line {
            let line = &self.lines[l];
            let a = if l == s.line { s.col } else { 0 };
            let b = if l == e.line { e.col } else { line.len() };
            out.extend_from_slice(&line[a..b]);
            if l != e.line {
                out.push(b'\n');
            }
        }
        Some(out)
    }

    pub fn delete_selection(&mut self) -> bool {
        let Some((s, e)) = self.selection() else { return false };
        self.snapshot(LastEdit::None);
        let (s, e) = (self.clip_col(s), self.clip_col(e));
        let tail = self.lines[e.line][e.col..].to_vec();
        self.lines[s.line].truncate(s.col);
        self.lines[s.line].extend_from_slice(&tail);
        self.lines.drain(s.line + 1..=e.line);
        self.cur = s;
        self.anchor = None;
        self.changed();
        true
    }

    pub fn select_all(&mut self) {
        self.anchor = Some(Pos { line: 0, col: 0 });
        let l = self.lines.len() - 1;
        self.cur = Pos { line: l, col: self.line_len(l) };
    }

    // ------------------------------------------------------------ editing

    fn pad_to_cursor(&mut self) {
        let l = self.cur.line;
        if self.lines[l].len() < self.cur.col {
            let n = self.cur.col - self.lines[l].len();
            self.lines[l].extend(core::iter::repeat_n(b' ', n));
        }
    }

    pub fn type_char(&mut self, c: u8) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.anchor = None;
        self.snapshot(LastEdit::Typing);
        self.pad_to_cursor();
        let Pos { line, col } = self.cur;
        if self.insert || col >= self.lines[line].len() {
            self.lines[line].insert(col, c);
        } else {
            self.lines[line][col] = c;
        }
        self.cur.col += 1;
        // Auto-dedent a closing brace typed on an otherwise blank line.
        if c == b'}' && self.lines[line][..col].iter().all(|&b| b == b' ') && col >= TAB {
            self.lines[line].drain(col - TAB..col);
            self.cur.col -= TAB;
        }
        self.want_col = self.cur.col;
        self.changed();
    }

    pub fn tab(&mut self) {
        let n = TAB - self.cur.col % TAB;
        for _ in 0..n {
            self.type_char(b' ');
        }
    }

    pub fn newline(&mut self) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.anchor = None;
        self.snapshot(LastEdit::None);
        let Pos { line, col } = self.cur;
        let col = col.min(self.lines[line].len());
        let rest = self.lines[line].split_off(col);
        let mut indent = self.lines[line].iter().take_while(|&&c| c == b' ').count();
        if self.lines[line].last() == Some(&b'{') {
            indent += TAB;
        }
        let mut new = vec![b' '; indent];
        let rest_trim: Vec<u8> = rest.iter().copied().skip_while(|&c| c == b' ').collect();
        new.extend_from_slice(&rest_trim);
        self.lines.insert(line + 1, new);
        self.cur = Pos { line: line + 1, col: indent };
        self.want_col = indent;
        self.last = LastEdit::None;
        self.changed();
    }

    pub fn backspace(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.anchor = None;
        let Pos { line, col } = self.cur;
        if col > self.lines[line].len() {
            self.cur.col -= 1;
            self.want_col = self.cur.col;
            return;
        }
        if col > 0 {
            self.snapshot(LastEdit::Deleting);
            // Backspace over indentation removes a whole tab stop.
            let all_space = self.lines[line][..col].iter().all(|&c| c == b' ');
            let n = if all_space { (col - 1) % TAB + 1 } else { 1 };
            self.lines[line].drain(col - n..col);
            self.cur.col -= n;
        } else if line > 0 {
            self.snapshot(LastEdit::None);
            let cur = self.lines.remove(line);
            let prev = self.lines[line - 1].len();
            self.lines[line - 1].extend_from_slice(&cur);
            self.cur = Pos { line: line - 1, col: prev };
        } else {
            return;
        }
        self.want_col = self.cur.col;
        self.changed();
    }

    pub fn delete(&mut self) {
        if self.delete_selection() {
            return;
        }
        self.anchor = None;
        self.pad_to_cursor();
        let Pos { line, col } = self.cur;
        if col < self.lines[line].len() {
            self.snapshot(LastEdit::Deleting);
            self.lines[line].remove(col);
        } else if line + 1 < self.lines.len() {
            self.snapshot(LastEdit::None);
            let next = self.lines.remove(line + 1);
            self.lines[line].extend_from_slice(&next);
        } else {
            return;
        }
        self.changed();
    }

    pub fn delete_line(&mut self) {
        self.anchor = None;
        self.snapshot(LastEdit::None);
        if self.lines.len() > 1 {
            self.lines.remove(self.cur.line);
            if self.cur.line >= self.lines.len() {
                self.cur.line = self.lines.len() - 1;
            }
        } else {
            self.lines[0].clear();
        }
        self.cur.col = 0;
        self.changed();
    }

    pub fn insert_text(&mut self, t: &[u8]) {
        if self.selection().is_some() {
            self.delete_selection();
        }
        self.anchor = None;
        self.snapshot(LastEdit::None);
        self.pad_to_cursor();
        let Pos { line, col } = self.cur;
        let tail = self.lines[line].split_off(col);
        let mut l = line;
        for &c in t {
            match c {
                b'\n' => {
                    l += 1;
                    self.lines.insert(l, Vec::new());
                }
                b'\r' => {}
                c => self.lines[l].push(c),
            }
        }
        let c = self.lines[l].len();
        self.lines[l].extend_from_slice(&tail);
        self.cur = Pos { line: l, col: c };
        self.want_col = c;
        self.last = LastEdit::None;
        self.changed();
    }

    // ------------------------------------------------------------ movement

    /// Start or drop the selection before a cursor movement.
    pub fn begin_move(&mut self, select: bool) {
        if select {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.last = LastEdit::None;
    }

    pub fn left(&mut self) {
        if self.cur.col > 0 {
            self.cur.col -= 1;
        } else if self.cur.line > 0 {
            self.cur.line -= 1;
            self.cur.col = self.line_len(self.cur.line);
        }
        self.want_col = self.cur.col;
    }

    pub fn right(&mut self) {
        self.cur.col += 1;
        self.want_col = self.cur.col;
    }

    pub fn up(&mut self, n: usize) {
        self.cur.line = self.cur.line.saturating_sub(n);
        self.cur.col = self.want_col;
    }

    pub fn down(&mut self, n: usize) {
        self.cur.line = (self.cur.line + n).min(self.lines.len() - 1);
        self.cur.col = self.want_col;
    }

    pub fn home(&mut self) {
        // Toggle between the first non-blank character and column 0.
        let ind = self.lines[self.cur.line].iter().take_while(|&&c| c == b' ').count();
        self.cur.col = if self.cur.col == ind { 0 } else { ind };
        self.want_col = self.cur.col;
    }

    pub fn end(&mut self) {
        self.cur.col = self.line_len(self.cur.line);
        self.want_col = self.cur.col;
    }

    pub fn word_left(&mut self) {
        let l = &self.lines[self.cur.line];
        let mut c = self.cur.col.min(l.len());
        if c == 0 {
            self.left();
            return;
        }
        while c > 0 && !is_word(l[c - 1]) {
            c -= 1;
        }
        while c > 0 && is_word(l[c - 1]) {
            c -= 1;
        }
        self.cur.col = c;
        self.want_col = c;
    }

    pub fn word_right(&mut self) {
        let l = &self.lines[self.cur.line];
        let mut c = self.cur.col;
        if c >= l.len() {
            if self.cur.line + 1 < self.lines.len() {
                self.cur.line += 1;
                self.cur.col = 0;
            }
            self.want_col = self.cur.col;
            return;
        }
        while c < l.len() && is_word(l[c]) {
            c += 1;
        }
        while c < l.len() && !is_word(l[c]) {
            c += 1;
        }
        self.cur.col = c;
        self.want_col = c;
    }

    pub fn goto(&mut self, line: usize, col: usize) {
        self.anchor = None;
        self.cur.line = line.min(self.lines.len() - 1);
        self.cur.col = col;
        self.want_col = col;
    }

    pub fn doc_start(&mut self) {
        self.cur = Pos { line: 0, col: 0 };
        self.want_col = 0;
    }

    pub fn doc_end(&mut self) {
        let l = self.lines.len() - 1;
        self.cur = Pos { line: l, col: self.line_len(l) };
        self.want_col = self.cur.col;
    }

    /// Find `pat` (case-insensitive) after the cursor, wrapping around.
    pub fn find(&mut self, pat: &str) -> bool {
        let p: Vec<u8> = pat.bytes().map(|c| c.to_ascii_lowercase()).collect();
        if p.is_empty() {
            return false;
        }
        let n = self.lines.len();
        for k in 0..=n {
            let l = (self.cur.line + k) % n;
            let line: Vec<u8> = self.lines[l].iter().map(|c| c.to_ascii_lowercase()).collect();
            let start = if k == 0 { (self.cur.col + 1).min(line.len()) } else { 0 };
            if k == n && start == 0 {
                break;
            }
            if line.len() >= p.len() {
                for c in start..=line.len() - p.len() {
                    if line[c..c + p.len()] == p[..] {
                        self.anchor = Some(Pos { line: l, col: c + p.len() });
                        self.cur = Pos { line: l, col: c };
                        self.want_col = c;
                        return true;
                    }
                }
            }
        }
        false
    }

    // ------------------------------------------------------------ view

    /// Keep the cursor inside a `w` x `h` view.
    pub fn scroll_into_view(&mut self, w: usize, h: usize) {
        if self.cur.line < self.top {
            self.top = self.cur.line;
        }
        if self.cur.line >= self.top + h {
            self.top = self.cur.line + 1 - h;
        }
        if self.cur.col < self.left {
            self.left = self.cur.col;
        }
        if self.cur.col >= self.left + w {
            self.left = self.cur.col + 1 - w;
        }
    }

    /// Draw the text with syntax colours into the view at (x, y).
    pub fn draw(&self, s: &mut Screen, x: usize, y: usize, w: usize, h: usize) {
        let mut in_comment = self.comment_state(self.top);
        let sel = self.selection();
        for row in 0..h {
            let l = self.top + row;
            s.fill(x, y + row, w, 1, b' ', scr::EDIT_TEXT);
            if l >= self.lines.len() {
                continue;
            }
            let line = &self.lines[l];
            let colors = highlight(line, &mut in_comment);
            for col in 0..w {
                let c = self.left + col;
                let in_sel = sel.is_some_and(|(a, b)| {
                    let p = (l, c);
                    p >= (a.line, a.col) && p < (b.line, b.col) && (c < line.len() || l < b.line)
                });
                let (ch, attr) = if c < line.len() { (line[c], colors[c]) } else { (b' ', scr::EDIT_TEXT) };
                s.put(x + col, y + row, ch, if in_sel { scr::EDIT_SEL } else { attr });
            }
        }
    }

    /// Whether line `l` starts inside a block comment.
    fn comment_state(&self, l: usize) -> bool {
        let mut inside = false;
        for line in &self.lines[..l.min(self.lines.len())] {
            scan_comments(line, &mut inside);
        }
        inside
    }
}

fn is_word(c: u8) -> bool {
    c.is_ascii_alphanumeric() || c == b'_'
}

/// Track block comments through a line without colouring it.
fn scan_comments(line: &[u8], in_comment: &mut bool) {
    let mut i = 0;
    let mut in_str = false;
    while i < line.len() {
        let c = line[i];
        let next = line.get(i + 1).copied();
        if *in_comment {
            if c == b'*' && next == Some(b'/') {
                *in_comment = false;
                i += 1;
            }
        } else if in_str {
            if c == b'\\' {
                i += 1;
            } else if c == b'"' {
                in_str = false;
            }
        } else if c == b'"' {
            in_str = true;
        } else if c == b'/' && next == Some(b'/') {
            return;
        } else if c == b'/' && next == Some(b'*') {
            *in_comment = true;
            i += 1;
        }
        i += 1;
    }
}

/// Colours for each byte of a line; `in_comment` carries block comments.
pub fn highlight(line: &[u8], in_comment: &mut bool) -> Vec<u8> {
    let mut out = vec![scr::EDIT_TEXT; line.len()];
    let mut i = 0;
    while i < line.len() {
        if *in_comment {
            let start = i;
            while i < line.len() && !(line[i] == b'*' && line.get(i + 1) == Some(&b'/')) {
                i += 1;
            }
            if i < line.len() {
                i += 2;
                *in_comment = false;
            }
            out[start..i.min(line.len())].fill(scr::EDIT_COMMENT);
            continue;
        }
        let c = line[i];
        if c == b'/' && line.get(i + 1) == Some(&b'/') {
            out[i..].fill(scr::EDIT_COMMENT);
            break;
        }
        if c == b'/' && line.get(i + 1) == Some(&b'*') {
            *in_comment = true;
            out[i] = scr::EDIT_COMMENT;
            out[i + 1] = scr::EDIT_COMMENT;
            i += 2;
            continue;
        }
        if c == b'"' {
            let start = i;
            i += 1;
            while i < line.len() && line[i] != b'"' {
                if line[i] == b'\\' {
                    i += 1;
                }
                i += 1;
            }
            i = (i + 1).min(line.len());
            out[start..i].fill(scr::EDIT_STRING);
            continue;
        }
        if c == b'\'' {
            // Char literal ('a', '\n') but not a lifetime ('a).
            let end = if line.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
            if line.get(end) == Some(&b'\'') {
                out[i..=end].fill(scr::EDIT_STRING);
                i = end + 1;
                continue;
            }
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == b'_') {
                i += 1;
            }
            out[start..i].fill(scr::EDIT_NUMBER);
            continue;
        }
        if is_word(c) {
            let start = i;
            while i < line.len() && is_word(line[i]) {
                i += 1;
            }
            let w = &line[start..i];
            if KEYWORDS.iter().any(|k| k.as_bytes() == w) {
                out[start..i].fill(scr::EDIT_KEYWORD);
            } else if line.get(i) == Some(&b'!') {
                out[start..=i].fill(scr::EDIT_KEYWORD);
                i += 1;
            }
            continue;
        }
        i += 1;
    }
    out
}
