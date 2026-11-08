//! The Rustland R++ desktop: menu bar, edit window, message window, status
//! line, dialogs, and the compile/run commands.

use alloc::boxed::Box;
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

use crate::edit::Editor;
use crate::rt::{self, Key};
use crate::scr::{self, Screen, H, W};

// Scan codes.
const K_ESC: u8 = 0x01;
const K_BKSP: u8 = 0x0E;
const K_TAB: u8 = 0x0F;
const K_ENTER: u8 = 0x1C;
const K_F1: u8 = 0x3B;
const K_F2: u8 = 0x3C;
const K_F3: u8 = 0x3D;
const K_F5: u8 = 0x3F;
const K_F6: u8 = 0x40;
const K_F9: u8 = 0x43;
const K_F10: u8 = 0x44;
const K_HOME: u8 = 0x47;
const K_UP: u8 = 0x48;
const K_PGUP: u8 = 0x49;
const K_LEFT: u8 = 0x4B;
const K_RIGHT: u8 = 0x4D;
const K_END: u8 = 0x4F;
const K_DOWN: u8 = 0x50;
const K_PGDN: u8 = 0x51;
const K_INS: u8 = 0x52;
const K_DEL: u8 = 0x53;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Cmd {
    New,
    Open,
    Save,
    SaveAs,
    DosShell,
    Exit,
    Undo,
    Cut,
    Copy,
    Paste,
    Clear,
    SelectAll,
    DeleteLine,
    Find,
    FindNext,
    GotoLine,
    Run,
    UserScreen,
    Args,
    Compile,
    Make,
    Info,
    Messages,
    NextError,
    Help,
    About,
    None,
}

struct Menu {
    title: &'static str,
    hot: u8,
    items: &'static [(&'static str, &'static str, Cmd)],
}

const MENUS: &[Menu] = &[
    Menu {
        title: "~F~ile",
        hot: 0x21,
        items: &[
            ("~N~ew", "", Cmd::New),
            ("~O~pen...", "F3", Cmd::Open),
            ("~S~ave", "F2", Cmd::Save),
            ("Save ~a~s...", "", Cmd::SaveAs),
            ("-", "", Cmd::None),
            ("~D~OS shell", "", Cmd::DosShell),
            ("E~x~it", "Alt+X", Cmd::Exit),
        ],
    },
    Menu {
        title: "~E~dit",
        hot: 0x12,
        items: &[
            ("~U~ndo", "Alt+BkSp", Cmd::Undo),
            ("-", "", Cmd::None),
            ("Cu~t~", "Shift+Del", Cmd::Cut),
            ("~C~opy", "Ctrl+Ins", Cmd::Copy),
            ("~P~aste", "Shift+Ins", Cmd::Paste),
            ("C~l~ear", "Ctrl+Del", Cmd::Clear),
            ("-", "", Cmd::None),
            ("Select ~a~ll", "Ctrl+A", Cmd::SelectAll),
            ("~D~elete line", "Ctrl+Y", Cmd::DeleteLine),
        ],
    },
    Menu {
        title: "~S~earch",
        hot: 0x1F,
        items: &[
            ("~F~ind...", "Ctrl+F", Cmd::Find),
            ("~S~earch again", "Ctrl+L", Cmd::FindNext),
            ("~G~o to line number...", "Ctrl+G", Cmd::GotoLine),
            ("~N~ext error", "F6", Cmd::NextError),
        ],
    },
    Menu {
        title: "~R~un",
        hot: 0x13,
        items: &[
            ("~R~un", "Ctrl+F9", Cmd::Run),
            ("~U~ser screen", "Alt+F5", Cmd::UserScreen),
            ("~A~rguments...", "", Cmd::Args),
        ],
    },
    Menu {
        title: "~C~ompile",
        hot: 0x2E,
        items: &[
            ("~C~ompile", "Alt+F9", Cmd::Compile),
            ("~M~ake", "F9", Cmd::Make),
            ("-", "", Cmd::None),
            ("~I~nformation...", "", Cmd::Info),
        ],
    },
    Menu {
        title: "~W~indow",
        hot: 0x11,
        items: &[("~M~essage", "", Cmd::Messages), ("~U~ser screen", "Alt+F5", Cmd::UserScreen)],
    },
    Menu {
        title: "~H~elp",
        hot: 0x23,
        items: &[("~C~ontents", "F1", Cmd::Help), ("~A~bout...", "", Cmd::About)],
    },
];

struct Msg {
    text: String,
    pos: Option<(usize, usize)>,
}

pub struct App {
    ed: Editor,
    s: Box<Screen>,
    clipboard: Vec<u8>,
    msgs: Vec<Msg>,
    msg_sel: usize,
    show_msgs: bool,
    built: Option<u32>,
    args: String,
    find: String,
    user_screen: Option<Box<[u16; W * H]>>,
    last_exit: Option<u8>,
    quit: bool,
}

fn exe_name(src: &str) -> String {
    let base = match src.rfind('.') {
        Some(i) if !src[i..].contains('\\') => &src[..i],
        _ => src,
    };
    format!("{base}.EXE")
}

fn plain_len(s: &str) -> usize {
    s.bytes().filter(|&c| c != b'~').count()
}

fn hot_letter(s: &str) -> Option<u8> {
    let i = s.find('~')?;
    s.as_bytes().get(i + 1).map(|c| c.to_ascii_lowercase())
}

impl App {
    pub fn new() -> App {
        App {
            ed: Editor::new("NONAME00.RS"),
            s: Box::new(Screen::new()),
            clipboard: Vec::new(),
            msgs: Vec::new(),
            msg_sel: 0,
            show_msgs: false,
            built: None,
            args: String::new(),
            find: String::new(),
            user_screen: None,
            last_exit: None,
            quit: false,
        }
    }

    pub fn open_path(&mut self, path: &str) {
        let mut name = path.to_ascii_uppercase();
        if !name.contains('.') {
            name.push_str(".RS");
        }
        match rt::read_file(&name) {
            Some(data) => self.ed.load(&data),
            None => self.ed.load(b""),
        }
        self.ed.filename = name;
        self.built = None;
        self.msgs.clear();
        self.show_msgs = false;
    }

    pub fn run(&mut self) {
        rt::set_text_mode();
        while !self.quit {
            self.draw();
            self.s.flush();
            self.place_cursor();
            let k = rt::read_key();
            self.key(k);
            // Handle keys typed while we were busy before redrawing, so fast
            // typing (or pasted input) never overflows the BIOS buffer.
            while !self.quit && rt::key_waiting() {
                let k = rt::read_key();
                self.key(k);
            }
        }
        rt::set_text_mode();
    }

    // ------------------------------------------------------------ layout

    /// The edit window frame: (y, h).
    fn edit_rect(&self) -> (usize, usize) {
        if self.show_msgs {
            (1, 17)
        } else {
            (1, 23)
        }
    }

    fn view_size(&self) -> (usize, usize) {
        let (_, h) = self.edit_rect();
        (W - 2, h - 2)
    }

    fn place_cursor(&mut self) {
        let (y, _) = self.edit_rect();
        let (w, h) = self.view_size();
        let (cl, cc) = (self.ed.cur.line, self.ed.cur.col);
        if cl >= self.ed.top && cl < self.ed.top + h && cc >= self.ed.left && cc < self.ed.left + w {
            rt::cursor_shape(if self.ed.insert { 0 } else { 1 });
            rt::set_cursor((cc - self.ed.left + 1) as u32, (cl - self.ed.top + y + 1) as u32);
        } else {
            rt::cursor_shape(2);
        }
    }

    // ------------------------------------------------------------ drawing

    fn draw(&mut self) {
        let (vw, vh) = self.view_size();
        self.ed.scroll_into_view(vw, vh);
        self.s.fill(0, 1, W, H - 2, 0xB1, scr::DESKTOP);
        self.draw_menu_bar(None);
        self.draw_edit_window();
        if self.show_msgs {
            self.draw_messages();
        }
        self.draw_status(" ~F1~ Help  ~F2~ Save  ~F3~ Open  ~Alt-F9~ Compile  ~F9~ Make  ~Ctrl-F9~ Run  ~F10~ Menu");
    }

    fn draw_status(&mut self, text: &str) {
        self.s.fill(0, H - 1, W, 1, b' ', scr::MENU);
        self.s.hot_text(0, H - 1, text, scr::MENU, scr::MENU_HOT);
    }

    fn menu_x(i: usize) -> usize {
        let mut x = 5;
        for m in &MENUS[..i] {
            x += plain_len(m.title) + 2;
        }
        x
    }

    fn draw_menu_bar(&mut self, sel: Option<usize>) {
        self.s.fill(0, 0, W, 1, b' ', scr::MENU);
        self.s.put(2, 0, 0xF0, scr::MENU);
        for (i, m) in MENUS.iter().enumerate() {
            let x = Self::menu_x(i);
            let (a, h) = if sel == Some(i) { (scr::MENU_SEL, scr::MENU_SEL_HOT) } else { (scr::MENU, scr::MENU_HOT) };
            self.s.put(x - 1, 0, b' ', a);
            let e = self.s.hot_text(x, 0, m.title, a, h);
            self.s.put(e, 0, b' ', a);
        }
        let clock = format!("{} free", fmt_kb(rt::heap_free()));
        self.s.text(W - clock.len() - 1, 0, &clock, scr::MENU);
    }

    fn draw_edit_window(&mut self) {
        let (y, h) = self.edit_rect();
        let f = scr::EDIT_FRAME;
        self.s.frame(0, y, W, h, f, true, None);
        // Close box, title, window number and zoom box.
        self.s.text(2, y, "[", f);
        self.s.put(3, y, 0xFE, scr::EDIT_FRAME_ICON);
        self.s.text(4, y, "]", f);
        let title = format!(" {} ", self.ed.filename);
        self.s.text((W - title.len()) / 2, y, &title, f);
        self.s.text(W - 8, y, "1", f);
        self.s.text(W - 6, y, "[", f);
        self.s.put(W - 5, y, 0x12, scr::EDIT_FRAME_ICON);
        self.s.text(W - 4, y, "]", f);
        let (vw, vh) = self.view_size();
        self.ed.draw(&mut self.s, 1, y + 1, vw, vh);
        // Status in the bottom frame.
        let by = y + h - 1;
        if self.ed.modified {
            self.s.put(2, by, b'*', f);
        }
        let pos = format!(" {}:{} ", self.ed.cur.line + 1, self.ed.cur.col + 1);
        self.s.text(5, by, &pos, f);
        if !self.ed.insert {
            self.s.text(16, by, " Overwrite ", f);
        }
        // Vertical scroll bar.
        let (top, bot) = (y + 1, y + h - 2);
        self.s.put(W - 1, top, 0x1E, scr::SCROLL);
        self.s.put(W - 1, bot, 0x1F, scr::SCROLL);
        for j in top + 1..bot {
            self.s.put(W - 1, j, 0xB1, scr::SCROLL);
        }
        let span = bot - top - 1;
        if span > 0 {
            let n = self.ed.lines.len().max(1);
            let t = top + 1 + (self.ed.cur.line * (span - 1)) / n;
            self.s.put(W - 1, t.min(bot - 1), 0xFE, scr::SCROLL);
        }
        // Horizontal scroll bar.
        let (l, r) = (30, W - 2);
        self.s.put(l, by, 0x11, scr::SCROLL);
        self.s.put(r, by, 0x10, scr::SCROLL);
        for i in l + 1..r {
            self.s.put(i, by, 0xB1, scr::SCROLL);
        }
        let hx = l + 1 + (self.ed.left.min(r - l - 2));
        self.s.put(hx, by, 0xFE, scr::SCROLL);
    }

    fn draw_messages(&mut self) {
        let (y, h) = (18, 6);
        self.s.frame(0, y, W, h, scr::MSG_FRAME, false, Some(scr::MSG_TEXT));
        self.s.text((W - 9) / 2, y, " Message ", scr::MSG_FRAME);
        self.s.text(W - 4, y, "2", scr::MSG_FRAME);
        let first = self.msg_sel.saturating_sub(h - 3);
        for (row, m) in self.msgs.iter().enumerate().skip(first).take(h - 2) {
            let attr = if row == self.msg_sel && m.pos.is_some() { scr::MSG_SEL } else { scr::MSG_TEXT };
            let t: String = m.text.chars().take(W - 4).collect();
            if attr != scr::MSG_TEXT {
                self.s.fill(1, y + 1 + row - first, W - 2, 1, b' ', attr);
            }
            self.s.text(2, y + 1 + row - first, &t, attr);
        }
    }

    // ------------------------------------------------------------ keys

    fn key(&mut self, k: Key) {
        let (vw, vh) = self.view_size();
        let _ = vw;
        // Global shortcuts.
        let cmd = match (k.scan, k.alt, k.ctrl, k.shift) {
            (K_F10, false, false, false) => {
                let c = self.menu_loop(0);
                self.exec(c);
                return;
            }
            (K_F1, ..) => Cmd::Help,
            (K_F2, false, false, _) => Cmd::Save,
            (K_F3, false, false, _) => Cmd::Open,
            (K_F5, true, ..) => Cmd::UserScreen,
            (K_F6, ..) => Cmd::NextError,
            (K_F9, true, ..) => Cmd::Compile,
            (K_F9, _, true, _) => Cmd::Run,
            (K_F9, false, false, _) => Cmd::Make,
            (0x2D, true, ..) => Cmd::Exit,
            (K_BKSP, true, ..) => Cmd::Undo,
            (K_INS, _, true, _) => Cmd::Copy,
            (K_INS, _, _, true) => Cmd::Paste,
            (K_DEL, _, _, true) => Cmd::Cut,
            (K_DEL, _, true, _) => Cmd::Clear,
            _ => Cmd::None,
        };
        if cmd != Cmd::None {
            self.exec(cmd);
            return;
        }
        if k.alt {
            if let Some(i) = MENUS.iter().position(|m| m.hot == k.scan) {
                let c = self.menu_loop(i);
                self.exec(c);
            }
            return;
        }
        if k.ctrl && (1..=26).contains(&k.ascii) {
            let c = match k.ascii + b'A' - 1 {
                b'A' => Cmd::SelectAll,
                b'C' => Cmd::Copy,
                b'F' => Cmd::Find,
                b'G' => Cmd::GotoLine,
                b'L' => Cmd::FindNext,
                b'S' => Cmd::Save,
                b'V' => Cmd::Paste,
                b'X' => Cmd::Cut,
                b'Y' => Cmd::DeleteLine,
                b'Z' => Cmd::Undo,
                _ => Cmd::None,
            };
            self.exec(c);
            return;
        }
        let ed = &mut self.ed;
        let nav = matches!(k.scan, K_UP | K_DOWN | K_LEFT | K_RIGHT | K_HOME | K_END | K_PGUP | K_PGDN) && k.ascii == 0;
        if nav {
            ed.begin_move(k.shift);
            match (k.scan, k.ctrl) {
                (K_UP, _) => ed.up(1),
                (K_DOWN, _) => ed.down(1),
                (K_LEFT, false) => ed.left(),
                (K_LEFT, true) => ed.word_left(),
                (K_RIGHT, false) => ed.right(),
                (K_RIGHT, true) => ed.word_right(),
                (K_HOME, false) => ed.home(),
                (K_END, false) => ed.end(),
                (K_HOME, true) | (K_PGUP, true) => ed.doc_start(),
                (K_END, true) | (K_PGDN, true) => ed.doc_end(),
                (K_PGUP, false) => {
                    ed.up(vh - 1);
                    ed.top = ed.top.saturating_sub(vh - 1);
                }
                _ => {
                    ed.down(vh - 1);
                    ed.top = (ed.top + vh - 1).min(ed.lines.len().saturating_sub(1));
                }
            }
            return;
        }
        match k.scan {
            K_INS if k.ascii == 0 => ed.insert = !ed.insert,
            K_DEL if k.ascii == 0 => ed.delete(),
            K_BKSP => ed.backspace(),
            K_ENTER => ed.newline(),
            K_TAB if !k.shift => ed.tab(),
            K_ESC => ed.anchor = None,
            _ => {
                if k.ascii >= 0x20 && k.ascii != 0x7F {
                    ed.type_char(k.ascii);
                }
            }
        }
    }

    fn exec(&mut self, c: Cmd) {
        match c {
            Cmd::None => {}
            Cmd::New => {
                if self.confirm_discard() {
                    self.ed = Editor::new("NONAME00.RS");
                    self.built = None;
                    self.msgs.clear();
                    self.show_msgs = false;
                }
            }
            Cmd::Open => {
                if self.confirm_discard() {
                    if let Some(name) = self.file_dialog("Open a File", "*.RS") {
                        self.open_path(&name);
                    }
                }
            }
            Cmd::Save => {
                self.save();
            }
            Cmd::SaveAs => {
                self.save_as();
            }
            Cmd::DosShell => self.dos_shell(),
            Cmd::Exit => {
                if self.confirm_discard() {
                    self.quit = true;
                }
            }
            Cmd::Undo => {
                self.ed.undo();
            }
            Cmd::Cut => {
                if let Some(t) = self.ed.selected_text() {
                    self.clipboard = t;
                    self.ed.delete_selection();
                }
            }
            Cmd::Copy => {
                if let Some(t) = self.ed.selected_text() {
                    self.clipboard = t;
                }
            }
            Cmd::Paste => {
                let t = self.clipboard.clone();
                if !t.is_empty() {
                    self.ed.insert_text(&t);
                }
            }
            Cmd::Clear => {
                self.ed.delete_selection();
            }
            Cmd::SelectAll => self.ed.select_all(),
            Cmd::DeleteLine => self.ed.delete_line(),
            Cmd::Find => {
                if let Some(t) = self.input_box("Find", "~T~ext to find", &self.find.clone(), 40) {
                    self.find = t;
                    self.find_next();
                }
            }
            Cmd::FindNext => self.find_next(),
            Cmd::GotoLine => {
                if let Some(t) = self.input_box("Go to Line Number", "Enter new line ~n~umber", "", 8) {
                    if let Ok(n) = t.trim().parse::<usize>() {
                        self.ed.goto(n.max(1) - 1, 0);
                    }
                }
            }
            Cmd::Run => self.run_program(),
            Cmd::UserScreen => self.user_screen(),
            Cmd::Args => {
                if let Some(t) = self.input_box("Program Arguments", "~A~rguments", &self.args.clone(), 50) {
                    self.args = t;
                }
            }
            Cmd::Compile | Cmd::Make => {
                self.compile(true);
            }
            Cmd::Info => self.info(),
            Cmd::Messages => self.show_msgs = !self.show_msgs,
            Cmd::NextError => self.next_error(),
            Cmd::Help => {
                self.draw();
                crate::help::show(&mut self.s);
            }
            Cmd::About => {
                self.message_box(
                    "About",
                    &[
                        "",
                        "Rustland R++",
                        "Version 1.0",
                        "",
                        "An IDE for R++, a subset of Rust,",
                        "running in 32-bit protected mode",
                        "on TinyDOS and rusty486.",
                    ],
                    &["  ~O~K  "],
                    scr::DLG,
                );
            }
        }
    }

    // ------------------------------------------------------------ menus

    /// Run the pull-down menus starting at menu `i`; returns the chosen command.
    fn menu_loop(&mut self, mut i: usize) -> Cmd {
        let mut item = 0;
        loop {
            self.draw();
            self.draw_menu_bar(Some(i));
            let m = &MENUS[i];
            let w = m.items.iter().map(|(l, k, _)| plain_len(l) + k.len() + 6).max().unwrap_or(10).max(16);
            let x = (Self::menu_x(i) - 1).min(W - w - 2);
            let h = m.items.len() + 2;
            self.s.frame(x, 1, w, h, scr::MENU, false, Some(scr::MENU));
            self.s.shadow(x, 1, w, h);
            for (j, (label, keys, _)) in m.items.iter().enumerate() {
                let y = 2 + j;
                if *label == "-" {
                    self.s.put(x, y, 0xC3, scr::MENU);
                    self.s.fill(x + 1, y, w - 2, 1, 0xC4, scr::MENU);
                    self.s.put(x + w - 1, y, 0xB4, scr::MENU);
                    continue;
                }
                let (a, hot) = if j == item { (scr::MENU_SEL, scr::MENU_SEL_HOT) } else { (scr::MENU, scr::MENU_HOT) };
                self.s.fill(x + 1, y, w - 2, 1, b' ', a);
                self.s.hot_text(x + 2, y, label, a, hot);
                self.s.text(x + w - 2 - keys.len(), y, keys, a);
            }
            let hint = match m.items[item].2 {
                Cmd::Run => " Run the program, compiling it first if needed",
                Cmd::Compile => " Compile the source file to an .EXE",
                Cmd::Open => " Open a source file",
                Cmd::Exit => " Leave Rustland R++",
                _ => " ~Esc~ Close menu  ~Enter~ Select",
            };
            self.draw_status(hint);
            self.s.flush();
            rt::cursor_shape(2);
            let k = rt::read_key();
            let n = m.items.len();
            match k.scan {
                K_ESC => return Cmd::None,
                K_LEFT => {
                    i = (i + MENUS.len() - 1) % MENUS.len();
                    item = 0;
                }
                K_RIGHT => {
                    i = (i + 1) % MENUS.len();
                    item = 0;
                }
                K_UP => loop {
                    item = (item + n - 1) % n;
                    if m.items[item].0 != "-" {
                        break;
                    }
                },
                K_DOWN => loop {
                    item = (item + 1) % n;
                    if m.items[item].0 != "-" {
                        break;
                    }
                },
                K_HOME => item = 0,
                K_END => item = n - 1,
                K_ENTER => return m.items[item].2,
                _ => {
                    if k.alt {
                        if let Some(j) = MENUS.iter().position(|mm| mm.hot == k.scan) {
                            i = j;
                            item = 0;
                        }
                        continue;
                    }
                    let c = k.ascii.to_ascii_lowercase();
                    if let Some((_, _, cmd)) = m.items.iter().find(|(l, _, _)| hot_letter(l) == Some(c)) {
                        return *cmd;
                    }
                }
            }
        }
    }

    // ------------------------------------------------------------ dialogs

    /// A message box; returns the chosen button, or None for Esc.
    fn message_box(&mut self, title: &str, lines: &[&str], buttons: &[&str], attr: u8) -> Option<usize> {
        let mut focus = 0;
        let bw: usize = buttons.iter().map(|b| plain_len(b) + 4).sum();
        let w = lines.iter().map(|l| l.len()).max().unwrap_or(0).max(bw).max(title.len() + 6) + 6;
        let h = lines.len() + 5;
        let (x, y) = ((W - w) / 2, (H - h) / 2);
        loop {
            self.draw();
            self.s.dialog(x, y, w, h, title, attr);
            for (j, l) in lines.iter().enumerate() {
                self.s.text(x + (w - l.len()) / 2, y + 1 + j, l, attr);
            }
            let mut bx = x + (w - bw) / 2 + 1;
            for (j, b) in buttons.iter().enumerate() {
                self.s.button(bx, y + h - 3, b, j == focus);
                bx += plain_len(b) + 4;
            }
            self.s.flush();
            rt::cursor_shape(2);
            let k = rt::read_key();
            match k.scan {
                K_ESC => return None,
                K_ENTER => return Some(focus),
                K_TAB | K_RIGHT => focus = (focus + 1) % buttons.len(),
                K_LEFT => focus = (focus + buttons.len() - 1) % buttons.len(),
                _ => {
                    let c = k.ascii.to_ascii_lowercase();
                    if let Some(j) = buttons.iter().position(|b| hot_letter(b) == Some(c)) {
                        return Some(j);
                    }
                }
            }
        }
    }

    fn error_box(&mut self, text: &str) {
        self.message_box("Error", &["", text], &["  ~O~K  "], scr::ERROR_DLG);
    }

    /// A one-line input dialog.
    fn input_box(&mut self, title: &str, label: &str, init: &str, width: usize) -> Option<String> {
        let mut text = String::from(init);
        let w = (width + 8).max(plain_len(label) + 8).max(36);
        let h = 8;
        let (x, y) = ((W - w) / 2, (H - h) / 2);
        let mut fresh = true;
        loop {
            self.draw();
            self.s.dialog(x, y, w, h, title, scr::DLG);
            self.s.hot_text(x + 3, y + 2, label, scr::DLG, 0x7E);
            let fw = w - 8;
            self.s.fill(x + 3, y + 3, fw + 1, 1, b' ', scr::INPUT);
            let shown: String = text.chars().rev().take(fw).collect::<Vec<_>>().into_iter().rev().collect();
            self.s.text(x + 4, y + 3, &shown, if fresh && !text.is_empty() { 0x2F } else { scr::INPUT });
            self.s.button(x + w / 2 - 12, y + 5, "  ~O~K  ", true);
            self.s.button(x + w / 2 + 2, y + 5, " Cancel ", false);
            self.s.flush();
            rt::cursor_shape(0);
            rt::set_cursor((x + 4 + shown.len()) as u32, (y + 3) as u32);
            let k = rt::read_key();
            match k.scan {
                K_ESC => return None,
                K_ENTER => return Some(text),
                K_BKSP => {
                    if fresh {
                        text.clear();
                    } else {
                        text.pop();
                    }
                }
                _ => {
                    if k.ascii >= 0x20 && k.ascii < 0x7F && !k.ctrl && !k.alt {
                        if fresh {
                            text.clear();
                        }
                        if text.len() < 120 {
                            text.push(k.ascii as char);
                        }
                    }
                }
            }
            fresh = false;
        }
    }

    /// A file dialog: a name field and the matching files.
    fn file_dialog(&mut self, title: &str, pattern: &str) -> Option<String> {
        let files = rt::list_files(pattern);
        let dir = rt::current_dir();
        let mut name = String::from(pattern);
        let mut sel = 0usize;
        let mut top = 0usize;
        let (w, h) = (52, 18);
        let (x, y) = ((W - w) / 2, (H - h) / 2);
        let rows = h - 8;
        loop {
            self.draw();
            self.s.dialog(x, y, w, h, title, scr::DLG);
            self.s.hot_text(x + 3, y + 2, "~N~ame", scr::DLG, 0x7E);
            self.s.fill(x + 3, y + 3, 30, 1, b' ', scr::INPUT);
            self.s.text(x + 4, y + 3, &name, scr::INPUT);
            self.s.hot_text(x + 3, y + 5, "~F~iles", scr::DLG, 0x7E);
            self.s.fill(x + 3, y + 6, 30, rows, b' ', scr::LIST);
            if sel < top {
                top = sel;
            }
            if sel >= top + rows {
                top = sel + 1 - rows;
            }
            for (j, f) in files.iter().enumerate().skip(top).take(rows) {
                let a = if j == sel { scr::LIST_SEL } else { scr::LIST };
                self.s.fill(x + 3, y + 6 + j - top, 30, 1, b' ', a);
                self.s.text(x + 4, y + 6 + j - top, f, a);
            }
            if files.is_empty() {
                self.s.text(x + 4, y + 6, "(no files)", scr::LIST);
            }
            self.s.button(x + 37, y + 3, "  ~O~pen  ", true);
            self.s.button(x + 37, y + 6, " Cancel  ", false);
            let d: String = dir.chars().take(w - 6).collect();
            self.s.text(x + 3, y + h - 2, &d, scr::DLG);
            self.s.flush();
            rt::cursor_shape(0);
            rt::set_cursor((x + 4 + name.len()) as u32, (y + 3) as u32);
            let k = rt::read_key();
            match k.scan {
                K_ESC => return None,
                K_ENTER => {
                    let n = String::from(name.trim());
                    if n.contains('*') || n.contains('?') {
                        if let Some(f) = files.get(sel) {
                            return Some(f.clone());
                        }
                        continue;
                    }
                    if n.is_empty() {
                        continue;
                    }
                    return Some(n);
                }
                K_UP if !files.is_empty() => {
                    sel = sel.saturating_sub(1);
                    name = files[sel].clone();
                }
                K_DOWN if !files.is_empty() => {
                    sel = (sel + 1).min(files.len() - 1);
                    name = files[sel].clone();
                }
                K_BKSP => {
                    name.pop();
                }
                _ => {
                    if k.ascii > 0x20 && k.ascii < 0x7F && !k.ctrl && !k.alt && name.len() < 28 {
                        if name.contains('*') {
                            name.clear();
                        }
                        name.push(k.ascii.to_ascii_uppercase() as char);
                    }
                }
            }
        }
    }

    /// "Save changes?" for a modified file. Returns false to cancel.
    fn confirm_discard(&mut self) -> bool {
        if !self.ed.modified {
            return true;
        }
        let msg = format!("{} has been modified. Save?", self.ed.filename);
        match self.message_box("Information", &["", &msg], &["  ~Y~es  ", "  ~N~o  ", " Cancel "], scr::DLG) {
            Some(0) => self.save(),
            Some(1) => true,
            _ => false,
        }
    }

    // ------------------------------------------------------------ files

    fn save(&mut self) -> bool {
        if self.ed.filename.starts_with("NONAME") {
            return self.save_as();
        }
        let data = self.ed.save_bytes();
        if rt::write_file(&self.ed.filename, &data) {
            self.ed.modified = false;
            true
        } else {
            let m = format!("Cannot write {}", self.ed.filename);
            self.error_box(&m);
            false
        }
    }

    fn save_as(&mut self) -> bool {
        let init = self.ed.filename.clone();
        match self.input_box("Save File As", "~S~ave file as", &init, 40) {
            Some(n) if !n.trim().is_empty() => {
                let mut n = n.trim().to_ascii_uppercase();
                if !n.contains('.') {
                    n.push_str(".RS");
                }
                self.ed.filename = n;
                self.save()
            }
            _ => false,
        }
    }

    fn find_next(&mut self) {
        let f = self.find.clone();
        if !f.is_empty() && !self.ed.find(&f) {
            let m = format!("Search string not found: '{f}'");
            self.message_box("Information", &["", &m], &["  ~O~K  "], scr::DLG);
        }
    }

    // ------------------------------------------------------------ compile and run

    /// Compile the edit buffer to an .EXE. With `wait`, the result box
    /// waits for a key even on success.
    fn compile(&mut self, wait: bool) -> bool {
        let exe = exe_name(&self.ed.filename);
        let src = self.ed.text();
        let lines = self.ed.lines.len();
        self.compile_box(&exe, lines, "Compiling", None);
        let result = rusty486_lang::compile(&src);
        self.msgs.clear();
        self.msgs.push(Msg { text: format!("Compiling {}:", self.ed.filename), pos: None });
        let ok = match result {
            Ok(bytes) => {
                if rt::write_file(&exe, &bytes) {
                    self.built = Some(self.ed.version);
                    self.msgs.push(Msg { text: format!("Success: {} ({} bytes)", exe, bytes.len()), pos: None });
                    true
                } else {
                    self.msgs.push(Msg { text: format!("Error: cannot write {exe}"), pos: None });
                    false
                }
            }
            Err(e) => {
                let pos = if e.line >= 1 && e.line < 100_000 { Some((e.line as usize - 1, e.col.max(1) as usize - 1)) } else { None };
                let text = match pos {
                    Some(_) => format!("Error {} {}: {}", self.ed.filename, e.line, e.msg),
                    None => format!("Error: {e}"),
                };
                self.msgs.push(Msg { text, pos });
                false
            }
        };
        self.msg_sel = if ok { 0 } else { 1 };
        if !ok {
            self.show_msgs = true;
            self.compile_box(&exe, lines, "Errors", Some(false));
            rt::read_key();
            self.next_error_at(1);
        } else if wait {
            self.compile_box(&exe, lines, "Success", Some(true));
            rt::read_key();
        }
        ok
    }

    fn compile_box(&mut self, exe: &str, lines: usize, state: &str, done: Option<bool>) {
        self.draw();
        let (w, h) = (44, 11);
        let (x, y) = ((W - w) / 2, (H - h) / 2);
        self.s.dialog(x, y, w, h, "Compiling", scr::DLG);
        let main = format!("Main file: {}", self.ed.filename);
        self.s.text(x + 3, y + 2, &main, scr::DLG);
        let out = format!("{state:>9}: {exe}");
        self.s.text(x + 3, y + 3, &out, scr::DLG);
        let total = format!("Total    Link");
        self.s.text(x + 20, y + 5, &total, scr::DLG);
        let l = format!("Lines:  {lines:>9}");
        self.s.text(x + 3, y + 6, &l, scr::DLG);
        let errs = match done {
            Some(false) => 1,
            _ => 0,
        };
        let e = format!("Errors: {errs:>9}");
        self.s.text(x + 3, y + 7, &e, scr::DLG);
        if let Some(ok) = done {
            let msg = if ok { "Success  :  Press any key" } else { "Errors   :  Press any key" };
            let a = if ok { 0x7A } else { 0x74 };
            self.s.fill(x + 1, y + h - 2, w - 2, 1, b' ', scr::DLG);
            self.s.text(x + (w - msg.len()) / 2, y + h - 2, msg, a);
        }
        self.s.flush();
        rt::cursor_shape(2);
    }

    fn next_error(&mut self) {
        if self.msgs.is_empty() {
            return;
        }
        let start = self.msg_sel + 1;
        let n = self.msgs.len();
        for k in 0..n {
            let i = (start + k) % n;
            if self.msgs[i].pos.is_some() {
                self.next_error_at(i);
                return;
            }
        }
    }

    fn next_error_at(&mut self, i: usize) {
        if let Some(Some((l, c))) = self.msgs.get(i).map(|m| m.pos) {
            self.msg_sel = i;
            self.show_msgs = true;
            self.ed.goto(l, c);
        }
    }

    fn run_program(&mut self) {
        if self.built != Some(self.ed.version) && !self.compile(false) {
            return;
        }
        let exe = exe_name(&self.ed.filename);
        rt::set_text_mode();
        rt::cursor_shape(0);
        let code = rt::exec(&exe, &self.args.clone());
        // Video mode from the BIOS data area.
        let mode = unsafe { core::ptr::read_volatile(0x449 as *const u8) };
        match code {
            Some(c) => {
                self.last_exit = Some(c);
                if mode == 3 {
                    rt::dos_print("\r\n");
                    self.user_screen = Some(Box::new(Screen::grab()));
                    // Show where the program ended until a key is pressed.
                    let note = format!(" Program exited with code {c}. Press any key to return to Rustland R++ ");
                    let mut s = Screen::grab();
                    for (i, b) in note.bytes().enumerate() {
                        s[24 * W + i] = 0x7000 | b as u16;
                    }
                    Screen::show(&s);
                }
                while rt::key_waiting() {
                    rt::read_key();
                }
                rt::read_key();
            }
            None => {
                rt::set_text_mode();
                let m = format!("Unable to execute {exe}");
                self.error_box(&m);
            }
        }
        rt::set_text_mode();
    }

    fn user_screen(&mut self) {
        match &self.user_screen {
            Some(s) => {
                Screen::show(s);
                rt::cursor_shape(2);
                rt::read_key();
            }
            None => {
                self.message_box("Information", &["", "No program has been run yet."], &["  ~O~K  "], scr::DLG);
            }
        }
    }

    fn dos_shell(&mut self) {
        rt::set_text_mode();
        rt::cursor_shape(0);
        rt::dos_print("Type EXIT to return to Rustland R++...\r\n");
        let shell = rt::getenv("COMSPEC").unwrap_or_else(|| String::from("\\COMMAND.COM"));
        if rt::exec(&shell, "").is_none() {
            rt::set_text_mode();
            let m = format!("Unable to execute {shell}");
            self.error_box(&m);
        }
        rt::set_text_mode();
    }

    fn info(&mut self) {
        let dir = format!("Current directory: {}", rt::current_dir());
        let file = format!("Current file:      {}", self.ed.filename);
        let lines = format!("Lines:             {}", self.ed.lines.len());
        let mem = format!("Memory free:       {}", fmt_kb(rt::heap_free()));
        let exit = match self.last_exit {
            Some(c) => format!("Last exit code:    {c}"),
            None => "Last exit code:    -".to_string(),
        };
        let built = format!("Program built:     {}", if self.built == Some(self.ed.version) { "up to date" } else { "no" });
        self.message_box(
            "Information",
            &["", &dir, &file, &lines, &mem, &built, &exit, ""],
            &["  ~O~K  "],
            scr::DLG,
        );
    }
}

fn fmt_kb(bytes: u32) -> String {
    format!("{}K", bytes / 1024)
}
