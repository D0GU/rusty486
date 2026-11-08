// WRITE.RS - R++ Write, a small formatted word processor.
// Part 1 is a tiny ncurses-style screen library, part 2 the editor.

// ===================================================== curses library

const A_BOLD: u32 = 0x100;
const A_REVERSE: u32 = 0x200;
const A_UNDERLINE: u32 = 0x400;
const A_ITALIC: u32 = 0x800;

const KEY_UP: i32 = 0x148;
const KEY_DOWN: i32 = 0x150;
const KEY_LEFT: i32 = 0x14B;
const KEY_RIGHT: i32 = 0x14D;
const KEY_HOME: i32 = 0x147;
const KEY_END: i32 = 0x14F;
const KEY_PPAGE: i32 = 0x149;
const KEY_NPAGE: i32 = 0x151;
const KEY_DC: i32 = 0x153;
const KEY_F1: i32 = 0x13B;
const KEY_F2: i32 = 0x13C;
const KEY_F3: i32 = 0x13D;

static mut SCR: [u16; 2000] = [0; 2000];
static mut PAIRS: [u8; 16] = [7; 16];
static mut CY: i32 = 0;
static mut CX: i32 = 0;
static mut ATTR: u32 = 0;

fn init_pair(n: u32, fg: u8, bg: u8) {
    unsafe { PAIRS[n as usize] = fg | (bg << 4); }
}

fn color_pair(n: u32) -> u32 {
    n
}

// A curses attribute to a VGA colour byte.
fn vga(a: u32) -> u8 {
    let c = unsafe { PAIRS[(a & 0xFF) as usize] };
    let mut fg = c & 0x0F;
    let bg = c >> 4;
    if a & A_UNDERLINE != 0 {
        fg = 3;
    }
    if a & A_ITALIC != 0 {
        fg = 2;
    }
    if a & A_BOLD != 0 {
        fg = fg | 8;
    }
    if a & A_REVERSE != 0 {
        return ((fg & 7) << 4) | bg;
    }
    fg | (bg << 4)
}

fn attrset(a: u32) {
    unsafe { ATTR = a; }
}

fn attron(a: u32) {
    unsafe { ATTR = ATTR | a; }
}

fn attroff(a: u32) {
    unsafe { ATTR = ATTR & !a; }
}

fn mv(y: i32, x: i32) {
    unsafe {
        CY = y;
        CX = x;
    }
}

fn addch(c: u8) {
    unsafe {
        if CY >= 0 && CY < 25 && CX >= 0 && CX < 80 {
            SCR[(CY * 80 + CX) as usize] = ((vga(ATTR) as u16) << 8) | (c as u16);
        }
        CX += 1;
    }
}

fn addstr(s: &str) {
    for c in s.bytes() {
        addch(c);
    }
}

fn mvaddstr(y: i32, x: i32, s: &str) {
    mv(y, x);
    addstr(s);
}

fn addnum(n: u32) {
    let mut d = [0u8; 10];
    let mut k = 0;
    let mut v = n;
    loop {
        d[k] = b'0' + (v % 10) as u8;
        k += 1;
        v = v / 10;
        if v == 0 {
            break;
        }
    }
    while k > 0 {
        k -= 1;
        addch(d[k]);
    }
}

fn hline(c: u8, n: i32) {
    for _ in 0..n {
        addch(c);
    }
}

// Fill the screen with the current attribute.
fn clear() {
    unsafe {
        let cell = ((vga(ATTR) as u16) << 8) | 32;
        for i in 0..2000 {
            SCR[i] = cell;
        }
    }
}

fn box_at(y: i32, x: i32, h: i32, w: i32) {
    mv(y, x);
    addch(0xDA);
    hline(0xC4, w - 2);
    addch(0xBF);
    for j in 1..h - 1 {
        mv(y + j, x);
        addch(0xB3);
        mv(y + j, x + w - 1);
        addch(0xB3);
    }
    mv(y + h - 1, x);
    addch(0xC0);
    hline(0xC4, w - 2);
    addch(0xD9);
}

fn curs_set(visible: bool) {
    let mut r = Regs::new();
    r.eax = 0x0100;
    r.ecx = if visible { 0x0607 } else { 0x2000 };
    dos_int(0x10, &mut r);
}

// Copy the screen buffer to video memory and place the cursor.
fn refresh() {
    let vram = 0xB8000 as *mut u16;
    unsafe {
        for i in 0..2000 {
            *vram.add(i) = SCR[i];
        }
        gotoxy(CX as u32, CY as u32);
    }
}

// A key: the ASCII code, or 0x100 + scan code for special keys.
fn getch() -> i32 {
    let k = read_key();
    let a = k & 0xFF;
    if a != 0 && a != 0xE0 {
        a as i32
    } else {
        0x100 + (k >> 8) as i32
    }
}

fn initscr() {
    set_video_mode(3);
    init_pair(0, 7, 0);
    attrset(0);
    clear();
}

fn endwin() {
    set_video_mode(3);
}

// ===================================================== the word processor

const MAXT: usize = 8000;
const PW: usize = 64;
const PX: i32 = 8;
const PY: i32 = 3;
const PH: usize = 19;

const S_BOLD: u8 = 1;
const S_ITALIC: u8 = 2;
const S_UNDER: u8 = 4;

const AL_LEFT: u8 = 0;
const AL_CENTER: u8 = 1;
const AL_RIGHT: u8 = 2;
const AL_JUSTIFY: u8 = 3;

// The document: characters and their styles. Every paragraph ends in '\n';
// the style byte of that '\n' holds the paragraph alignment.
static mut TEXT: [u8; 8000] = [0; 8000];
static mut STYLE: [u8; 8000] = [0; 8000];
static mut LEN: usize = 0;
static mut CUR: usize = 0;
static mut TOP: usize = 0;
static mut TYPING: u8 = 0;
static mut DIRTY: bool = false;
static mut MSG: &str = "";

// Screen lines after word wrapping.
static mut LSTART: [usize; 2000] = [0; 2000];
static mut LLEN: [usize; 2000] = [0; 2000];
static mut LLAST: [bool; 2000] = [false; 2000];
static mut LALIGN: [u8; 2000] = [0; 2000];
static mut NLINES: usize = 0;

fn para_end(p: usize) -> usize {
    let mut i = p;
    unsafe {
        while TEXT[i] != b'\n' {
            i += 1;
        }
    }
    i
}

fn add_line(s: usize, n: usize, last: bool, al: u8) {
    unsafe {
        if NLINES < 2000 {
            LSTART[NLINES] = s;
            LLEN[NLINES] = n;
            LLAST[NLINES] = last;
            LALIGN[NLINES] = al;
            NLINES += 1;
        }
    }
}

// Word-wrap every paragraph to the page width.
fn layout() {
    unsafe {
        NLINES = 0;
        let mut p: usize = 0;
        while p < LEN {
            let q = para_end(p);
            let al = STYLE[q] >> 4;
            let mut s = p;
            loop {
                if q - s <= PW {
                    add_line(s, q - s, true, al);
                    break;
                }
                let mut b = s + PW;
                while b > s && TEXT[b] != b' ' {
                    b -= 1;
                }
                if b == s {
                    add_line(s, PW, false, al);
                    s += PW;
                } else {
                    add_line(s, b - s, false, al);
                    s = b + 1;
                }
            }
            p = q + 1;
        }
    }
}

// The screen line holding text position `pos`.
fn line_of(pos: usize) -> usize {
    let mut i: usize = 0;
    unsafe {
        while i + 1 < NLINES {
            let end = LSTART[i] + LLEN[i];
            if pos < end || (pos == end && LSTART[i + 1] != pos) {
                break;
            }
            i += 1;
        }
    }
    i
}

fn spaces_in(i: usize) -> usize {
    let mut g = 0;
    unsafe {
        for k in 0..LLEN[i] {
            if TEXT[LSTART[i] + k] == b' ' {
                g += 1;
            }
        }
    }
    g
}

// Page column of character `col` of line `i`, after alignment.
fn line_x(i: usize, col: usize) -> usize {
    unsafe {
        let n = LLEN[i];
        match LALIGN[i] {
            AL_CENTER => (PW - n) / 2 + col,
            AL_RIGHT => PW - n + col,
            AL_JUSTIFY if !LLAST[i] => {
                let g = spaces_in(i);
                if g == 0 {
                    return col;
                }
                let extra = PW - n;
                let mut x = 0;
                let mut seen = 0;
                for k in 0..col {
                    x += 1;
                    if TEXT[LSTART[i] + k] == b' ' {
                        x += extra / g;
                        if seen < extra % g {
                            x += 1;
                        }
                        seen += 1;
                    }
                }
                x
            }
            _ => col,
        }
    }
}

fn style_attr(st: u8) -> u32 {
    let mut a = color_pair(3);
    if st & S_BOLD != 0 {
        a = a | A_BOLD;
    }
    if st & S_ITALIC != 0 {
        a = a | A_ITALIC;
    }
    if st & S_UNDER != 0 {
        a = a | A_UNDERLINE;
    }
    a
}

fn word_count() -> u32 {
    let mut n = 0;
    let mut in_word = false;
    unsafe {
        for i in 0..LEN {
            let w = TEXT[i] != b' ' && TEXT[i] != b'\n';
            if w && !in_word {
                n += 1;
            }
            in_word = w;
        }
    }
    n
}

fn align_name(a: u8) -> &'static str {
    match a {
        AL_CENTER => "Center ",
        AL_RIGHT => "Right  ",
        AL_JUSTIFY => "Justify",
        _ => "Left   ",
    }
}

fn draw() {
    unsafe {
        let li = line_of(CUR);
        if li < TOP {
            TOP = li;
        }
        if li >= TOP + PH {
            TOP = li + 1 - PH;
        }
        // Desktop, title bar and page frame.
        attrset(color_pair(1));
        clear();
        mv(0, 0);
        attrset(color_pair(2));
        hline(b' ', 80);
        mvaddstr(0, 2, "R++ Write");
        attrset(color_pair(4));
        mvaddstr(0, 13, "  a word processor for Rustland R++");
        attrset(color_pair(3));
        for y in PY - 1..PY + PH as i32 + 1 {
            mv(y, PX - 2);
            hline(b' ', PW as i32 + 4);
        }
        box_at(PY - 1, PX - 2, PH as i32 + 2, PW as i32 + 4);
        // Ruler in the top border: a tick every 8 columns.
        attrset(color_pair(7));
        for c in 0..PW as i32 {
            if c % 8 == 0 {
                mv(PY - 1, PX + c);
                addch(0xC2);
            }
        }
        mvaddstr(PY - 1, PX + 2, " DOC.WP ");
        // The text.
        for row in 0..PH {
            let i = TOP + row;
            if i >= NLINES {
                break;
            }
            let n = LLEN[i];
            for k in 0..n {
                let p = LSTART[i] + k;
                mv(PY + row as i32, PX + line_x(i, k) as i32);
                attrset(style_attr(STYLE[p]));
                addch(TEXT[p]);
            }
        }
        // Help line and status bar.
        attrset(color_pair(1));
        mvaddstr(23, 1, "^B Bold ^K Italic ^U Under  ^L ^E ^R ^J Align  F1 Help F2 Save F3 Open Esc Quit");
        attrset(color_pair(5));
        mv(24, 0);
        hline(b' ', 80);
        mvaddstr(24, 1, "Ln ");
        addnum(li as u32 + 1);
        addstr("/");
        addnum(NLINES as u32);
        addstr("  Col ");
        addnum((CUR - LSTART[li]) as u32 + 1);
        addstr("  Words ");
        addnum(word_count());
        mv(24, 34);
        if TYPING & S_BOLD != 0 { attrset(color_pair(6)); } else { attrset(color_pair(5)); }
        addstr("Bold ");
        if TYPING & S_ITALIC != 0 { attrset(color_pair(6)); } else { attrset(color_pair(5)); }
        addstr("Italic ");
        if TYPING & S_UNDER != 0 { attrset(color_pair(6)); } else { attrset(color_pair(5)); }
        addstr("Underline");
        attrset(color_pair(5));
        addstr("  ");
        addstr(align_name(STYLE[para_end(CUR)] >> 4));
        if DIRTY {
            addstr("  *");
        }
        mvaddstr(24, 67, MSG);
        // The cursor.
        mv(PY + (li - TOP) as i32, PX + line_x(li, CUR - LSTART[li]) as i32);
        refresh();
    }
}

// ----------------------------------------------------- editing

fn insert(c: u8, st: u8) {
    unsafe {
        if LEN >= MAXT {
            MSG = "Document full";
            return;
        }
        let mut i = LEN;
        while i > CUR {
            TEXT[i] = TEXT[i - 1];
            STYLE[i] = STYLE[i - 1];
            i -= 1;
        }
        TEXT[CUR] = c;
        STYLE[CUR] = st;
        LEN += 1;
        CUR += 1;
        DIRTY = true;
    }
}

fn delete_at(p: usize) {
    unsafe {
        // The final newline always stays.
        if p + 1 >= LEN {
            return;
        }
        for i in p..LEN - 1 {
            TEXT[i] = TEXT[i + 1];
            STYLE[i] = STYLE[i + 1];
        }
        LEN -= 1;
        DIRTY = true;
    }
}

fn new_paragraph() {
    unsafe {
        let al = STYLE[para_end(CUR)] & 0xF0;
        insert(b'\n', al);
    }
}

fn set_align(a: u8) {
    unsafe {
        let q = para_end(CUR);
        STYLE[q] = (STYLE[q] & 0x0F) | (a << 4);
        DIRTY = true;
    }
}

fn type_text(s: &str, st: u8) {
    for c in s.bytes() {
        if c == b'\n' {
            new_paragraph();
        } else {
            insert(c, st);
        }
    }
}

// New text takes the style of the character before the cursor.
fn pick_style() {
    unsafe {
        if CUR > 0 && TEXT[CUR - 1] != b'\n' {
            TYPING = STYLE[CUR - 1] & 7;
        }
    }
}

fn move_lines(d: i32) {
    unsafe {
        let li = line_of(CUR) as i32;
        let col = CUR - LSTART[li as usize];
        let mut t = li + d;
        if t < 0 {
            t = 0;
        }
        if t >= NLINES as i32 {
            t = NLINES as i32 - 1;
        }
        let t = t as usize;
        CUR = LSTART[t] + col.min(LLEN[t]);
    }
}

// ----------------------------------------------------- files and help

fn save() {
    unsafe {
        let mut buf = [0u8; 16000];
        for i in 0..LEN {
            buf[2 * i] = TEXT[i];
            buf[2 * i + 1] = STYLE[i];
        }
        if file_write("DOC.WP", &buf[..2 * LEN]) {
            DIRTY = false;
            MSG = "Saved DOC.WP";
        } else {
            MSG = "Save failed!";
        }
    }
}

fn load() {
    unsafe {
        let mut buf = [0u8; 16000];
        let n = file_read("DOC.WP", &mut buf);
        if n < 2 {
            MSG = "No DOC.WP yet";
            return;
        }
        LEN = n as usize / 2;
        for i in 0..LEN {
            TEXT[i] = buf[2 * i];
            STYLE[i] = buf[2 * i + 1];
        }
        CUR = 0;
        TOP = 0;
        DIRTY = false;
        MSG = "Opened DOC.WP";
    }
}

fn help() {
    attrset(color_pair(2));
    for y in 5..19 {
        mv(y, 18);
        hline(b' ', 44);
    }
    box_at(5, 18, 14, 44);
    mvaddstr(5, 34, " Help ");
    mvaddstr(7, 21, "Ctrl+B  Ctrl+K  Ctrl+U   bold/italic/");
    mvaddstr(8, 21, "                         underline");
    mvaddstr(9, 21, "Ctrl+L  Ctrl+E           left/centre");
    mvaddstr(10, 21, "Ctrl+R  Ctrl+J           right/justify");
    mvaddstr(11, 21, "Arrows Home End PgUp PgDn  move");
    mvaddstr(12, 21, "Enter  new paragraph   Tab  indent");
    mvaddstr(13, 21, "F2  save DOC.WP   F3  open DOC.WP");
    mvaddstr(14, 21, "Esc  quit");
    attron(A_BOLD);
    mvaddstr(16, 27, "Press any key to continue");
    refresh();
    getch();
}

fn ask_quit() -> bool {
    unsafe {
        if !DIRTY {
            return true;
        }
        MSG = "Save? Y/N/Esc";
        draw();
        match getch() {
            121 | 89 => {
                save();
                true
            }
            110 | 78 => true,
            _ => {
                MSG = "";
                false
            }
        }
    }
}

// Handle one key; returns true to quit.
fn handle(k: i32) -> bool {
    unsafe {
        MSG = "";
        match k {
            2 => TYPING = TYPING ^ S_BOLD,
            11 => TYPING = TYPING ^ S_ITALIC,
            21 => TYPING = TYPING ^ S_UNDER,
            12 => set_align(AL_LEFT),
            5 => set_align(AL_CENTER),
            18 => set_align(AL_RIGHT),
            10 => set_align(AL_JUSTIFY),
            13 => new_paragraph(),
            9 => type_text("    ", TYPING),
            8 => {
                if CUR > 0 {
                    CUR -= 1;
                    delete_at(CUR);
                }
            }
            27 => {
                if ask_quit() {
                    return true;
                }
            }
            KEY_DC => delete_at(CUR),
            KEY_LEFT => {
                if CUR > 0 {
                    CUR -= 1;
                }
                pick_style();
            }
            KEY_RIGHT => {
                if CUR + 1 < LEN {
                    CUR += 1;
                }
                pick_style();
            }
            KEY_UP => move_lines(-1),
            KEY_DOWN => move_lines(1),
            KEY_PPAGE => move_lines(-(PH as i32)),
            KEY_NPAGE => move_lines(PH as i32),
            KEY_HOME => CUR = LSTART[line_of(CUR)],
            KEY_END => {
                let li = line_of(CUR);
                CUR = LSTART[li] + LLEN[li];
            }
            KEY_F1 => help(),
            KEY_F2 => save(),
            KEY_F3 => load(),
            _ => {
                if k >= 32 && k < 127 {
                    insert(k as u8, TYPING);
                }
            }
        }
    }
    false
}

fn main() {
    initscr();
    init_pair(1, 8, 0);
    init_pair(2, 0, 7);
    init_pair(3, 7, 1);
    init_pair(4, 4, 7);
    init_pair(5, 0, 3);
    init_pair(6, 14, 3);
    init_pair(7, 11, 1);
    unsafe {
        TEXT[0] = b'\n';
        STYLE[0] = 0;
        LEN = 1;
    }
    // A sample document.
    set_align(AL_CENTER);
    type_text("R++ Write", S_BOLD);
    new_paragraph();
    type_text("a word processor written live in Rustland R++", S_ITALIC);
    new_paragraph();
    set_align(AL_LEFT);
    new_paragraph();
    set_align(AL_JUSTIFY);
    type_text("This paragraph is justified: the words are spread so that every line but the last fills the page from margin to margin. Text can be ", 0);
    type_text("bold", S_BOLD);
    type_text(", ", 0);
    type_text("italic", S_ITALIC);
    type_text(" or ", 0);
    type_text("underlined", S_UNDER);
    type_text(", and each paragraph has its own alignment.", 0);
    new_paragraph();
    set_align(AL_LEFT);
    new_paragraph();
    set_align(AL_RIGHT);
    type_text("Right-aligned, like a signature.", 0);
    unsafe {
        CUR = 0;
        DIRTY = false;
    }
    loop {
        layout();
        draw();
        let mut quit = handle(getch());
        // Keys typed while we were drawing: handle them all before the next
        // redraw so the 16-key BIOS buffer never overflows.
        while !quit && key_pressed() {
            layout();
            quit = handle(getch());
        }
        if quit {
            break;
        }
    }
    endwin();
    println!("Thanks for using R++ Write!");
}
