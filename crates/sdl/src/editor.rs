//! The built-in code editor, styled after the DOS-era IDEs: a grey menu bar,
//! a blue editing window with a double-line frame and scroll bar, and a grey
//! function-key status line. It draws into an 80x25 text grid.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Instant, SystemTime};

use rusty486_asm::AsmError;
use sdl2::clipboard::ClipboardUtil;
use sdl2::keyboard::{Keycode, Mod};

pub const COLS: usize = 80;
pub const ROWS: usize = 25;
/// The text area inside the window frame.
const VIEW_TOP: usize = 2;
const VIEW_ROWS: usize = 21;
const VIEW_LEFT: usize = 1;
const VIEW_COLS: usize = 78;
const TAB_WIDTH: usize = 8;
const UNDO_LIMIT: usize = 500;

// CGA colours.
const BLACK: u8 = 0;
const BLUE: u8 = 1;
const GREEN: u8 = 2;
const CYAN: u8 = 3;
const RED: u8 = 4;
const LIGHT_GRAY: u8 = 7;
const LIGHT_GREEN: u8 = 10;
const LIGHT_CYAN: u8 = 11;
const LIGHT_RED: u8 = 12;
const LIGHT_MAGENTA: u8 = 13;
const YELLOW: u8 = 14;
const WHITE: u8 = 15;

const fn attr(fg: u8, bg: u8) -> u8 {
    bg << 4 | fg
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Pos {
    pub line: usize,
    pub col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tab {
    Code,
    Sprites,
    Map,
    Sound,
    Disks,
}

/// Menu bar entries: tab, label, hotkey letter index.
const TABS: [(Tab, &str); 5] =
    [(Tab::Code, "Code"), (Tab::Sprites, "Sprites"), (Tab::Map, "Map"), (Tab::Sound, "Sound"), (Tab::Disks, "Disks")];

/// What the frontend should do after an editor event.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    None,
    Save,
    Build,
    Run,
    /// Switch back to the machine view.
    Machine,
    Reload,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Error,
}

#[derive(Clone)]
struct Snap {
    lines: Vec<Vec<char>>,
    cur: Pos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum EditKind {
    None,
    Typing,
    Deleting,
    Other,
}

pub struct Editor {
    pub lines: Vec<Vec<char>>,
    pub cur: Pos,
    anchor: Option<Pos>,
    top: usize,
    left: usize,
    pub path: PathBuf,
    pub dirty: bool,
    disk_mtime: Option<SystemTime>,
    warned_mtime: Option<SystemTime>,
    undo: Vec<Snap>,
    redo: Vec<Snap>,
    last_edit: EditKind,
    /// Assembler errors by 1-based line.
    pub errors: BTreeMap<usize, String>,
    status: Option<(String, Level, Instant)>,
    pub tab: Tab,
    overwrite: bool,
    dragging: bool,
    /// Column to aim for when moving vertically through short lines.
    want_col: Option<usize>,
}

impl Editor {
    pub fn open(path: PathBuf) -> std::io::Result<Editor> {
        let text = std::fs::read_to_string(&path)?;
        let mut e = Editor::from_text(path, &text);
        e.disk_mtime = mtime(&e.path);
        Ok(e)
    }

    pub fn from_text(path: PathBuf, text: &str) -> Editor {
        let mut e = Editor {
            lines: vec![vec![]],
            cur: Pos::default(),
            anchor: None,
            top: 0,
            left: 0,
            disk_mtime: None,
            warned_mtime: None,
            path,
            dirty: false,
            undo: Vec::new(),
            redo: Vec::new(),
            last_edit: EditKind::None,
            errors: BTreeMap::new(),
            status: None,
            tab: Tab::Code,
            overwrite: false,
            dragging: false,
            want_col: None,
        };
        e.set_text_raw(text);
        e
    }

    pub fn file_name(&self) -> String {
        self.path.file_name().map_or("untitled".into(), |n| n.to_string_lossy().into_owned())
    }

    /// The buffer as text (with a trailing newline).
    pub fn text(&self) -> String {
        let mut s: String =
            self.lines.iter().map(|l| l.iter().collect::<String>()).collect::<Vec<_>>().join("\n");
        s.push('\n');
        s
    }

    fn set_text_raw(&mut self, text: &str) {
        self.lines = text
            .split('\n')
            .map(|l| {
                let mut out = Vec::new();
                for c in l.trim_end_matches('\r').chars() {
                    if c == '\t' {
                        let n = TAB_WIDTH - out.len() % TAB_WIDTH;
                        out.extend(std::iter::repeat_n(' ', n));
                    } else {
                        out.push(c);
                    }
                }
                out
            })
            .collect();
        // A trailing newline doesn't make an extra empty line.
        if self.lines.len() > 1 && self.lines.last().is_some_and(|l| l.is_empty()) {
            self.lines.pop();
        }
        self.anchor = None;
        self.clamp();
    }

    /// Replace the whole buffer (undoable), e.g. from an MCP edit.
    pub fn set_text(&mut self, text: &str) {
        self.snapshot(EditKind::Other);
        self.set_text_raw(text);
        self.dirty = true;
    }

    pub fn save(&mut self) -> std::io::Result<()> {
        std::fs::write(&self.path, self.text())?;
        self.dirty = false;
        self.disk_mtime = mtime(&self.path);
        Ok(())
    }

    pub fn reload(&mut self) -> std::io::Result<()> {
        let text = std::fs::read_to_string(&self.path)?;
        self.snapshot(EditKind::Other);
        self.set_text_raw(&text);
        self.dirty = false;
        self.disk_mtime = mtime(&self.path);
        Ok(())
    }

    /// Pick up changes made to the file by other programs (e.g. Claude Code).
    pub fn check_disk(&mut self) {
        let now = mtime(&self.path);
        if now.is_none() || now == self.disk_mtime {
            return;
        }
        if !self.dirty {
            if self.reload().is_ok() {
                self.set_status("Reloaded: the file changed on disk", Level::Info);
            }
        } else if self.warned_mtime != now {
            self.warned_mtime = now;
            self.set_status("File changed on disk. Ctrl+L reloads it (losing your edits)", Level::Error);
        }
    }

    pub fn set_status(&mut self, text: impl Into<String>, level: Level) {
        self.status = Some((text.into(), level, Instant::now()));
    }

    /// Show assembler errors and move to the first one.
    pub fn set_errors(&mut self, errors: &[AsmError]) {
        self.errors = errors.iter().map(|e| (e.line, e.message.clone())).collect();
        if let Some(e) = errors.first() {
            self.goto_line(e.line);
            let more = if errors.len() > 1 { format!(" (+{} more, F8 next)", errors.len() - 1) } else { String::new() };
            self.set_status(format!("Error on line {}: {}{more}", e.line, e.message), Level::Error);
        }
    }

    pub fn clear_errors(&mut self) {
        self.errors.clear();
    }

    pub fn goto_line(&mut self, line: usize) {
        self.anchor = None;
        self.cur = Pos { line: line.saturating_sub(1).min(self.lines.len() - 1), col: 0 };
        self.top = self.cur.line.saturating_sub(VIEW_ROWS / 3);
        self.ensure_visible();
    }

    // ------------------------------------------------------------ editing

    fn snapshot(&mut self, kind: EditKind) {
        if kind != self.last_edit || kind == EditKind::Other {
            self.undo.push(Snap { lines: self.lines.clone(), cur: self.cur });
            if self.undo.len() > UNDO_LIMIT {
                self.undo.remove(0);
            }
            self.redo.clear();
        }
        self.last_edit = kind;
        self.dirty = true;
        self.want_col = None;
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(Snap { lines: std::mem::replace(&mut self.lines, s.lines), cur: self.cur });
            self.cur = s.cur;
            self.after_history();
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(Snap { lines: std::mem::replace(&mut self.lines, s.lines), cur: self.cur });
            self.cur = s.cur;
            self.after_history();
        }
    }

    fn after_history(&mut self) {
        self.anchor = None;
        self.dirty = true;
        self.last_edit = EditKind::None;
        self.clamp();
    }

    fn selection(&self) -> Option<(Pos, Pos)> {
        let a = self.anchor?;
        (a != self.cur).then(|| if a < self.cur { (a, self.cur) } else { (self.cur, a) })
    }

    fn selected_text(&self) -> Option<String> {
        let (a, b) = self.selection()?;
        let mut s = String::new();
        for line in a.line..=b.line {
            let l = &self.lines[line];
            let from = if line == a.line { a.col.min(l.len()) } else { 0 };
            let to = if line == b.line { b.col.min(l.len()) } else { l.len() };
            s.extend(&l[from..to]);
            if line != b.line {
                s.push('\n');
            }
        }
        Some(s)
    }

    fn delete_range(&mut self, a: Pos, b: Pos) {
        let tail: Vec<char> = self.lines[b.line][b.col.min(self.lines[b.line].len())..].to_vec();
        self.lines[a.line].truncate(a.col);
        self.lines[a.line].extend(tail);
        self.lines.drain(a.line + 1..=b.line);
        self.cur = a;
    }

    fn delete_selection(&mut self) -> bool {
        match self.selection() {
            Some((a, b)) => {
                self.delete_range(a, b);
                self.anchor = None;
                true
            }
            None => {
                self.anchor = None;
                false
            }
        }
    }

    /// Insert text at the cursor (replacing any selection).
    pub fn insert(&mut self, text: &str) {
        self.delete_selection();
        for c in text.chars() {
            match c {
                '\n' => {
                    let rest = self.lines[self.cur.line].split_off(self.cur.col);
                    self.lines.insert(self.cur.line + 1, rest);
                    self.cur = Pos { line: self.cur.line + 1, col: 0 };
                }
                '\r' => {}
                '\t' => {
                    let n = TAB_WIDTH - self.cur.col % TAB_WIDTH;
                    for _ in 0..n {
                        self.lines[self.cur.line].insert(self.cur.col, ' ');
                        self.cur.col += 1;
                    }
                }
                c => {
                    self.lines[self.cur.line].insert(self.cur.col, c);
                    self.cur.col += 1;
                }
            }
        }
        self.ensure_visible();
    }

    fn newline(&mut self) {
        self.snapshot(EditKind::Other);
        self.delete_selection();
        let indent: String =
            self.lines[self.cur.line].iter().take_while(|c| **c == ' ').take(self.cur.col).collect();
        self.insert("\n");
        self.insert(&indent);
    }

    fn backspace(&mut self) {
        self.snapshot(EditKind::Deleting);
        if self.delete_selection() {
            return;
        }
        if self.cur.col > 0 {
            self.cur.col -= 1;
            self.lines[self.cur.line].remove(self.cur.col);
        } else if self.cur.line > 0 {
            let line = self.lines.remove(self.cur.line);
            self.cur.line -= 1;
            self.cur.col = self.lines[self.cur.line].len();
            self.lines[self.cur.line].extend(line);
        }
        self.ensure_visible();
    }

    fn delete(&mut self) {
        self.snapshot(EditKind::Deleting);
        if self.delete_selection() {
            return;
        }
        let len = self.lines[self.cur.line].len();
        if self.cur.col < len {
            self.lines[self.cur.line].remove(self.cur.col);
        } else if self.cur.line + 1 < self.lines.len() {
            let next = self.lines.remove(self.cur.line + 1);
            self.lines[self.cur.line].extend(next);
        }
    }

    // ------------------------------------------------------------ movement

    fn clamp(&mut self) {
        if self.lines.is_empty() {
            self.lines.push(vec![]);
        }
        self.cur.line = self.cur.line.min(self.lines.len() - 1);
        self.cur.col = self.cur.col.min(self.lines[self.cur.line].len());
        self.ensure_visible();
    }

    fn ensure_visible(&mut self) {
        if self.cur.line < self.top {
            self.top = self.cur.line;
        } else if self.cur.line >= self.top + VIEW_ROWS {
            self.top = self.cur.line + 1 - VIEW_ROWS;
        }
        if self.cur.col < self.left {
            self.left = self.cur.col;
        } else if self.cur.col >= self.left + VIEW_COLS {
            self.left = self.cur.col + 1 - VIEW_COLS;
        }
    }

    /// Start a cursor movement: Shift extends the selection.
    fn begin_move(&mut self, shift: bool) {
        if shift {
            if self.anchor.is_none() {
                self.anchor = Some(self.cur);
            }
        } else {
            self.anchor = None;
        }
        self.last_edit = EditKind::None;
    }

    fn move_vertical(&mut self, delta: isize) {
        let col = *self.want_col.get_or_insert(self.cur.col);
        let line = (self.cur.line as isize + delta).clamp(0, self.lines.len() as isize - 1) as usize;
        self.cur = Pos { line, col: col.min(self.lines[line].len()) };
    }

    fn word_left(&mut self) {
        if self.cur.col == 0 {
            if self.cur.line > 0 {
                self.cur.line -= 1;
                self.cur.col = self.lines[self.cur.line].len();
            }
            return;
        }
        let l = &self.lines[self.cur.line];
        let mut c = self.cur.col;
        while c > 0 && !is_word(l[c - 1]) {
            c -= 1;
        }
        while c > 0 && is_word(l[c - 1]) {
            c -= 1;
        }
        self.cur.col = c;
    }

    fn word_right(&mut self) {
        let l = &self.lines[self.cur.line];
        if self.cur.col >= l.len() {
            if self.cur.line + 1 < self.lines.len() {
                self.cur = Pos { line: self.cur.line + 1, col: 0 };
            }
            return;
        }
        let mut c = self.cur.col;
        while c < l.len() && is_word(l[c]) {
            c += 1;
        }
        while c < l.len() && !is_word(l[c]) {
            c += 1;
        }
        self.cur.col = c;
    }

    fn next_error(&mut self) {
        let after = self.errors.range(self.cur.line + 2..).next().or_else(|| self.errors.iter().next());
        if let Some((&line, msg)) = after {
            let msg = msg.clone();
            self.goto_line(line);
            self.set_status(format!("Error on line {line}: {msg}"), Level::Error);
        }
    }

    // ------------------------------------------------------------ input

    pub fn key(&mut self, key: Keycode, m: Mod, clip: &ClipboardUtil) -> Action {
        let ctrl = m.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD);
        let shift = m.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD);
        let alt = m.intersects(Mod::LALTMOD | Mod::RALTMOD);

        // Global keys, valid on every tab.
        match key {
            Keycode::Escape => return Action::Machine,
            Keycode::F2 => return Action::Save,
            Keycode::F9 if ctrl => return Action::Run,
            Keycode::F9 => return Action::Build,
            Keycode::F5 => return Action::Run,
            Keycode::S if ctrl => return Action::Save,
            Keycode::R if ctrl => return Action::Run,
            Keycode::B if ctrl => return Action::Build,
            Keycode::L if ctrl => return Action::Reload,
            _ => {}
        }
        if alt {
            let letter = key.name().to_ascii_lowercase();
            if let Some((tab, _)) = TABS.iter().find(|(_, n)| n[..1].eq_ignore_ascii_case(&letter)) {
                self.tab = *tab;
            }
            return Action::None;
        }
        if self.tab != Tab::Code {
            return Action::None;
        }

        let page = VIEW_ROWS as isize - 1;
        match key {
            Keycode::Left | Keycode::Right | Keycode::Up | Keycode::Down | Keycode::Home | Keycode::End
            | Keycode::PageUp | Keycode::PageDown => {
                self.begin_move(shift);
                if !matches!(key, Keycode::Up | Keycode::Down | Keycode::PageUp | Keycode::PageDown) {
                    self.want_col = None;
                }
                match key {
                    Keycode::Left if ctrl => self.word_left(),
                    Keycode::Right if ctrl => self.word_right(),
                    Keycode::Left => {
                        if self.cur.col > 0 {
                            self.cur.col -= 1;
                        } else if self.cur.line > 0 {
                            self.cur.line -= 1;
                            self.cur.col = self.lines[self.cur.line].len();
                        }
                    }
                    Keycode::Right => {
                        if self.cur.col < self.lines[self.cur.line].len() {
                            self.cur.col += 1;
                        } else if self.cur.line + 1 < self.lines.len() {
                            self.cur = Pos { line: self.cur.line + 1, col: 0 };
                        }
                    }
                    Keycode::Up => self.move_vertical(-1),
                    Keycode::Down => self.move_vertical(1),
                    Keycode::PageUp => {
                        self.top = self.top.saturating_sub(page as usize);
                        self.move_vertical(-page);
                    }
                    Keycode::PageDown => {
                        self.top = (self.top + page as usize).min(self.lines.len().saturating_sub(1));
                        self.move_vertical(page);
                    }
                    Keycode::Home if ctrl => self.cur = Pos::default(),
                    Keycode::End if ctrl => {
                        let line = self.lines.len() - 1;
                        self.cur = Pos { line, col: self.lines[line].len() };
                    }
                    Keycode::Home => {
                        // Toggle between first non-blank and column 0.
                        let indent = self.lines[self.cur.line].iter().take_while(|c| **c == ' ').count();
                        self.cur.col = if self.cur.col == indent { 0 } else { indent };
                    }
                    _ => self.cur.col = self.lines[self.cur.line].len(),
                }
                self.ensure_visible();
            }
            Keycode::Backspace => self.backspace(),
            Keycode::Delete if shift => self.cut(clip),
            Keycode::Delete => self.delete(),
            Keycode::Return | Keycode::KpEnter => self.newline(),
            Keycode::Tab => {
                self.snapshot(EditKind::Typing);
                self.insert("\t");
            }
            Keycode::Insert if ctrl => self.copy(clip),
            Keycode::Insert if shift => self.paste(clip),
            Keycode::Insert => self.overwrite = !self.overwrite,
            Keycode::F8 => self.next_error(),
            Keycode::A if ctrl => {
                self.anchor = Some(Pos::default());
                let line = self.lines.len() - 1;
                self.cur = Pos { line, col: self.lines[line].len() };
                self.ensure_visible();
            }
            Keycode::C if ctrl => self.copy(clip),
            Keycode::X if ctrl => self.cut(clip),
            Keycode::V if ctrl => self.paste(clip),
            Keycode::Z if ctrl => self.undo(),
            Keycode::Y if ctrl => self.redo(),
            _ => {}
        }
        Action::None
    }

    /// Typed text (from SDL text input events).
    pub fn text_input(&mut self, s: &str) {
        if self.tab != Tab::Code {
            return;
        }
        let s: String = s.chars().filter(|c| (' '..='~').contains(c)).collect();
        if s.is_empty() {
            return;
        }
        self.snapshot(EditKind::Typing);
        if self.overwrite && self.selection().is_none() {
            let line = &mut self.lines[self.cur.line];
            for _ in 0..s.chars().count() {
                if self.cur.col < line.len() {
                    line.remove(self.cur.col);
                }
            }
        }
        self.insert(&s);
    }

    fn copy(&mut self, clip: &ClipboardUtil) {
        if let Some(t) = self.selected_text() {
            let _ = clip.set_clipboard_text(&t.replace('\n', "\r\n"));
        }
    }

    fn cut(&mut self, clip: &ClipboardUtil) {
        if self.selection().is_some() {
            self.copy(clip);
            self.snapshot(EditKind::Other);
            self.delete_selection();
            self.ensure_visible();
        }
    }

    fn paste(&mut self, clip: &ClipboardUtil) {
        if let Ok(t) = clip.clipboard_text() {
            self.snapshot(EditKind::Other);
            self.insert(&t);
        }
    }

    /// Mouse button press at a text cell. Returns true if handled.
    pub fn mouse_down(&mut self, col: usize, row: usize, shift: bool) {
        if row == 0 {
            // Menu bar: switch tabs.
            let mut x = 2;
            for (tab, name) in TABS {
                if (x..x + name.len() + 2).contains(&col) {
                    self.tab = tab;
                }
                x += name.len() + 2;
            }
            return;
        }
        if self.tab != Tab::Code {
            return;
        }
        if col == COLS - 1 && (VIEW_TOP..VIEW_TOP + VIEW_ROWS).contains(&row) {
            // Scroll bar: arrows step, the track pages.
            let max_top = self.lines.len().saturating_sub(1);
            match row {
                r if r == VIEW_TOP => self.top = self.top.saturating_sub(1),
                r if r == VIEW_TOP + VIEW_ROWS - 1 => self.top = (self.top + 1).min(max_top),
                r if r < self.thumb_row() => self.top = self.top.saturating_sub(VIEW_ROWS),
                _ => self.top = (self.top + VIEW_ROWS).min(max_top),
            }
            return;
        }
        if let Some(p) = self.cell_pos(col, row) {
            self.begin_move(shift);
            self.cur = p;
            self.want_col = None;
            self.dragging = true;
            if !shift {
                self.anchor = Some(p);
            }
        }
    }

    pub fn mouse_drag(&mut self, col: usize, row: usize) {
        if !self.dragging {
            return;
        }
        let row = row.clamp(VIEW_TOP, VIEW_TOP + VIEW_ROWS - 1);
        let col = col.clamp(VIEW_LEFT, VIEW_LEFT + VIEW_COLS - 1);
        if let Some(p) = self.cell_pos(col, row) {
            self.cur = p;
            self.ensure_visible();
        }
    }

    pub fn mouse_up(&mut self) {
        self.dragging = false;
    }

    pub fn wheel(&mut self, dy: i32) {
        let max_top = self.lines.len().saturating_sub(1) as isize;
        self.top = (self.top as isize - dy as isize * 3).clamp(0, max_top) as usize;
    }

    fn cell_pos(&self, col: usize, row: usize) -> Option<Pos> {
        if !(VIEW_TOP..VIEW_TOP + VIEW_ROWS).contains(&row) || !(VIEW_LEFT..VIEW_LEFT + VIEW_COLS).contains(&col) {
            return None;
        }
        let line = (self.top + row - VIEW_TOP).min(self.lines.len() - 1);
        let c = (self.left + col - VIEW_LEFT).min(self.lines[line].len());
        Some(Pos { line, col: c })
    }

    fn thumb_row(&self) -> usize {
        let track = VIEW_ROWS - 3; // between the two arrows
        let max_top = self.lines.len().saturating_sub(1).max(1);
        VIEW_TOP + 1 + (self.top.min(max_top) * track / max_top)
    }

    // ------------------------------------------------------------ drawing

    /// Draw the editor into an 80x25 (char, attr) grid. Returns the cursor
    /// position (cell offset) and shape for the renderer.
    pub fn draw(&mut self, buf: &mut [u8], project: &str) -> Option<(usize, (u8, u8))> {
        let mut g = Grid(buf);
        g.fill(0, 0, COLS, ROWS, b' ', attr(LIGHT_GRAY, BLUE));

        // Menu bar.
        g.fill(0, 0, COLS, 1, b' ', attr(BLACK, LIGHT_GRAY));
        let mut x = 2;
        for (tab, name) in TABS {
            let active = tab == self.tab;
            let (text_attr, key_attr) = if active {
                (attr(BLACK, GREEN), attr(RED, GREEN))
            } else {
                (attr(BLACK, LIGHT_GRAY), attr(RED, LIGHT_GRAY))
            };
            g.puts(x, 0, &format!(" {name} "), text_attr);
            g.put(x + 1, 0, name.as_bytes()[0], key_attr);
            x += name.len() + 2;
        }
        let right = format!("{project} ");
        g.puts(COLS - right.len(), 0, &right, attr(BLACK, LIGHT_GRAY));

        // Window frame.
        let frame = attr(WHITE, BLUE);
        g.put(0, 1, 0xC9, frame);
        g.fill(1, 1, COLS - 2, 1, 0xCD, frame);
        g.put(COLS - 1, 1, 0xBB, frame);
        for r in 2..ROWS - 2 {
            g.put(0, r, 0xBA, frame);
            g.put(COLS - 1, r, 0xBA, frame);
        }
        g.put(0, ROWS - 2, 0xC8, frame);
        g.fill(1, ROWS - 2, COLS - 2, 1, 0xCD, frame);
        g.put(COLS - 1, ROWS - 2, 0xBC, frame);

        let title = match self.tab {
            Tab::Code => format!(" {}{} ", self.file_name(), if self.dirty { "*" } else { "" }),
            other => format!(" {} ", TABS.iter().find(|t| t.0 == other).unwrap().1),
        };
        g.puts((COLS - title.len()) / 2, 1, &title, attr(BLUE, LIGHT_GRAY));

        let cursor = if self.tab == Tab::Code {
            self.draw_code(&mut g);
            let pos = format!(" {}:{} ", self.cur.line + 1, self.cur.col + 1);
            g.puts(3, ROWS - 2, &pos, frame);
            if self.overwrite {
                g.puts(3 + pos.len(), ROWS - 2, " OVR ", frame);
            }
            let row = VIEW_TOP + self.cur.line - self.top;
            let col = VIEW_LEFT + self.cur.col - self.left;
            Some((row * COLS + col, if self.overwrite { (0, 15) } else { (13, 14) }))
        } else {
            None // the tab's own content is drawn by tabs::draw
        };

        // Status line.
        let status_attr = attr(BLACK, LIGHT_GRAY);
        g.fill(0, ROWS - 1, COLS, 1, b' ', status_attr);
        match &self.status {
            Some((text, level, at)) if at.elapsed().as_secs() < 8 || *level == Level::Error => {
                let a = if *level == Level::Error { attr(WHITE, RED) } else { attr(BLACK, CYAN) };
                g.fill(0, ROWS - 1, COLS, 1, b' ', a);
                let text: String = text.chars().take(COLS - 2).collect();
                g.puts(1, ROWS - 1, &text, a);
            }
            _ => {
                let keys = [("F2", "Save"), ("F9", "Build"), ("Ctrl+F9", "Run"), ("F8", "Next error"), ("Esc", "Machine")];
                let mut x = 1;
                for (k, label) in keys {
                    g.puts(x, ROWS - 1, k, attr(RED, LIGHT_GRAY));
                    x += k.len() + 1;
                    g.puts(x, ROWS - 1, label, status_attr);
                    x += label.len() + 2;
                }
            }
        }
        cursor
    }

    fn draw_code(&mut self, g: &mut Grid) {
        self.ensure_visible();
        let sel = self.selection();
        for r in 0..VIEW_ROWS {
            let line_no = self.top + r;
            let Some(line) = self.lines.get(line_no) else { break };
            let colors = highlight(line);
            let error = self.errors.contains_key(&(line_no + 1));
            for c in 0..VIEW_COLS {
                let col = self.left + c;
                let ch = line.get(col).copied().unwrap_or(' ');
                let pos = Pos { line: line_no, col };
                let selected = sel.is_some_and(|(a, b)| pos >= a && pos < b);
                let a = if selected {
                    attr(BLUE, LIGHT_GRAY)
                } else if error {
                    attr(WHITE, RED)
                } else {
                    attr(colors.get(col).copied().unwrap_or(LIGHT_GRAY), BLUE)
                };
                let byte = if ch.is_ascii() { ch as u8 } else { b'?' };
                g.put(VIEW_LEFT + c, VIEW_TOP + r, byte, a);
            }
        }

        // Vertical scroll bar in the right border.
        let bar = attr(BLUE, CYAN);
        for r in VIEW_TOP..VIEW_TOP + VIEW_ROWS {
            g.put(COLS - 1, r, 0xB0, bar);
        }
        g.put(COLS - 1, VIEW_TOP, 0x1E, attr(BLUE, CYAN));
        g.put(COLS - 1, VIEW_TOP + VIEW_ROWS - 1, 0x1F, attr(BLUE, CYAN));
        g.put(COLS - 1, self.thumb_row(), 0xFE, attr(BLUE, CYAN));
    }
}

fn is_word(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.'
}

fn mtime(p: &std::path::Path) -> Option<SystemTime> {
    std::fs::metadata(p).and_then(|m| m.modified()).ok()
}

/// Syntax colours (foreground) for each character of a line.
fn highlight(line: &[char]) -> Vec<u8> {
    let mut out = vec![YELLOW; line.len()];
    let mut i = 0;
    let mut first_word = true;
    while i < line.len() {
        let c = line[i];
        if c == ';' {
            out[i..].fill(LIGHT_GRAY);
            break;
        }
        if c == '\'' || c == '"' || c == '`' {
            let start = i;
            i += 1;
            while i < line.len() && line[i] != c {
                i += 1;
            }
            i = (i + 1).min(line.len());
            out[start..i].fill(LIGHT_GREEN);
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || line[i] == '_') {
                i += 1;
            }
            out[start..i].fill(LIGHT_MAGENTA);
            continue;
        }
        if c.is_ascii_alphabetic() || matches!(c, '_' | '.' | '%' | '$' | '?' | '@') {
            let start = i;
            i += 1;
            while i < line.len() && (line[i].is_ascii_alphanumeric() || matches!(line[i], '_' | '.' | '$' | '#' | '@' | '~' | '?')) {
                i += 1;
            }
            let word: String = line[start..i].iter().collect::<String>().to_ascii_lowercase();
            let label = line.get(i) == Some(&':') || (first_word && start == 0);
            let color = if word.starts_with('%') {
                LIGHT_RED
            } else if rusty486_asm::regs::is_reg(&word) {
                LIGHT_CYAN
            } else if rusty486_asm::is_mnemonic(&word) {
                WHITE
            } else if rusty486_asm::is_directive_word(&word) {
                LIGHT_RED
            } else if label {
                LIGHT_GREEN
            } else {
                YELLOW
            };
            out[start..i].fill(color);
            first_word = false;
            continue;
        }
        if !c.is_whitespace() {
            out[i] = LIGHT_GRAY; // punctuation
        }
        i += 1;
    }
    out
}

/// Helper for writing into an 80x25 (char, attr) buffer.
struct Grid<'a>(&'a mut [u8]);

impl Grid<'_> {
    fn put(&mut self, x: usize, y: usize, ch: u8, a: u8) {
        if x < COLS && y < ROWS {
            let i = (y * COLS + x) * 2;
            self.0[i] = ch;
            self.0[i + 1] = a;
        }
    }

    fn puts(&mut self, x: usize, y: usize, s: &str, a: u8) {
        for (k, b) in s.bytes().enumerate() {
            self.put(x + k, y, b, a);
        }
    }

    fn fill(&mut self, x: usize, y: usize, w: usize, h: usize, ch: u8, a: u8) {
        for r in y..y + h {
            for c in x..x + w {
                self.put(c, r, ch, a);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(text: &str) -> Editor {
        Editor::from_text(PathBuf::from("test.asm"), text)
    }

    #[test]
    fn insert_and_undo() {
        let mut e = editor("mov ax, bx\n");
        e.cur = Pos { line: 0, col: 3 };
        e.snapshot(EditKind::Typing);
        e.insert("zx");
        assert_eq!(e.text(), "movzx ax, bx\n");
        e.undo();
        assert_eq!(e.text(), "mov ax, bx\n");
    }

    #[test]
    fn newline_keeps_indent() {
        let mut e = editor("        nop");
        e.cur = Pos { line: 0, col: 11 };
        e.newline();
        assert_eq!(e.text(), "        nop\n        \n");
        assert_eq!(e.cur, Pos { line: 1, col: 8 });
    }

    #[test]
    fn tabs_expand_and_selection_deletes() {
        let mut e = editor("a\tb\nline2");
        assert_eq!(e.lines[0].iter().collect::<String>(), "a       b");
        e.anchor = Some(Pos { line: 0, col: 1 });
        e.cur = Pos { line: 1, col: 4 };
        e.delete_selection();
        assert_eq!(e.text(), "a2\n");
    }
}
