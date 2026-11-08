// WRITE.RS - R++ Write 2.0, a word processor for writing books.
//
// Part 1: a small ncurses-style screen library.
// Part 2: the document (text + styles), word wrapping and pages.
// Part 3: menus and dialogs in the Rustland style.
// Part 4: files, book tools and the main loop.

// ===================================================== 1. curses library

const A_BOLD: u32 = 0x100;
const A_REVERSE: u32 = 0x200;
const A_UNDERLINE: u32 = 0x400;
const A_ITALIC: u32 = 0x800;

// Keys: ASCII (and Ctrl+letter = 1..26), 0x100 + scan code for special
// keys, 0x300 + scan with Ctrl held, 0x400 + scan with Alt held.
const KEY_ESC: i32 = 0x101;
const KEY_BKSP: i32 = 0x10E;
const KEY_TAB: i32 = 0x10F;
const KEY_ENTER: i32 = 0x11C;
const KEY_F1: i32 = 0x13B;
const KEY_F3: i32 = 0x13D;
const KEY_F5: i32 = 0x13F;
const KEY_F7: i32 = 0x141;
const KEY_F9: i32 = 0x143;
const KEY_F10: i32 = 0x144;
const KEY_HOME: i32 = 0x147;
const KEY_UP: i32 = 0x148;
const KEY_PGUP: i32 = 0x149;
const KEY_LEFT: i32 = 0x14B;
const KEY_RIGHT: i32 = 0x14D;
const KEY_END: i32 = 0x14F;
const KEY_DOWN: i32 = 0x150;
const KEY_PGDN: i32 = 0x151;
const KEY_DEL: i32 = 0x153;
const CTRL_KEY: i32 = 0x300;
const ALT_KEY: i32 = 0x400;

static mut SCR: [u16; 2000] = [0; 2000];
static mut PAIRS: [u8; 32] = [7; 32];
static mut CY: i32 = 0;
static mut CX: i32 = 0;
static mut ATTR: u32 = 0;
// The VGA attribute of ATTR, shifted into place (computed once per attrset).
static mut AV: u16 = 0x0700;
static mut CURSOR_ON: bool = true;
static mut SHIFT: bool = false;
static mut ITALIC_FG: u8 = 10;
static mut UNDER_FG: u8 = 11;
static mut BOLD_FG: u8 = 255;

fn init_pair(n: u32, fg: u8, bg: u8) {
    unsafe { PAIRS[n as usize] = fg | (bg << 4); }
}

fn vga(a: u32) -> u8 {
    unsafe {
        let c = PAIRS[(a & 0x1F) as usize];
        let mut fg = c & 0x0F;
        let bg = c >> 4;
        if a & A_UNDERLINE != 0 {
            fg = UNDER_FG;
        }
        if a & A_ITALIC != 0 {
            fg = ITALIC_FG;
        }
        if a & A_BOLD != 0 {
            if BOLD_FG == 255 { fg = fg | 8; } else { fg = BOLD_FG; }
        }
        if a & A_REVERSE != 0 {
            return ((fg & 7) << 4) | bg;
        }
        fg | (bg << 4)
    }
}

fn attrset(a: u32) {
    unsafe {
        if a != ATTR || AV == 0 {
            ATTR = a;
            AV = (vga(a) as u16) << 8;
        }
    }
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
            SCR[(CY * 80 + CX) as usize] = AV | (c as u16);
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

// Text with Borland hotkeys: "~F~ile" draws the F in `hot`.
fn addhot(s: &str, a: u32, hot: u32) {
    let mut on = false;
    for c in s.bytes() {
        if c == b'~' {
            on = !on;
            attrset(if on { hot } else { a });
        } else {
            addch(c);
        }
    }
    attrset(a);
}

fn hot_len(s: &str) -> i32 {
    let mut n = 0;
    for c in s.bytes() {
        if c != b'~' {
            n += 1;
        }
    }
    n
}

fn hot_letter(s: &str) -> u8 {
    let b = s.as_bytes();
    for i in 0..b.len() {
        if b[i] == b'~' && i + 1 < b.len() {
            return b[i + 1].to_ascii_lowercase();
        }
    }
    0
}

fn hline(c: u8, n: i32) {
    for _ in 0..n {
        addch(c);
    }
}

fn fill(y: i32, x: i32, h: i32, w: i32, c: u8) {
    for j in 0..h {
        mv(y + j, x);
        hline(c, w);
    }
}

fn box_at(y: i32, x: i32, h: i32, w: i32, double: bool) {
    let mut c = [0xDAu8, 0xBF, 0xC0, 0xD9, 0xC4, 0xB3];
    if double {
        c = [0xC9, 0xBB, 0xC8, 0xBC, 0xCD, 0xBA];
    }
    let tl = c[0];
    let tr = c[1];
    let bl = c[2];
    let br = c[3];
    let hz = c[4];
    let vt = c[5];
    mv(y, x);
    addch(tl);
    hline(hz, w - 2);
    addch(tr);
    for j in 1..h - 1 {
        mv(y + j, x);
        addch(vt);
        mv(y + j, x + w - 1);
        addch(vt);
    }
    mv(y + h - 1, x);
    addch(bl);
    hline(hz, w - 2);
    addch(br);
}

// Darken the cells right of and below a box.
fn shadow(y: i32, x: i32, h: i32, w: i32) {
    unsafe {
        let a = (vga(P_SHADOW) as u16) << 8;
        for j in y + 1..y + h + 1 {
            for i in x + w..x + w + 2 {
                if j < 25 && i < 80 {
                    let k = (j * 80 + i) as usize;
                    SCR[k] = a | (SCR[k] & 0xFF);
                }
            }
        }
        for i in x + 2..x + w {
            if y + h < 25 && i < 80 {
                let k = ((y + h) * 80 + i) as usize;
                SCR[k] = a | (SCR[k] & 0xFF);
            }
        }
    }
}

fn curs_set(visible: bool) {
    unsafe { CURSOR_ON = visible; }
}

fn refresh() {
    let vram = 0xB8000 as *mut u8;
    unsafe {
        copy_bytes(vram, SCR.as_ptr() as *const u8, 4000);
        let mut r = Regs::new();
        r.eax = 0x0100;
        r.ecx = if CURSOR_ON { 0x0607 } else { 0x2000 };
        dos_int(0x10, &mut r);
        gotoxy(CX as u32, CY as u32);
    }
}

fn getch() -> i32 {
    let k = read_key();
    let scan = ((k >> 8) & 0xFF) as i32;
    let a = (k & 0xFF) as i32;
    let f = unsafe { *(0x417 as *const u8) };
    unsafe { SHIFT = f & 3 != 0; }
    if f & 8 != 0 {
        return ALT_KEY + scan;
    }
    match scan {
        0x01 => return KEY_ESC,
        0x0E => return KEY_BKSP,
        0x0F => return KEY_TAB,
        0x1C => return KEY_ENTER,
        _ => {}
    }
    if a != 0 && a != 0xE0 {
        return a;
    }
    if f & 4 != 0 {
        return CTRL_KEY + scan;
    }
    0x100 + scan
}

// ----------------------------------------------------- string builder

static mut SB: [u8; 256] = [0; 256];
static mut SBLEN: usize = 0;

fn sb_clear() {
    unsafe { SBLEN = 0; }
}

fn sb_char(c: u8) {
    unsafe {
        if SBLEN < 256 {
            SB[SBLEN] = c;
            SBLEN += 1;
        }
    }
}

fn sb_str(s: &str) {
    for c in s.bytes() {
        sb_char(c);
    }
}

fn sb_num(n: u32) {
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
        sb_char(d[k]);
        if k > 0 && k % 3 == 0 {
            sb_char(b',');
        }
    }
}

fn sb() -> &'static str {
    unsafe { str_from_bytes(&SB[..SBLEN]) }
}

// ===================================================== 2. the document

const MAXT: usize = 200000;
const MAXL: usize = 12000;
const PW: usize = 64;
const PX: i32 = 8;

// Character styles (low bits of a character's style byte).
const S_BOLD: u8 = 1;
const S_ITALIC: u8 = 2;
const S_UNDER: u8 = 4;

// Paragraph styles live in the style byte of the '\n' that ends the
// paragraph: kind in bits 0-2, alignment in bits 4-5.
const K_NORMAL: u8 = 0;
const K_CHAPTER: u8 = 1;
const K_SECTION: u8 = 2;
const K_QUOTE: u8 = 3;
const K_SCENE: u8 = 4;
const K_NOTE: u8 = 5;

const AL_LEFT: u8 = 0;
const AL_CENTER: u8 = 1;
const AL_RIGHT: u8 = 2;
const AL_JUSTIFY: u8 = 3;

static mut TEXT: [u8; 200000] = [0; 200000];
static mut STYLE: [u8; 200000] = [0; 200000];
static mut LEN: usize = 0;
static mut CUR: usize = 0;
static mut ANCHOR: usize = 0;
static mut SEL: bool = false;
static mut TOP: usize = 0;
static mut TYPING: u8 = 0;
static mut DIRTY: bool = false;
static mut EDITS: u32 = 0;

// Wrapped lines: start, length, flags (1 = last line of its paragraph,
// 2 = first), paragraph style, indent, width and word count.
static mut LSTART: [u32; 12000] = [0; 12000];
static mut LLEN: [u32; 12000] = [0; 12000];
static mut LFL: [u8; 12000] = [0; 12000];
static mut LPS: [u8; 12000] = [0; 12000];
static mut LIND: [u8; 12000] = [0; 12000];
static mut LAW: [u8; 12000] = [0; 12000];
static mut LWORDS: [u16; 12000] = [0; 12000];
static mut NLINES: usize = 0;
static mut WORDS: u32 = 0;

// Lines being built by a re-layout.
static mut TSTART: [u32; 12000] = [0; 12000];
static mut TLEN: [u32; 12000] = [0; 12000];
static mut TFL: [u8; 12000] = [0; 12000];
static mut TPS: [u8; 12000] = [0; 12000];
static mut TIND: [u8; 12000] = [0; 12000];
static mut TAW: [u8; 12000] = [0; 12000];
static mut TWORDS: [u16; 12000] = [0; 12000];
static mut NT: usize = 0;

// Settings.
static mut THEME: u32 = 0;
static mut FIRST_INDENT: bool = true;
static mut AUTOSAVE: bool = true;
static mut SHOW_BREAKS: bool = true;
static mut TYPEWRITER: bool = false;
static mut FOCUS: bool = false;
static mut PAGE_LINES: u32 = 40;
static mut GOAL: u32 = 1000;
static mut SESSION_BASE: u32 = 0;

fn kind_of(st: u8) -> u8 {
    st & 7
}

fn align_of(st: u8) -> u8 {
    (st >> 4) & 3
}

fn para_start(p: usize) -> usize {
    let mut i = p;
    unsafe {
        while i > 0 && TEXT[i - 1] != b'\n' {
            i -= 1;
        }
    }
    i
}

fn para_end(p: usize) -> usize {
    let mut i = p;
    unsafe {
        while TEXT[i] != b'\n' {
            i += 1;
        }
    }
    i
}

fn para_indent(st: u8, first: bool) -> usize {
    unsafe {
        match kind_of(st) {
            K_QUOTE => 6,
            K_NOTE => 2,
            K_NORMAL => if first && FIRST_INDENT && align_of(st) != AL_CENTER && align_of(st) != AL_RIGHT { 4 } else { 0 },
            _ => 0,
        }
    }
}

fn para_width(st: u8, first: bool) -> usize {
    match kind_of(st) {
        K_QUOTE => PW - 12,
        K_NOTE => PW - 4,
        _ => PW - para_indent(st, first),
    }
}

fn count_words(s: usize, n: usize) -> u16 {
    let mut w: u16 = 0;
    let mut inw = false;
    unsafe {
        for i in s..s + n {
            let c = TEXT[i];
            let is = c != b' ';
            if is && !inw {
                w += 1;
            }
            inw = is;
        }
    }
    w
}

fn push_line(s: usize, n: usize, fl: u8, st: u8, ind: usize, w: usize) {
    unsafe {
        if NT >= MAXL {
            return;
        }
        TSTART[NT] = s as u32;
        TLEN[NT] = n as u32;
        TFL[NT] = fl;
        TPS[NT] = st;
        TIND[NT] = ind as u8;
        TAW[NT] = w as u8;
        TWORDS[NT] = if kind_of(st) == K_NOTE { 0 } else { count_words(s, n) };
        NT += 1;
    }
}

// Word-wrap the paragraph [p, q) (q is its '\n') into the T arrays.
fn layout_para(p: usize, q: usize) {
    unsafe {
        let st = STYLE[q];
        let mut s = p;
        let mut first = true;
        loop {
            let ind = para_indent(st, first);
            let w = para_width(st, first);
            let ff: u8 = if first { 2 } else { 0 };
            if q - s <= w {
                push_line(s, q - s, ff | 1, st, ind, w);
                return;
            }
            let mut b = s + w;
            while b > s && TEXT[b] != b' ' {
                b -= 1;
            }
            if b == s {
                push_line(s, w, ff, st, ind, w);
                s += w;
            } else {
                push_line(s, b - s, ff, st, ind, w);
                s = b + 1;
            }
            first = false;
        }
    }
}

// The first line whose start is >= pos.
fn first_line_ge(pos: usize) -> usize {
    let mut lo: usize = 0;
    let mut hi: usize = unsafe { NLINES };
    while lo < hi {
        let mid = (lo + hi) / 2;
        if unsafe { LSTART[mid] } < pos as u32 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn move_lines(from: usize, to: usize, n: usize) {
    unsafe {
        move_bytes(LSTART.as_mut_ptr().add(to) as *mut u8, LSTART.as_ptr().add(from) as *const u8, n * 4);
        move_bytes(LLEN.as_mut_ptr().add(to) as *mut u8, LLEN.as_ptr().add(from) as *const u8, n * 4);
        move_bytes(LWORDS.as_mut_ptr().add(to) as *mut u8, LWORDS.as_ptr().add(from) as *const u8, n * 2);
        move_bytes(LFL.as_mut_ptr().add(to), LFL.as_ptr().add(from), n);
        move_bytes(LPS.as_mut_ptr().add(to), LPS.as_ptr().add(from), n);
        move_bytes(LIND.as_mut_ptr().add(to), LIND.as_ptr().add(from), n);
        move_bytes(LAW.as_mut_ptr().add(to), LAW.as_ptr().add(from), n);
    }
}

// Re-wrap paragraphs a..=z (new positions; z is a '\n') after the text
// changed length by d. Lines before are kept; lines after are shifted.
fn relayout(a: usize, z: usize, d: i32) {
    unsafe {
        let la = first_line_ge(a);
        let z_old = (z as i32 - d) as usize;
        let lb = first_line_ge(z_old + 1);
        NT = 0;
        let mut p = a;
        while p <= z {
            let q = para_end(p);
            layout_para(p, q);
            p = q + 1;
        }
        for i in la..lb {
            WORDS -= LWORDS[i] as u32;
        }
        for t in 0..NT {
            WORDS += TWORDS[t] as u32;
        }
        let tail = NLINES - lb;
        let mut nt = NT;
        if la + nt + tail > MAXL {
            nt = MAXL - la - tail;
        }
        move_lines(lb, la + nt, tail);
        NLINES = la + nt + tail;
        if d != 0 {
            for i in la + nt..NLINES {
                LSTART[i] = (LSTART[i] as i32 + d) as u32;
            }
        }
        copy_bytes(LSTART.as_mut_ptr().add(la) as *mut u8, TSTART.as_ptr() as *const u8, nt * 4);
        copy_bytes(LLEN.as_mut_ptr().add(la) as *mut u8, TLEN.as_ptr() as *const u8, nt * 4);
        copy_bytes(LWORDS.as_mut_ptr().add(la) as *mut u8, TWORDS.as_ptr() as *const u8, nt * 2);
        copy_bytes(LFL.as_mut_ptr().add(la), TFL.as_ptr(), nt);
        copy_bytes(LPS.as_mut_ptr().add(la), TPS.as_ptr(), nt);
        copy_bytes(LIND.as_mut_ptr().add(la), TIND.as_ptr(), nt);
        copy_bytes(LAW.as_mut_ptr().add(la), TAW.as_ptr(), nt);
    }
}

fn full_layout() {
    unsafe {
        NLINES = 0;
        WORDS = 0;
        relayout(0, LEN - 1, 0);
    }
}

// The line holding position pos (the last line starting at or before it).
fn line_of(pos: usize) -> usize {
    let i = first_line_ge(pos + 1);
    if i == 0 { 0 } else { i - 1 }
}

// Page column of character col of line i, after indent and alignment.
fn line_x(i: usize, col: usize) -> usize {
    unsafe {
        let ind = LIND[i] as usize;
        let w = LAW[i] as usize;
        let n = LLEN[i] as usize;
        let mut al = align_of(LPS[i]);
        if kind_of(LPS[i]) == K_SCENE {
            al = AL_CENTER;
        }
        if n > w {
            return ind + col;
        }
        match al {
            AL_CENTER => ind + (w - n) / 2 + col,
            AL_RIGHT => ind + w - n + col,
            AL_JUSTIFY if LFL[i] & 1 == 0 => {
                let s = LSTART[i] as usize;
                let mut g: usize = 0;
                for k in 0..n {
                    if TEXT[s + k] == b' ' {
                        g += 1;
                    }
                }
                if g == 0 {
                    return ind + col;
                }
                let extra = w - n;
                let mut x = ind;
                let mut seen: usize = 0;
                for k in 0..col {
                    x += 1;
                    if TEXT[s + k] == b' ' {
                        x += extra / g;
                        if seen < extra % g {
                            x += 1;
                        }
                        seen += 1;
                    }
                }
                x
            }
            _ => ind + col,
        }
    }
}

// ----------------------------------------------------- undo

const UNDO_N: usize = 3;
static mut UNDO_BUF: [u32; 3] = [0; 3];
static mut UNDO_LEN: [usize; 3] = [0; 3];
static mut UNDO_CUR: [usize; 3] = [0; 3];
static mut UNDO_COUNT: usize = 0;
static mut UNDO_NEXT: usize = 0;
static mut LAST_KIND: u32 = 0;

fn undo_init() {
    unsafe {
        for i in 0..UNDO_N {
            UNDO_BUF[i] = alloc(MAXT * 2) as u32;
        }
    }
}

// Remember the document before an edit. Runs of typing (kind 1) or
// deleting (kind 2) share one undo step.
fn undo_mark(kind: u32) {
    unsafe {
        if kind != 0 && kind == LAST_KIND {
            return;
        }
        LAST_KIND = kind;
        let slot = UNDO_NEXT;
        let p = UNDO_BUF[slot] as *mut u8;
        copy_bytes(p, TEXT.as_ptr(), LEN);
        copy_bytes(p.add(MAXT), STYLE.as_ptr(), LEN);
        UNDO_LEN[slot] = LEN;
        UNDO_CUR[slot] = CUR;
        UNDO_NEXT = (slot + 1) % UNDO_N;
        if UNDO_COUNT < UNDO_N {
            UNDO_COUNT += 1;
        }
    }
}

fn undo() {
    unsafe {
        if UNDO_COUNT == 0 {
            set_msg("Nothing to undo");
            return;
        }
        UNDO_NEXT = (UNDO_NEXT + UNDO_N - 1) % UNDO_N;
        UNDO_COUNT -= 1;
        let p = UNDO_BUF[UNDO_NEXT] as *const u8;
        LEN = UNDO_LEN[UNDO_NEXT];
        copy_bytes(TEXT.as_mut_ptr(), p, LEN);
        copy_bytes(STYLE.as_mut_ptr(), p.add(MAXT), LEN);
        CUR = UNDO_CUR[UNDO_NEXT];
        if CUR >= LEN {
            CUR = LEN - 1;
        }
        SEL = false;
        LAST_KIND = 0;
        DIRTY = true;
        full_layout();
        set_msg("Undone");
    }
}

// ----------------------------------------------------- editing

fn changed() {
    unsafe {
        DIRTY = true;
        EDITS += 1;
    }
}

// Insert n characters with styles at pos.
fn ins_bytes(pos: usize, t: *const u8, s: *const u8, n: usize) {
    unsafe {
        if LEN + n > MAXT {
            set_msg("Document is full");
            return;
        }
        move_bytes(TEXT.as_mut_ptr().add(pos + n), TEXT.as_ptr().add(pos), LEN - pos);
        move_bytes(STYLE.as_mut_ptr().add(pos + n), STYLE.as_ptr().add(pos), LEN - pos);
        copy_bytes(TEXT.as_mut_ptr().add(pos), t, n);
        copy_bytes(STYLE.as_mut_ptr().add(pos), s, n);
        LEN += n;
        // Through the end of the paragraph after the inserted text (which
        // may have been split by an inserted '\n').
        relayout(para_start(pos), para_end(pos + n), n as i32);
        changed();
    }
}

// Delete n characters at pos (the final newline always stays). Joining
// paragraphs keeps the first one's style unless it was empty.
fn del_bytes(pos: usize, count: usize) {
    unsafe {
        let mut n = count;
        if pos + n >= LEN {
            n = LEN - 1 - pos;
        }
        if n == 0 {
            return;
        }
        let ps = para_start(pos);
        let first_end = para_end(pos);
        let keep = STYLE[first_end];
        let joins = first_end < pos + n;
        move_bytes(TEXT.as_mut_ptr().add(pos), TEXT.as_ptr().add(pos + n), LEN - pos - n);
        move_bytes(STYLE.as_mut_ptr().add(pos), STYLE.as_ptr().add(pos + n), LEN - pos - n);
        LEN -= n;
        let q = para_end(pos);
        if joins && pos > ps {
            STYLE[q] = keep;
        }
        relayout(ps, q, 0 - n as i32);
        changed();
    }
}

static mut ONE_T: [u8; 1] = [0; 1];
static mut ONE_S: [u8; 1] = [0; 1];

fn insert_char(c: u8, st: u8) {
    unsafe {
        ONE_T[0] = c;
        ONE_S[0] = st;
        ins_bytes(CUR, ONE_T.as_ptr(), ONE_S.as_ptr(), 1);
        CUR += 1;
    }
}

fn insert_str(s: &str, st: u8) {
    for c in s.bytes() {
        insert_char(c, st);
    }
}

fn sel_a() -> usize {
    unsafe { if ANCHOR < CUR { ANCHOR } else { CUR } }
}

fn sel_b() -> usize {
    unsafe { if ANCHOR < CUR { CUR } else { ANCHOR } }
}

fn has_sel() -> bool {
    unsafe { SEL && ANCHOR != CUR }
}

fn delete_selection() -> bool {
    unsafe {
        if !has_sel() {
            SEL = false;
            return false;
        }
        undo_mark(0);
        let a = sel_a();
        let b = sel_b();
        del_bytes(a, b - a);
        CUR = a;
        SEL = false;
        true
    }
}

fn type_key(c: u8) {
    unsafe {
        delete_selection();
        undo_mark(1);
        insert_char(c, TYPING);
    }
}

// Enter: split the paragraph (both halves keep its style). At the end of a
// paragraph, start a fresh normal one in plain text.
fn new_paragraph() {
    unsafe {
        delete_selection();
        undo_mark(0);
        let q = para_end(CUR);
        let at_end = CUR == q;
        insert_char(b'\n', STYLE[q] & 0x37);
        if at_end {
            let q2 = CUR;
            STYLE[q2] = 0;
            TYPING = 0;
            relayout(para_start(q2), q2, 0);
        }
    }
}

fn backspace() {
    unsafe {
        if delete_selection() {
            return;
        }
        if CUR > 0 {
            undo_mark(2);
            CUR -= 1;
            del_bytes(CUR, 1);
        }
    }
}

fn delete_key() {
    unsafe {
        if delete_selection() {
            return;
        }
        undo_mark(2);
        del_bytes(CUR, 1);
    }
}

fn delete_paragraph() {
    unsafe {
        undo_mark(0);
        let a = para_start(CUR);
        let q = para_end(CUR);
        if q + 1 >= LEN {
            del_bytes(a, q - a);
        } else {
            del_bytes(a, q + 1 - a);
        }
        CUR = a;
        SEL = false;
    }
}

// Ctrl+B/I/U: style the selection, or switch the style for new text.
fn toggle_style(bit: u8) {
    unsafe {
        if has_sel() {
            undo_mark(0);
            let a = sel_a();
            let b = sel_b();
            let mut all = true;
            for i in a..b {
                if TEXT[i] != b'\n' && STYLE[i] & bit == 0 {
                    all = false;
                }
            }
            for i in a..b {
                if TEXT[i] != b'\n' {
                    STYLE[i] = if all { STYLE[i] & !bit } else { STYLE[i] | bit };
                }
            }
            relayout(para_start(a), para_end(b), 0);
            changed();
        } else {
            TYPING = TYPING ^ bit;
        }
    }
}

fn plain_text() {
    unsafe {
        if has_sel() {
            undo_mark(0);
            let a = sel_a();
            let b = sel_b();
            for i in a..b {
                if TEXT[i] != b'\n' {
                    STYLE[i] = 0;
                }
            }
            relayout(para_start(a), para_end(b), 0);
            changed();
        }
        TYPING = 0;
    }
}

// Apply a paragraph alignment or kind to the paragraphs in the selection
// (or the cursor's paragraph).
fn set_para(mask: u8, value: u8) {
    unsafe {
        undo_mark(0);
        let a = if has_sel() { sel_a() } else { CUR };
        let b = if has_sel() { sel_b() } else { CUR };
        let mut p = para_start(a);
        loop {
            let q = para_end(p);
            STYLE[q] = (STYLE[q] & !mask) | value;
            if q >= b || q + 1 >= LEN {
                break;
            }
            p = q + 1;
        }
        relayout(para_start(a), para_end(b), 0);
        changed();
    }
}

fn set_align(al: u8) {
    set_para(0x30, al << 4);
}

fn set_kind(k: u8) {
    // Headings and scene breaks are centred; other kinds keep alignment.
    if k == K_CHAPTER || k == K_SCENE {
        set_para(0x37, k | (AL_CENTER << 4));
    } else if k == K_SECTION {
        set_para(0x37, k);
    } else {
        set_para(0x07, k);
    }
}

// ----------------------------------------------------- clipboard

static mut CLIPT: [u8; 65536] = [0; 65536];
static mut CLIPS: [u8; 65536] = [0; 65536];
static mut CLIPLEN: usize = 0;

fn copy_sel() -> bool {
    unsafe {
        if !has_sel() {
            set_msg("Select text first (Shift+arrows)");
            return false;
        }
        let a = sel_a();
        let mut n = sel_b() - a;
        if n > 65536 {
            n = 65536;
        }
        copy_bytes(CLIPT.as_mut_ptr(), TEXT.as_ptr().add(a), n);
        copy_bytes(CLIPS.as_mut_ptr(), STYLE.as_ptr().add(a), n);
        CLIPLEN = n;
        set_msg("Copied");
        true
    }
}

fn paste() {
    unsafe {
        if CLIPLEN == 0 {
            set_msg("The clipboard is empty");
            return;
        }
        delete_selection();
        undo_mark(0);
        ins_bytes(CUR, CLIPT.as_ptr(), CLIPS.as_ptr(), CLIPLEN);
        CUR += CLIPLEN;
    }
}

// ===================================================== 3. screen, menus, dialogs

// Colour pairs.
const P_DESK: u32 = 1;
const P_MENU: u32 = 2;
const P_MENU_HOT: u32 = 3;
const P_MENU_SEL: u32 = 4;
const P_MENU_SEL_HOT: u32 = 5;
const P_TEXT: u32 = 6;
const P_FRAME: u32 = 7;
const P_H1: u32 = 8;
const P_H2: u32 = 9;
const P_QUOTE: u32 = 10;
const P_NOTE: u32 = 11;
const P_STATUS: u32 = 12;
const P_STATUS_HI: u32 = 13;
const P_DLG: u32 = 14;
const P_DLG_FRAME: u32 = 15;
const P_BUTTON: u32 = 16;
const P_BUTTON_FOCUS: u32 = 17;
const P_INPUT: u32 = 18;
const P_LIST: u32 = 19;
const P_LIST_SEL: u32 = 20;
const P_SELECT: u32 = 21;
const P_RULER: u32 = 22;
const P_SHADOW: u32 = 23;
const P_BREAK: u32 = 24;
const P_DLG_HOT: u32 = 25;
const P_SCENE: u32 = 26;
const P_BUTTON_HOT: u32 = 27;
const P_BUTTON_SHADOW: u32 = 28;

static mut DESK_CH: u8 = 0xB1;
const NTHEMES: u32 = 4;

fn theme_name(t: u32) -> &'static str {
    match t {
        1 => "Paper",
        2 => "Amber",
        3 => "Midnight",
        _ => "Borland Blue",
    }
}

fn apply_theme(t: u32) {
    unsafe {
        AV = 0;
        // Shared dialog colours.
        init_pair(P_DLG, 0, 7);
        init_pair(P_DLG_FRAME, 15, 7);
        init_pair(P_DLG_HOT, 14, 7);
        init_pair(P_BUTTON, 0, 2);
        init_pair(P_BUTTON_FOCUS, 15, 2);
        init_pair(P_BUTTON_HOT, 14, 2);
        init_pair(P_BUTTON_SHADOW, 0, 7);
        init_pair(P_INPUT, 15, 1);
        init_pair(P_LIST, 0, 3);
        init_pair(P_LIST_SEL, 15, 2);
        init_pair(P_SHADOW, 8, 0);
        init_pair(P_MENU, 0, 7);
        init_pair(P_MENU_HOT, 4, 7);
        init_pair(P_MENU_SEL, 0, 2);
        init_pair(P_MENU_SEL_HOT, 4, 2);
        init_pair(P_STATUS, 0, 3);
        init_pair(P_STATUS_HI, 15, 3);
        BOLD_FG = 255;
        match t {
            1 => {
                DESK_CH = 0xB0;
                init_pair(P_DESK, 8, 0);
                init_pair(P_TEXT, 0, 7);
                init_pair(P_FRAME, 0, 7);
                init_pair(P_H1, 4, 7);
                init_pair(P_H2, 1, 7);
                init_pair(P_QUOTE, 0, 7);
                init_pair(P_NOTE, 0, 6);
                init_pair(P_SELECT, 15, 1);
                init_pair(P_RULER, 8, 7);
                init_pair(P_BREAK, 8, 7);
                init_pair(P_SCENE, 4, 7);
                ITALIC_FG = 2;
                UNDER_FG = 6;
                BOLD_FG = 1;
            }
            2 => {
                DESK_CH = 0xB0;
                init_pair(P_DESK, 6, 0);
                init_pair(P_MENU, 0, 6);
                init_pair(P_MENU_HOT, 15, 6);
                init_pair(P_MENU_SEL, 14, 0);
                init_pair(P_MENU_SEL_HOT, 15, 0);
                init_pair(P_STATUS, 0, 6);
                init_pair(P_STATUS_HI, 15, 6);
                init_pair(P_DLG, 0, 6);
                init_pair(P_DLG_FRAME, 15, 6);
                init_pair(P_DLG_HOT, 15, 6);
                init_pair(P_BUTTON, 6, 0);
                init_pair(P_BUTTON_FOCUS, 14, 0);
                init_pair(P_BUTTON_HOT, 15, 0);
                init_pair(P_BUTTON_SHADOW, 0, 6);
                init_pair(P_INPUT, 14, 0);
                init_pair(P_LIST, 6, 0);
                init_pair(P_LIST_SEL, 0, 6);
                init_pair(P_TEXT, 6, 0);
                init_pair(P_FRAME, 14, 0);
                init_pair(P_H1, 14, 0);
                init_pair(P_H2, 14, 0);
                init_pair(P_QUOTE, 6, 0);
                init_pair(P_NOTE, 0, 6);
                init_pair(P_SELECT, 0, 6);
                init_pair(P_RULER, 6, 0);
                init_pair(P_BREAK, 6, 0);
                init_pair(P_SCENE, 14, 0);
                ITALIC_FG = 12;
                UNDER_FG = 14;
                BOLD_FG = 14;
            }
            3 => {
                DESK_CH = 0xB0;
                init_pair(P_DESK, 1, 0);
                init_pair(P_MENU, 0, 3);
                init_pair(P_MENU_HOT, 15, 3);
                init_pair(P_MENU_SEL, 15, 1);
                init_pair(P_MENU_SEL_HOT, 14, 1);
                init_pair(P_TEXT, 7, 0);
                init_pair(P_FRAME, 3, 0);
                init_pair(P_H1, 14, 0);
                init_pair(P_H2, 11, 0);
                init_pair(P_QUOTE, 7, 0);
                init_pair(P_NOTE, 13, 0);
                init_pair(P_SELECT, 0, 7);
                init_pair(P_RULER, 3, 0);
                init_pair(P_BREAK, 3, 0);
                init_pair(P_SCENE, 11, 0);
                ITALIC_FG = 10;
                UNDER_FG = 13;
            }
            _ => {
                DESK_CH = 0xB1;
                init_pair(P_DESK, 1, 7);
                init_pair(P_TEXT, 7, 1);
                init_pair(P_FRAME, 15, 1);
                init_pair(P_H1, 14, 1);
                init_pair(P_H2, 15, 1);
                init_pair(P_QUOTE, 7, 1);
                init_pair(P_NOTE, 0, 3);
                init_pair(P_SELECT, 1, 7);
                init_pair(P_RULER, 11, 1);
                init_pair(P_BREAK, 3, 1);
                init_pair(P_SCENE, 11, 1);
                ITALIC_FG = 10;
                UNDER_FG = 11;
            }
        }
    }
}

// ----------------------------------------------------- status message

static mut MSGBUF: [u8; 60] = [0; 60];
static mut MSGLEN: usize = 0;

fn set_msg(s: &str) {
    unsafe {
        MSGLEN = 0;
        for c in s.bytes() {
            if MSGLEN < 60 {
                MSGBUF[MSGLEN] = c;
                MSGLEN += 1;
            }
        }
    }
}

// ----------------------------------------------------- file name

static mut FNAME: [u8; 64] = [0; 64];
static mut FNLEN: usize = 0;
static mut NAMED: bool = false;

fn set_fname(s: &str) {
    unsafe {
        FNLEN = 0;
        for c in s.bytes() {
            if FNLEN < 64 {
                FNAME[FNLEN] = c.to_ascii_uppercase();
                FNLEN += 1;
            }
        }
    }
}

fn fname() -> &'static str {
    unsafe { str_from_bytes(&FNAME[..FNLEN]) }
}

// ----------------------------------------------------- the main screen

fn text_rows() -> usize {
    unsafe { if FOCUS { 23 } else { 21 } }
}

fn text_y() -> i32 {
    unsafe { if FOCUS { 1 } else { 2 } }
}

fn break_before(i: usize) -> bool {
    unsafe { SHOW_BREAKS && i > 0 && (i as u32) % PAGE_LINES == 0 }
}

// Screen row of line li when the view starts at TOP (page breaks take a row).
fn row_of(li: usize) -> usize {
    unsafe {
        let mut row = 0;
        let mut i = TOP;
        while i < li {
            row += 1;
            i += 1;
            if break_before(i) {
                row += 1;
            }
        }
        row
    }
}

fn scroll_to_cursor() {
    unsafe {
        let li = line_of(CUR);
        let ph = text_rows();
        if TYPEWRITER {
            TOP = if li > ph / 2 { li - ph / 2 } else { 0 };
        }
        if li < TOP {
            TOP = li;
        }
        while row_of(li) >= ph {
            TOP += 1;
        }
    }
}

fn char_attr(i: usize, st: u8) -> u32 {
    let k = kind_of(st);
    let mut a = match k {
        K_CHAPTER => P_H1 | A_BOLD,
        K_SECTION => P_H2 | A_BOLD,
        K_QUOTE => P_QUOTE | A_ITALIC,
        K_SCENE => P_SCENE,
        K_NOTE => P_NOTE,
        _ => P_TEXT,
    };
    let cs = unsafe { STYLE[i] };
    if cs & S_BOLD != 0 {
        a = a | A_BOLD;
    }
    if cs & S_ITALIC != 0 {
        a = a | A_ITALIC;
    }
    if cs & S_UNDER != 0 {
        a = a | A_UNDERLINE;
    }
    a
}

fn draw_line(i: usize, y: i32) {
    unsafe {
        let s = LSTART[i] as usize;
        let n = LLEN[i] as usize;
        let st = LPS[i];
        let sa = if has_sel() { sel_a() } else { 0 };
        let sb2 = if has_sel() { sel_b() } else { 0 };
        if kind_of(st) == K_NOTE {
            attrset(P_NOTE);
            mv(y, PX + LIND[i] as i32 - 1);
            hline(b' ', LAW[i] as i32 + 2);
        }
        let mut x = line_x(i, 0);
        // Justified lines: spread the spaces.
        let mut al = align_of(st);
        if kind_of(st) == K_SCENE {
            al = AL_CENTER;
        }
        let just = al == AL_JUSTIFY && LFL[i] & 1 == 0 && n <= LAW[i] as usize;
        let mut g: usize = 0;
        if just {
            for k in 0..n {
                if TEXT[s + k] == b' ' {
                    g += 1;
                }
            }
        }
        let extra = if just && g > 0 { LAW[i] as usize - n } else { 0 };
        let mut seen: usize = 0;
        for k in 0..n {
            let p = s + k;
            let mut a = char_attr(p, st);
            if p >= sa && p < sb2 {
                a = P_SELECT;
            }
            attrset(a);
            mv(y, PX + x as i32);
            addch(TEXT[p]);
            x += 1;
            if TEXT[p] == b' ' && g > 0 {
                let mut w = extra / g;
                if seen < extra % g {
                    w += 1;
                }
                seen += 1;
                for _ in 0..w {
                    addch(b' ');
                }
                x += w;
            }
        }
        // A selected paragraph end shows as a selected blank.
        if LFL[i] & 1 != 0 && s + n >= sa && s + n < sb2 {
            attrset(P_SELECT);
            mv(y, PX + x as i32);
            addch(b' ');
        }
    }
}

fn draw_menu_bar(sel: i32) {
    attrset(P_MENU);
    mv(0, 0);
    hline(b' ', 80);
    mv(0, 1);
    addch(0xF0);
    let mut x = 4;
    for m in 0..NMENUS {
        let t = menu_title(m);
        if m as i32 == sel {
            attrset(P_MENU_SEL);
            mv(0, x - 1);
            addch(b' ');
            addhot(t, P_MENU_SEL, P_MENU_SEL_HOT);
            addch(b' ');
        } else {
            mv(0, x);
            addhot(t, P_MENU, P_MENU_HOT);
        }
        x += hot_len(t) + 2;
    }
    attrset(P_MENU);
    mvaddstr(0, 70, "R++ Write");
}

fn draw_status() {
    unsafe {
        let li = line_of(CUR);
        attrset(P_STATUS);
        mv(24, 0);
        hline(b' ', 80);
        sb_clear();
        sb_str(" Pg ");
        sb_num(li as u32 / PAGE_LINES + 1);
        sb_str("/");
        sb_num(NLINES as u32 / PAGE_LINES + 1);
        sb_str("  Ln ");
        sb_num(li as u32 + 1);
        sb_str("  Col ");
        sb_num((CUR - LSTART[li] as usize) as u32 + 1);
        sb_str("  ");
        sb_num(WORDS);
        sb_str(" words");
        mvaddstr(24, 0, sb());
        // Session goal: words written since opening, with a bar.
        let done = if WORDS > SESSION_BASE { WORDS - SESSION_BASE } else { 0 };
        sb_clear();
        sb_str("+");
        sb_num(done);
        sb_str("/");
        sb_num(GOAL);
        mvaddstr(24, 38, sb());
        mv(24, 50);
        let filled: u32 = if GOAL == 0 { 10 } else if done >= GOAL { 10 } else { done * 10 / GOAL };
        attrset(P_STATUS_HI);
        hline(0xDB, filled as i32);
        attrset(P_STATUS);
        hline(0xB0, 10 - filled as i32);
        if MSGLEN > 0 {
            attrset(P_STATUS_HI);
            mv(24, 61);
            for k in 0..MSGLEN {
                if k < 19 {
                    addch(MSGBUF[k]);
                }
            }
            return;
        }
        let st = STYLE[para_end(CUR)];
        mvaddstr(24, 62, match kind_of(st) {
            K_CHAPTER => "Chapter",
            K_SECTION => "Section",
            K_QUOTE => "Quote",
            K_SCENE => "Scene",
            K_NOTE => "Note",
            _ => "Normal",
        });
        mv(24, 70);
        if TYPING & S_BOLD != 0 { attrset(P_STATUS_HI); addch(b'B'); } else { attrset(P_STATUS); addch(b'b'); }
        if TYPING & S_ITALIC != 0 { attrset(P_STATUS_HI); addch(b'I'); } else { attrset(P_STATUS); addch(b'i'); }
        if TYPING & S_UNDER != 0 { attrset(P_STATUS_HI); addch(b'U'); } else { attrset(P_STATUS); addch(b'u'); }
        attrset(P_STATUS);
        mvaddstr(24, 74, match align_of(st) {
            AL_CENTER => "Cntr",
            AL_RIGHT => "Rght",
            AL_JUSTIFY => "Just",
            _ => "Left",
        });
    }
}

fn draw() {
    unsafe {
        scroll_to_cursor();
        let ph = text_rows() as i32;
        let ty = text_y();
        if FOCUS {
            attrset(P_TEXT);
            fill(0, 0, 25, 80, b' ');
        } else {
            attrset(P_DESK);
            fill(1, 0, 23, 80, DESK_CH);
            draw_menu_bar(-1);
            // The page.
            attrset(P_TEXT);
            fill(1, PX - 2, 23, PW as i32 + 4, b' ');
            attrset(P_FRAME);
            box_at(1, PX - 2, 23, PW as i32 + 4, true);
            // Ruler ticks every 8 columns, and the title.
            attrset(P_RULER);
            for c in 0..PW as i32 {
                if c % 8 == 0 {
                    mv(1, PX + c);
                    addch(0xD1);
                }
            }
            sb_clear();
            sb_str(" ");
            sb_str(fname());
            if DIRTY {
                sb_str(" *");
            }
            sb_str(" ");
            attrset(P_FRAME);
            mvaddstr(1, PX + 32 - (SBLEN as i32) / 2, sb());
            shadow(1, PX - 2, 23, PW as i32 + 4);
            draw_status();
        }
        // Text, with a dotted rule and the number at each page break.
        let mut row = 0;
        let mut i = TOP;
        while row < ph && i < NLINES {
            if i > TOP && break_before(i) {
                attrset(P_BREAK);
                mv(ty + row, PX);
                hline(0xFA, PW as i32);
                sb_clear();
                sb_str(" page ");
                sb_num(i as u32 / PAGE_LINES + 1);
                sb_str(" ");
                mvaddstr(ty + row, PX + 28, sb());
                row += 1;
                if row >= ph {
                    break;
                }
            }
            draw_line(i, ty + row);
            row += 1;
            i += 1;
        }
        // The cursor.
        let li = line_of(CUR);
        let col = CUR - LSTART[li] as usize;
        mv(ty + row_of(li) as i32, PX + line_x(li, col) as i32);
        curs_set(true);
        refresh();
    }
}

// ----------------------------------------------------- menus

const NMENUS: usize = 9;
static MENU_START: [usize; 10] = [0, 10, 17, 21, 30, 37, 40, 45, 50, 52];

fn menu_title(m: usize) -> &'static str {
    match m {
        0 => "~F~ile",
        1 => "~E~dit",
        2 => "~S~earch",
        3 => "F~o~rmat",
        4 => "St~y~le",
        5 => "~V~iew",
        6 => "~B~ook",
        7 => "O~p~tions",
        _ => "~H~elp",
    }
}

// Alt+letter scan codes for the menus.
fn menu_scan(m: usize) -> i32 {
    match m {
        0 => 0x21,
        1 => 0x12,
        2 => 0x1F,
        3 => 0x18,
        4 => 0x15,
        5 => 0x2F,
        6 => 0x30,
        7 => 0x19,
        _ => 0x23,
    }
}

// Menu items: "label|shortcut|command".
fn item_def(i: usize) -> &'static str {
    match i {
        0 => "~N~ew|Ctrl+N|1",
        1 => "~O~pen...|Ctrl+O|2",
        2 => "~S~ave|Ctrl+S|3",
        3 => "Save ~a~s...||4",
        4 => "-||0",
        5 => "~E~xport text...||5",
        6 => "~R~ename file...||6",
        7 => "~D~elete file...||7",
        8 => "-||0",
        9 => "E~x~it|Alt+X|8",
        10 => "~U~ndo|Ctrl+Z|10",
        11 => "-||0",
        12 => "Cu~t~|Ctrl+X|11",
        13 => "~C~opy|Ctrl+C|12",
        14 => "~P~aste|Ctrl+V|13",
        15 => "Select ~a~ll|Ctrl+A|14",
        16 => "~D~elete paragraph|Ctrl+Y|15",
        17 => "~F~ind...|Ctrl+F|20",
        18 => "Find ~n~ext|F3|21",
        19 => "~R~eplace all...|Ctrl+H|22",
        20 => "~G~o to page...|Ctrl+G|23",
        21 => "~B~old|Ctrl+B|30",
        22 => "~I~talic|Ctrl+I|31",
        23 => "~U~nderline|Ctrl+U|32",
        24 => "~P~lain text|Ctrl+P|33",
        25 => "-||0",
        26 => "Align ~l~eft|Ctrl+L|34",
        27 => "~C~enter|Ctrl+E|35",
        28 => "Align ~r~ight|Ctrl+R|36",
        29 => "~J~ustify|Ctrl+J|37",
        30 => "~N~ormal text|Alt+1|40",
        31 => "~C~hapter heading|Alt+2|41",
        32 => "~S~ection heading|Alt+3|42",
        33 => "Block ~q~uote|Alt+4|43",
        34 => "Scene ~b~reak|Alt+5|44",
        35 => "Writer's n~o~te|Alt+6|45",
        36 => "~I~nsert scene break||65",
        37 => "~F~ocus mode|F7|50",
        38 => "~P~age breaks||51",
        39 => "~T~ypewriter scrolling||52",
        40 => "~O~utline...|F5|60",
        41 => "~S~tatistics...|F9|61",
        42 => "Word ~g~oal...||62",
        43 => "~N~ext chapter|Ctrl+PgDn|63",
        44 => "~P~revious chapter|Ctrl+PgUp|64",
        45 => "~C~olors...||70",
        46 => "~F~irst-line indent||71",
        47 => "~A~uto-save||72",
        48 => "Page ~l~ength...||73",
        49 => "~S~ave options||74",
        50 => "~K~eys|F1|80",
        _ => "~A~bout...||81",
    }
}

// Field k of a "|"-separated string.
fn field(s: &str, k: usize) -> &str {
    let b = s.as_bytes();
    let mut f = 0;
    let mut start: usize = 0;
    for i in 0..b.len() {
        if b[i] == b'|' {
            if f == k {
                return str_from_bytes(&b[start..i]);
            }
            f += 1;
            start = i + 1;
        }
    }
    if f == k { str_from_bytes(&b[start..]) } else { "" }
}

fn item_cmd(i: usize) -> u32 {
    parse_int(field(item_def(i), 2)) as u32
}

fn cmd_checked(c: u32) -> bool {
    unsafe {
        match c {
            50 => FOCUS,
            51 => SHOW_BREAKS,
            52 => TYPEWRITER,
            71 => FIRST_INDENT,
            72 => AUTOSAVE,
            _ => false,
        }
    }
}

fn menu_x(m: usize) -> i32 {
    let mut x = 4;
    for k in 0..m {
        x += hot_len(menu_title(k)) + 2;
    }
    x
}

// Pull-down menus starting at menu m; returns the chosen command (0 = none).
fn menu_loop(start: usize) -> u32 {
    let mut m = start;
    let mut item = MENU_START[m];
    loop {
        unsafe {
            let was_focus = FOCUS;
            FOCUS = false;
            draw();
            FOCUS = was_focus;
        }
        draw_menu_bar(m as i32);
        let first = MENU_START[m];
        let n = MENU_START[m + 1] - first;
        let mut w: i32 = 20;
        for k in 0..n {
            let d = item_def(first + k);
            let lw = hot_len(field(d, 0)) + field(d, 1).len() as i32 + 8;
            if lw > w {
                w = lw;
            }
        }
        let mut x = menu_x(m) - 1;
        if x + w > 79 {
            x = 79 - w;
        }
        attrset(P_MENU);
        fill(1, x, n as i32 + 2, w, b' ');
        box_at(1, x, n as i32 + 2, w, false);
        shadow(1, x, n as i32 + 2, w);
        for k in 0..n {
            let d = item_def(first + k);
            let y = 2 + k as i32;
            let label = field(d, 0);
            if label == "-" {
                attrset(P_MENU);
                mv(y, x);
                addch(0xC3);
                hline(0xC4, w - 2);
                addch(0xB4);
                continue;
            }
            let sel = first + k == item;
            let a = if sel { P_MENU_SEL } else { P_MENU };
            let h = if sel { P_MENU_SEL_HOT } else { P_MENU_HOT };
            attrset(a);
            mv(y, x + 1);
            hline(b' ', w - 2);
            mv(y, x + 1);
            if cmd_checked(item_cmd(first + k)) {
                addch(0xFB);
            }
            mv(y, x + 2);
            addhot(label, a, h);
            let keys = field(d, 1);
            mvaddstr(y, x + w - 2 - keys.len() as i32, keys);
        }
        curs_set(false);
        mv(24, 79);
        refresh();
        let k = getch();
        match k {
            KEY_ESC => return 0,
            KEY_LEFT => {
                m = (m + NMENUS - 1) % NMENUS;
                item = MENU_START[m];
            }
            KEY_RIGHT => {
                m = (m + 1) % NMENUS;
                item = MENU_START[m];
            }
            KEY_UP => loop {
                item = if item == first { first + n - 1 } else { item - 1 };
                if field(item_def(item), 0) != "-" {
                    break;
                }
            },
            KEY_DOWN => loop {
                item = if item + 1 >= first + n { first } else { item + 1 };
                if field(item_def(item), 0) != "-" {
                    break;
                }
            },
            KEY_ENTER => return item_cmd(item),
            _ => {
                if k >= ALT_KEY {
                    for j in 0..NMENUS {
                        if menu_scan(j) == k - ALT_KEY {
                            m = j;
                            item = MENU_START[m];
                        }
                    }
                } else if k < 128 {
                    let c = (k as u8).to_ascii_lowercase();
                    for j in first..first + n {
                        if hot_letter(field(item_def(j), 0)) == c {
                            return item_cmd(j);
                        }
                    }
                }
            }
        }
    }
}

// ----------------------------------------------------- dialogs

fn dialog_box(y: i32, x: i32, h: i32, w: i32, title: &str) {
    attrset(P_DLG);
    fill(y, x, h, w, b' ');
    attrset(P_DLG_FRAME);
    box_at(y, x, h, w, true);
    mvaddstr(y, x + 3, "[");
    addch(0xFE);
    addch(b']');
    let tx = x + (w - title.len() as i32 - 2) / 2;
    mvaddstr(y, tx, " ");
    addstr(title);
    addstr(" ");
    shadow(y, x, h, w);
    attrset(P_DLG);
}

fn button(y: i32, x: i32, label: &str, focus: bool) {
    let a = if focus { P_BUTTON_FOCUS } else { P_BUTTON };
    attrset(a);
    mv(y, x);
    addch(b' ');
    addhot(label, a, if focus { P_BUTTON_FOCUS } else { P_BUTTON_HOT });
    addch(b' ');
    // A half-block shadow, Borland style.
    attrset(P_BUTTON_SHADOW);
    let w = hot_len(label) + 2;
    mv(y, x + w);
    addch(0xDC);
    mv(y + 1, x + 1);
    hline(0xDF, w);
}

// Message box with up to three buttons ("~Y~es|~N~o|Cancel"). Returns the
// button index, or -1 for Esc.
fn message_box(title: &str, line1: &str, line2: &str, buttons: &str) -> i32 {
    let mut nb = 1;
    for c in buttons.bytes() {
        if c == b'|' {
            nb += 1;
        }
    }
    let mut focus = 0;
    loop {
        draw();
        let mut w = line1.len() as i32 + 8;
        if line2.len() as i32 + 8 > w {
            w = line2.len() as i32 + 8;
        }
        if w < 40 {
            w = 40;
        }
        let x = (80 - w) / 2;
        dialog_box(8, x, 8, w, title);
        mvaddstr(10, x + (w - line1.len() as i32) / 2, line1);
        mvaddstr(11, x + (w - line2.len() as i32) / 2, line2);
        let mut bx = x + (w - nb * 12) / 2 + 1;
        for k in 0..nb {
            button(13, bx, field(buttons, k as usize), k == focus);
            bx += 12;
        }
        curs_set(false);
        refresh();
        let k = getch();
        match k {
            KEY_ESC => return -1,
            KEY_ENTER => return focus,
            KEY_TAB | KEY_RIGHT => focus = (focus + 1) % nb,
            KEY_LEFT => focus = (focus + nb - 1) % nb,
            _ => {
                if k < 128 {
                    let c = (k as u8).to_ascii_lowercase();
                    for j in 0..nb {
                        if hot_letter(field(buttons, j as usize)) == c {
                            return j;
                        }
                    }
                }
            }
        }
    }
}

fn alert(title: &str, line1: &str, line2: &str) {
    message_box(title, line1, line2, "  ~O~K  ");
}

// One-line input. The text is left in INBUF.
static mut INBUF: [u8; 80] = [0; 80];
static mut INLEN: usize = 0;

fn input_text() -> &'static str {
    unsafe { str_from_bytes(&INBUF[..INLEN]) }
}

fn set_input(s: &str) {
    unsafe {
        INLEN = 0;
        for c in s.bytes() {
            if INLEN < 60 {
                INBUF[INLEN] = c;
                INLEN += 1;
            }
        }
    }
}

fn input_box(title: &str, label: &str, init: &str) -> bool {
    set_input(init);
    let mut fresh = true;
    loop {
        draw();
        dialog_box(8, 14, 9, 52, title);
        mv(10, 17);
        addhot(label, P_DLG, P_DLG_HOT);
        attrset(P_INPUT);
        mv(11, 17);
        hline(b' ', 46);
        unsafe {
            mv(11, 18);
            if fresh && INLEN > 0 {
                attrset(P_LIST_SEL);
            }
            addstr(input_text());
            attrset(P_INPUT);
        }
        button(13, 24, "  ~O~K  ", true);
        button(13, 40, " Cancel ", false);
        curs_set(true);
        unsafe { mv(11, 18 + INLEN as i32); }
        refresh();
        let k = getch();
        unsafe {
            match k {
                KEY_ESC => return false,
                KEY_ENTER => return true,
                KEY_BKSP => {
                    if fresh {
                        INLEN = 0;
                    } else if INLEN > 0 {
                        INLEN -= 1;
                    }
                }
                _ => {
                    if k >= 32 && k < 127 {
                        if fresh {
                            INLEN = 0;
                        }
                        if INLEN < 44 {
                            INBUF[INLEN] = k as u8;
                            INLEN += 1;
                        }
                    }
                }
            }
        }
        fresh = false;
    }
}

// A list of lines for list dialogs.
static mut LTXT: [u8; 24000] = [0; 24000];
static mut LOFF: [u32; 600] = [0; 600];
static mut LLN: [u32; 600] = [0; 600];
static mut LPOS: [u32; 600] = [0; 600];
static mut LN: usize = 0;
static mut LUSED: usize = 0;

fn list_clear() {
    unsafe {
        LN = 0;
        LUSED = 0;
    }
}

// Add the string builder's contents as a line; pos is a value to return.
fn list_add(pos: u32) {
    unsafe {
        if LN >= 600 || LUSED + SBLEN > 24000 {
            return;
        }
        copy_bytes(LTXT.as_mut_ptr().add(LUSED), SB.as_ptr(), SBLEN);
        LOFF[LN] = LUSED as u32;
        LLN[LN] = SBLEN as u32;
        LPOS[LN] = pos;
        LUSED += SBLEN;
        LN += 1;
    }
}

fn list_item(i: usize) -> &'static str {
    unsafe { str_from_bytes(&LTXT[LOFF[i] as usize..(LOFF[i] + LLN[i]) as usize]) }
}

// Show the list; returns the chosen index or -1. With select = false it
// is a scrollable text window.
fn list_dialog(title: &str, select: bool, start: usize) -> i32 {
    let rows: usize = 15;
    let mut sel = start;
    let mut top: usize = 0;
    loop {
        unsafe {
            let last = if LN > 0 { LN - 1 } else { 0 };
            if sel > last {
                sel = last;
            }
            if select {
                if sel < top {
                    top = sel;
                }
                if sel >= top + rows {
                    top = sel + 1 - rows;
                }
            }
            draw();
            dialog_box(2, 8, 20, 64, title);
            attrset(if select { P_LIST } else { P_DLG });
            fill(4, 11, rows as i32, 58, b' ');
            for r in 0..rows {
                let i = top + r;
                if i >= LN {
                    break;
                }
                if select && i == sel {
                    attrset(P_LIST_SEL);
                    mv(4 + r as i32, 11);
                    hline(b' ', 58);
                } else {
                    attrset(if select { P_LIST } else { P_DLG });
                }
                mvaddstr(4 + r as i32, 12, list_item(i));
            }
            if LN == 0 {
                mvaddstr(4, 12, "(empty)");
            }
            attrset(P_DLG);
            mvaddstr(20, 11, if select { "Enter selects, Esc cancels" } else { "Arrows scroll, Esc closes" });
            curs_set(false);
            refresh();
            let k = getch();
            let max_top = if LN > rows { LN - rows } else { 0 };
            match k {
                KEY_ESC => return -1,
                KEY_ENTER => return if select && LN > 0 { sel as i32 } else { -1 },
                _ => {}
            }
            if select {
                match k {
                    KEY_UP => if sel > 0 { sel -= 1; },
                    KEY_DOWN => if sel < last { sel += 1; },
                    KEY_PGUP => sel = if sel > rows { sel - rows } else { 0 },
                    KEY_PGDN => sel = if sel + rows < last { sel + rows } else { last },
                    KEY_HOME => sel = 0,
                    KEY_END => sel = last,
                    _ => {}
                }
            } else {
                match k {
                    KEY_UP => if top > 0 { top -= 1; },
                    KEY_DOWN => if top < max_top { top += 1; },
                    KEY_PGUP => top = if top > rows { top - rows } else { 0 },
                    KEY_PGDN => top = if top + rows < max_top { top + rows } else { max_top },
                    KEY_HOME => top = 0,
                    KEY_END => top = max_top,
                    _ => {}
                }
            }
        }
    }
}

// ===================================================== 4. files

fn put_name(off: usize, s: &str) {
    let b = s.as_bytes();
    let x = dos_buffer();
    let mut n = b.len();
    if n > 120 {
        n = 120;
    }
    unsafe {
        copy_bytes(x.add(off), b.as_ptr(), n);
        *x.add(off + n) = 0;
    }
}

fn f_open(name: &str, mode: u32) -> i32 {
    put_name(0, name);
    let mut r = Regs::new();
    r.eax = 0x3D00 | mode;
    r.ds = dos_buffer_seg();
    dos_int(0x21, &mut r);
    if r.carry() { -1 } else { (r.eax & 0xFFFF) as i32 }
}

fn f_create(name: &str) -> i32 {
    put_name(0, name);
    let mut r = Regs::new();
    r.eax = 0x3C00;
    r.ds = dos_buffer_seg();
    dos_int(0x21, &mut r);
    if r.carry() { -1 } else { (r.eax & 0xFFFF) as i32 }
}

fn f_close(h: i32) {
    let mut r = Regs::new();
    r.eax = 0x3E00;
    r.ebx = h as u32;
    dos_int(0x21, &mut r);
}

fn f_write(h: i32, p: *const u8, n: usize) -> bool {
    let mut done: usize = 0;
    while done < n {
        let mut c = n - done;
        if c > 32000 {
            c = 32000;
        }
        copy_bytes(dos_buffer(), p.add(done), c);
        let mut r = Regs::new();
        r.eax = 0x4000;
        r.ebx = h as u32;
        r.ecx = c as u32;
        r.ds = dos_buffer_seg();
        dos_int(0x21, &mut r);
        if r.carry() || (r.eax & 0xFFFF) as usize != c {
            return false;
        }
        done += c;
    }
    true
}

fn f_read(h: i32, p: *mut u8, n: usize) -> usize {
    let mut done: usize = 0;
    while done < n {
        let mut c = n - done;
        if c > 32000 {
            c = 32000;
        }
        let mut r = Regs::new();
        r.eax = 0x3F00;
        r.ebx = h as u32;
        r.ecx = c as u32;
        r.ds = dos_buffer_seg();
        dos_int(0x21, &mut r);
        let got = (r.eax & 0xFFFF) as usize;
        if r.carry() || got == 0 {
            break;
        }
        copy_bytes(p.add(done), dos_buffer(), got);
        done += got;
        if got < c {
            break;
        }
    }
    done
}

fn exists(name: &str) -> bool {
    let h = f_open(name, 0);
    if h >= 0 {
        f_close(h);
    }
    h >= 0
}

fn is_txt(name: &str) -> bool {
    name.ends_with(".TXT") || name.ends_with(".txt") || name.ends_with(".MD")
}

static mut IOBUF: [u8; 32768] = [0; 32768];
static mut IOLEN: usize = 0;

fn out_flush(h: i32) -> bool {
    unsafe {
        let ok = f_write(h, IOBUF.as_ptr(), IOLEN);
        IOLEN = 0;
        ok
    }
}

fn out_byte(h: i32, c: u8) {
    unsafe {
        if IOLEN >= 32768 {
            out_flush(h);
        }
        IOBUF[IOLEN] = c;
        IOLEN += 1;
    }
}

fn out_str(h: i32, s: &str) {
    for c in s.bytes() {
        out_byte(h, c);
    }
}

fn reset_doc() {
    unsafe {
        TEXT[0] = b'\n';
        STYLE[0] = 0;
        LEN = 1;
        CUR = 0;
        TOP = 0;
        SEL = false;
        TYPING = 0;
        UNDO_COUNT = 0;
        LAST_KIND = 0;
    }
}

fn after_load(name: &str) {
    unsafe {
        set_fname(name);
        NAMED = true;
        CUR = 0;
        TOP = 0;
        SEL = false;
        DIRTY = false;
        EDITS = 0;
        UNDO_COUNT = 0;
        full_layout();
        SESSION_BASE = WORDS;
    }
}

// .WP files: "RWP2", the length, the text, then one style byte per character.
fn save_wp(name: &str) -> bool {
    unsafe {
        let h = f_create(name);
        if h < 0 {
            return false;
        }
        let mut hdr = [0u8; 8];
        hdr[0] = b'R';
        hdr[1] = b'W';
        hdr[2] = b'P';
        hdr[3] = b'2';
        hdr[4] = (LEN & 0xFF) as u8;
        hdr[5] = ((LEN >> 8) & 0xFF) as u8;
        hdr[6] = ((LEN >> 16) & 0xFF) as u8;
        hdr[7] = 0;
        let ok = f_write(h, hdr.as_ptr(), 8) && f_write(h, TEXT.as_ptr(), LEN) && f_write(h, STYLE.as_ptr(), LEN);
        f_close(h);
        ok
    }
}

fn load_wp(name: &str) -> bool {
    unsafe {
        let h = f_open(name, 0);
        if h < 0 {
            return false;
        }
        let mut hdr = [0u8; 8];
        let got = f_read(h, hdr.as_mut_ptr(), 8);
        if got == 8 && hdr[0] == b'R' && hdr[1] == b'W' && hdr[2] == b'P' && hdr[3] == b'2' {
            let n = hdr[4] as usize | (hdr[5] as usize) << 8 | (hdr[6] as usize) << 16;
            if n == 0 || n > MAXT {
                f_close(h);
                return false;
            }
            LEN = n;
            f_read(h, TEXT.as_mut_ptr(), n);
            f_read(h, STYLE.as_mut_ptr(), n);
        } else {
            // Version 1: (character, style) pairs.
            f_close(h);
            let h1 = f_open(name, 0);
            LEN = 0;
            loop {
                let n = f_read(h1, IOBUF.as_mut_ptr(), 32768);
                let mut i = 0;
                while i + 1 < n && LEN < MAXT {
                    TEXT[LEN] = IOBUF[i];
                    STYLE[LEN] = IOBUF[i + 1];
                    LEN += 1;
                    i += 2;
                }
                if n < 32768 {
                    break;
                }
            }
            f_close(h1);
            if LEN == 0 || TEXT[LEN - 1] != b'\n' {
                TEXT[LEN] = b'\n';
                STYLE[LEN] = 0;
                LEN += 1;
            }
            return true;
        }
        f_close(h);
        if TEXT[LEN - 1] != b'\n' {
            TEXT[LEN - 1] = b'\n';
        }
        true
    }
}

// Plain text import: one paragraph per line; "# ", "## " and "> " lines
// become chapter, section and quote paragraphs, "* * *" a scene break.
fn finish_import_para(ps: usize) {
    unsafe {
        let n = LEN - ps;
        let mut kind = K_NORMAL;
        let mut cut = 0;
        if n >= 3 && TEXT[ps] == b'#' && TEXT[ps + 1] == b'#' && TEXT[ps + 2] == b' ' {
            kind = K_SECTION;
            cut = 3;
        } else if n >= 2 && TEXT[ps] == b'#' && TEXT[ps + 1] == b' ' {
            kind = K_CHAPTER;
            cut = 2;
        } else if n >= 2 && TEXT[ps] == b'>' && TEXT[ps + 1] == b' ' {
            kind = K_QUOTE;
            cut = 2;
        } else if n == 5 && TEXT[ps] == b'*' && TEXT[ps + 2] == b'*' && TEXT[ps + 4] == b'*' {
            kind = K_SCENE;
        }
        if cut > 0 {
            move_bytes(TEXT.as_mut_ptr().add(ps), TEXT.as_ptr().add(ps + cut), n - cut);
            LEN -= cut;
        }
        for i in ps..LEN {
            STYLE[i] = 0;
        }
        TEXT[LEN] = b'\n';
        STYLE[LEN] = if kind == K_CHAPTER || kind == K_SCENE { kind | (AL_CENTER << 4) } else { kind };
        LEN += 1;
    }
}

fn load_txt(name: &str) -> bool {
    unsafe {
        let h = f_open(name, 0);
        if h < 0 {
            return false;
        }
        LEN = 0;
        let mut ps: usize = 0;
        loop {
            let n = f_read(h, IOBUF.as_mut_ptr(), 32768);
            for i in 0..n {
                let c = IOBUF[i];
                if c == b'\n' {
                    finish_import_para(ps);
                    ps = LEN;
                } else if c == b'\t' {
                    if LEN + 4 < MAXT - 2 {
                        for _ in 0..4 {
                            TEXT[LEN] = b' ';
                            LEN += 1;
                        }
                    }
                } else if c >= 32 && LEN < MAXT - 2 {
                    TEXT[LEN] = c;
                    LEN += 1;
                }
            }
            if n < 32768 {
                break;
            }
        }
        f_close(h);
        if LEN > ps || LEN == 0 {
            finish_import_para(ps);
        }
        // Markdown files separate paragraphs with blank lines: drop them.
        let mut i: usize = 0;
        while i + 1 < LEN {
            if TEXT[i] == b'\n' && TEXT[i + 1] == b'\n' && i + 2 < LEN {
                move_bytes(TEXT.as_mut_ptr().add(i + 1), TEXT.as_ptr().add(i + 2), LEN - i - 2);
                move_bytes(STYLE.as_mut_ptr().add(i + 1), STYLE.as_ptr().add(i + 2), LEN - i - 2);
                LEN -= 1;
            } else {
                i += 1;
            }
        }
        true
    }
}

fn export_txt(name: &str) -> bool {
    unsafe {
        let h = f_create(name);
        if h < 0 {
            return false;
        }
        IOLEN = 0;
        let mut p: usize = 0;
        while p < LEN {
            let q = para_end(p);
            let k = kind_of(STYLE[q]);
            if k != K_NOTE {
                match k {
                    K_CHAPTER => out_str(h, "# "),
                    K_SECTION => out_str(h, "## "),
                    K_QUOTE => out_str(h, "> "),
                    _ => {}
                }
                for i in p..q {
                    out_byte(h, TEXT[i]);
                }
                out_str(h, "\r\n\r\n");
            }
            p = q + 1;
        }
        let ok = out_flush(h);
        f_close(h);
        ok
    }
}

fn add_ext(s: &str, ext: &str) {
    sb_clear();
    sb_str(s);
    if !s.contains(".") {
        sb_str(ext);
    }
}

// Files matching a pattern, added to the list.
fn list_files(pattern: &str) {
    let x = dos_buffer();
    let mut r = Regs::new();
    r.eax = 0x1A00;
    r.edx = 256;
    r.ds = dos_buffer_seg();
    dos_int(0x21, &mut r);
    put_name(0, pattern);
    let mut q = Regs::new();
    q.eax = 0x4E00;
    q.ds = dos_buffer_seg();
    loop {
        dos_int(0x21, &mut q);
        if q.carry() {
            break;
        }
        sb_clear();
        let mut i: usize = 0;
        unsafe {
            while i < 13 && *x.add(256 + 30 + i) != 0 {
                sb_char(*x.add(256 + 30 + i));
                i += 1;
            }
        }
        list_add(0);
        q = Regs::new();
        q.eax = 0x4F00;
        q.ds = dos_buffer_seg();
    }
}

fn str_less(a: &str, b: &str) -> bool {
    let x = a.as_bytes();
    let y = b.as_bytes();
    let mut i: usize = 0;
    while i < x.len() && i < y.len() {
        if x[i] != y[i] {
            return x[i] < y[i];
        }
        i += 1;
    }
    x.len() < y.len()
}

// Sort list lines alphabetically (insertion sort; lists are short).
fn list_sort() {
    unsafe {
        for i in 1..LN {
            let mut j = i;
            while j > 0 && str_less(list_item(j), list_item(j - 1)) {
                let o = LOFF[j];
                let l = LLN[j];
                let p = LPOS[j];
                LOFF[j] = LOFF[j - 1];
                LLN[j] = LLN[j - 1];
                LPOS[j] = LPOS[j - 1];
                LOFF[j - 1] = o;
                LLN[j - 1] = l;
                LPOS[j - 1] = p;
                j -= 1;
            }
        }
    }
}

// File dialog: a name field over a list of documents. Leaves the name in INBUF.
fn file_dialog(title: &str) -> bool {
    list_clear();
    list_files("*.WP");
    list_files("*.TXT");
    list_sort();
    set_input("*.WP");
    let mut sel: usize = 0;
    let mut top: usize = 0;
    loop {
        draw();
        dialog_box(3, 14, 18, 52, title);
        mv(5, 17);
        addhot("~N~ame", P_DLG, P_DLG_HOT);
        attrset(P_INPUT);
        mv(6, 17);
        hline(b' ', 30);
        mvaddstr(6, 18, input_text());
        attrset(P_DLG);
        mv(8, 17);
        addhot("~F~iles", P_DLG, P_DLG_HOT);
        attrset(P_LIST);
        fill(9, 17, 10, 30, b' ');
        unsafe {
            if sel < top {
                top = sel;
            }
            if sel >= top + 10 {
                top = sel - 9;
            }
            for r in 0..10 {
                let i = top + r;
                if i >= LN {
                    break;
                }
                attrset(if i == sel { P_LIST_SEL } else { P_LIST });
                mv(9 + r as i32, 17);
                hline(b' ', 30);
                mvaddstr(9 + r as i32, 18, list_item(i));
            }
            if LN == 0 {
                mvaddstr(9, 18, "(no documents)");
            }
        }
        button(6, 50, "  ~O~K  ", true);
        button(9, 50, " Cancel ", false);
        curs_set(true);
        unsafe { mv(6, 18 + INLEN as i32); }
        refresh();
        let k = getch();
        unsafe {
            match k {
                KEY_ESC => return false,
                KEY_ENTER => {
                    if input_text().contains("*") {
                        if LN > 0 {
                            set_input(list_item(sel));
                            return true;
                        }
                    } else if INLEN > 0 {
                        return true;
                    }
                }
                KEY_UP => {
                    if sel > 0 {
                        sel -= 1;
                    }
                    if LN > 0 {
                        set_input(list_item(sel));
                    }
                }
                KEY_DOWN => {
                    if sel + 1 < LN {
                        sel += 1;
                    }
                    if LN > 0 {
                        set_input(list_item(sel));
                    }
                }
                KEY_BKSP => {
                    if INLEN > 0 {
                        INLEN -= 1;
                    }
                }
                _ => {
                    if k > 32 && k < 127 && INLEN < 28 {
                        if input_text().contains("*") {
                            INLEN = 0;
                        }
                        INBUF[INLEN] = (k as u8).to_ascii_uppercase();
                        INLEN += 1;
                    }
                }
            }
        }
    }
}

// Ask to save a modified document. Returns false to cancel.
fn confirm_discard() -> bool {
    unsafe {
        if !DIRTY {
            return true;
        }
        sb_clear();
        sb_str(fname());
        sb_str(" has been modified.");
        match message_box("Save changes?", sb(), "Save it before continuing?", "  ~Y~es  |  ~N~o  | Cancel ") {
            0 => save(),
            1 => true,
            _ => false,
        }
    }
}

fn save() -> bool {
    unsafe {
        if !NAMED {
            return save_as();
        }
        if save_wp(fname()) {
            DIRTY = false;
            EDITS = 0;
            set_msg("Saved");
            true
        } else {
            alert("Error", "The document could not be saved.", fname());
            false
        }
    }
}

fn save_as() -> bool {
    unsafe {
        let init = if NAMED { fname() } else { "" };
        if !input_box("Save As", "~S~ave document as", init) {
            return false;
        }
        add_ext(input_text(), ".WP");
        set_input(sb());
        let name = input_text();
        if name.len() == 0 {
            return false;
        }
        if exists(name) && message_box("Save As", name, "already exists. Replace it?", "  ~Y~es  |  ~N~o  ") != 0 {
            return false;
        }
        set_fname(name);
        NAMED = true;
        save()
    }
}

fn open_doc() {
    if !confirm_discard() {
        return;
    }
    if !file_dialog("Open a Document") {
        return;
    }
    open_named(input_text());
}

fn open_named(n: &str) {
    sb_clear();
    for c in n.bytes() {
        sb_char(c.to_ascii_uppercase());
    }
    if !n.contains(".") {
        sb_str(".WP");
    }
    set_input(sb());
    let name = input_text();
    let ok = if is_txt(name) { load_txt(name) } else { load_wp(name) };
    if ok {
        after_load(name);
        set_msg("Opened");
    } else {
        reset_doc();
        full_layout();
        alert("Open", "Cannot read this file:", name);
    }
}

fn new_doc() {
    unsafe {
        if !confirm_discard() {
            return;
        }
        reset_doc();
        set_fname("UNTITLED.WP");
        NAMED = false;
        DIRTY = false;
        EDITS = 0;
        full_layout();
        SESSION_BASE = 0;
    }
}

fn export_doc() {
    sb_clear();
    let n = fname();
    let b = n.as_bytes();
    let mut i = 0;
    while i < b.len() && b[i] != b'.' {
        sb_char(b[i]);
        i += 1;
    }
    sb_str(".TXT");
    if !input_box("Export Text", "Export as plain text (Markdown headings) to", sb()) {
        return;
    }
    add_ext(input_text(), ".TXT");
    set_input(sb());
    if export_txt(input_text()) {
        set_msg("Exported");
    } else {
        alert("Export", "The text could not be written to", input_text());
    }
}

fn rename_file() {
    if !file_dialog("Rename a File") {
        return;
    }
    sb_clear();
    sb_str(input_text());
    let mut old = [0u8; 64];
    let ol = sb().len();
    for i in 0..ol {
        old[i] = sb().as_bytes()[i];
    }
    let oldname = str_from_bytes(&old[..ol]);
    if !input_box("Rename", "~N~ew name", oldname) {
        return;
    }
    put_name(0, oldname);
    put_name(128, input_text());
    let mut r = Regs::new();
    r.eax = 0x5600;
    r.ds = dos_buffer_seg();
    r.es = dos_buffer_seg();
    r.edi = 128;
    dos_int(0x21, &mut r);
    if r.carry() {
        alert("Rename", "The file could not be renamed.", oldname);
    } else {
        if oldname == fname() {
            set_fname(input_text());
        }
        set_msg("Renamed");
    }
}

fn delete_file() {
    if !file_dialog("Delete a File") {
        return;
    }
    let name = input_text();
    if message_box("Delete", name, "will be deleted. Are you sure?", "  ~Y~es  |  ~N~o  ") != 0 {
        return;
    }
    put_name(0, name);
    let mut r = Regs::new();
    r.eax = 0x4100;
    r.ds = dos_buffer_seg();
    dos_int(0x21, &mut r);
    if r.carry() {
        alert("Delete", "The file could not be deleted.", name);
    } else {
        set_msg("Deleted");
    }
}

// ----------------------------------------------------- settings

fn load_options() {
    let mut b = [0u8; 16];
    let n = file_read("WRITE.CFG", &mut b);
    unsafe {
        if n >= 9 && b[0] == b'W' {
            THEME = b[1] as u32 % NTHEMES;
            FIRST_INDENT = b[2] != 0;
            AUTOSAVE = b[3] != 0;
            SHOW_BREAKS = b[4] != 0;
            TYPEWRITER = b[5] != 0;
            PAGE_LINES = b[6] as u32;
            if PAGE_LINES < 10 {
                PAGE_LINES = 40;
            }
            GOAL = b[7] as u32 | (b[8] as u32) << 8;
        }
    }
}

fn save_options() {
    let mut b = [0u8; 9];
    unsafe {
        b[0] = b'W';
        b[1] = THEME as u8;
        b[2] = FIRST_INDENT as u8;
        b[3] = AUTOSAVE as u8;
        b[4] = SHOW_BREAKS as u8;
        b[5] = TYPEWRITER as u8;
        b[6] = PAGE_LINES as u8;
        b[7] = (GOAL & 0xFF) as u8;
        b[8] = (GOAL >> 8) as u8;
    }
    if file_write("WRITE.CFG", &b) {
        set_msg("Options saved");
    }
}

// ===================================================== book tools

static mut FINDBUF: [u8; 64] = [0; 64];
static mut FINDLEN: usize = 0;

fn find_from(start: usize) -> bool {
    unsafe {
        if FINDLEN == 0 {
            return false;
        }
        let mut i = start;
        let mut wrapped = false;
        loop {
            if i + FINDLEN > LEN {
                if wrapped {
                    return false;
                }
                wrapped = true;
                i = 0;
            }
            let mut ok = true;
            for k in 0..FINDLEN {
                if TEXT[i + k].to_ascii_lowercase() != FINDBUF[k].to_ascii_lowercase() {
                    ok = false;
                    break;
                }
            }
            if ok {
                ANCHOR = i;
                CUR = i + FINDLEN;
                SEL = true;
                return true;
            }
            i += 1;
            if wrapped && i >= start {
                return false;
            }
        }
    }
}

fn find_next() {
    unsafe {
        let s = if has_sel() { sel_a() + 1 } else { CUR };
        if !find_from(s) {
            set_msg("Not found");
        }
    }
}

fn find_dialog() {
    unsafe {
        if input_box("Find", "~T~ext to find", str_from_bytes(&FINDBUF[..FINDLEN])) {
            FINDLEN = INLEN;
            copy_bytes(FINDBUF.as_mut_ptr(), INBUF.as_ptr(), INLEN);
            find_next();
        }
    }
}

fn replace_all() {
    unsafe {
        if !input_box("Replace All", "~F~ind", str_from_bytes(&FINDBUF[..FINDLEN])) || INLEN == 0 {
            return;
        }
        FINDLEN = INLEN;
        copy_bytes(FINDBUF.as_mut_ptr(), INBUF.as_ptr(), INLEN);
        if !input_box("Replace All", "~R~eplace with", "") {
            return;
        }
        let mut rep = [0u8; 64];
        let rl = INLEN;
        copy_bytes(rep.as_mut_ptr(), INBUF.as_ptr(), rl);
        let mut rs = [0u8; 64];
        undo_mark(0);
        let mut count: u32 = 0;
        let mut i: usize = 0;
        while i + FINDLEN <= LEN {
            let mut ok = true;
            for k in 0..FINDLEN {
                if TEXT[i + k].to_ascii_lowercase() != FINDBUF[k].to_ascii_lowercase() {
                    ok = false;
                    break;
                }
            }
            if ok {
                let st = STYLE[i] & 7;
                for k in 0..rl {
                    rs[k] = st;
                }
                del_bytes(i, FINDLEN);
                if rl > 0 {
                    ins_bytes(i, rep.as_ptr(), rs.as_ptr(), rl);
                }
                i += rl;
                count += 1;
            } else {
                i += 1;
            }
        }
        if CUR >= LEN {
            CUR = LEN - 1;
        }
        SEL = false;
        sb_clear();
        sb_num(count);
        sb_str(" replacement(s) made.");
        alert("Replace All", sb(), "");
    }
}

fn goto_page() {
    unsafe {
        if input_box("Go to Page", "~P~age number", "") {
            let n = parse_int(input_text());
            if n >= 1 {
                let mut li = (n as u32 - 1) * PAGE_LINES;
                if li as usize >= NLINES {
                    li = NLINES as u32 - 1;
                }
                CUR = LSTART[li as usize] as usize;
                SEL = false;
            }
        }
    }
}

// Words in the lines from paragraph start p up to the next chapter heading.
fn chapter_words(p: usize, level: u8) -> u32 {
    unsafe {
        let mut i = first_line_ge(p);
        let mut w: u32 = 0;
        let mut first = true;
        while i < NLINES {
            let k = kind_of(LPS[i]);
            if !first && LFL[i] & 2 != 0 && (k == K_CHAPTER || (level == K_SECTION && k == K_SECTION)) {
                break;
            }
            w += LWORDS[i] as u32;
            first = false;
            i += 1;
        }
        w
    }
}

fn outline() {
    unsafe {
        list_clear();
        let mut p: usize = 0;
        let mut here: usize = 0;
        while p < LEN {
            let q = para_end(p);
            let k = kind_of(STYLE[q]);
            if k == K_CHAPTER || k == K_SECTION {
                if p <= CUR {
                    here = LN;
                }
                sb_clear();
                if k == K_SECTION {
                    sb_str("    ");
                }
                let mut n = q - p;
                if n > 32 {
                    n = 32;
                }
                for i in p..p + n {
                    sb_char(TEXT[i]);
                }
                while SBLEN < 38 {
                    sb_char(b' ');
                }
                sb_num(chapter_words(p, k));
                sb_str(" words, p.");
                sb_num(line_of(p) as u32 / PAGE_LINES + 1);
                list_add(p as u32);
            }
            p = q + 1;
        }
        let r = list_dialog("Outline", true, here);
        if r >= 0 {
            CUR = LPOS[r as usize] as usize;
            SEL = false;
        }
    }
}

fn next_chapter(dir: i32) {
    unsafe {
        let start = para_start(CUR);
        let mut p = start;
        loop {
            if dir > 0 {
                let q = para_end(p);
                if q + 1 >= LEN {
                    set_msg("No next chapter");
                    return;
                }
                p = q + 1;
            } else {
                if p == 0 {
                    set_msg("No previous chapter");
                    return;
                }
                p = para_start(p - 1);
            }
            if kind_of(STYLE[para_end(p)]) == K_CHAPTER {
                CUR = p;
                SEL = false;
                return;
            }
        }
    }
}

fn statistics() {
    unsafe {
        let mut chars: u32 = 0;
        let mut paras: u32 = 0;
        let mut chapters: u32 = 0;
        let mut notes: u32 = 0;
        let mut p: usize = 0;
        while p < LEN {
            let q = para_end(p);
            let k = kind_of(STYLE[q]);
            if k == K_CHAPTER {
                chapters += 1;
            }
            if k == K_NOTE {
                notes += 1;
            } else {
                chars += (q - p) as u32;
                if q > p {
                    paras += 1;
                }
            }
            p = q + 1;
        }
        list_clear();
        sb_clear(); sb_str("Document:          "); sb_str(fname()); list_add(0);
        sb_clear(); list_add(0);
        sb_clear(); sb_str("Words:             "); sb_num(WORDS); list_add(0);
        sb_clear(); sb_str("Characters:        "); sb_num(chars); list_add(0);
        sb_clear(); sb_str("Paragraphs:        "); sb_num(paras); list_add(0);
        sb_clear(); sb_str("Chapters:          "); sb_num(chapters); list_add(0);
        sb_clear(); sb_str("Writer's notes:    "); sb_num(notes); list_add(0);
        sb_clear(); sb_str("Pages:             "); sb_num(NLINES as u32 / PAGE_LINES + 1); sb_str(" ("); sb_num(PAGE_LINES); sb_str(" lines each)"); list_add(0);
        sb_clear(); sb_str("Reading time:      about "); sb_num(WORDS / 250 + 1); sb_str(" min"); list_add(0);
        sb_clear(); list_add(0);
        let done = if WORDS > SESSION_BASE { WORDS - SESSION_BASE } else { 0 };
        sb_clear(); sb_str("This session:      "); sb_num(done); sb_str(" of "); sb_num(GOAL); sb_str(" words"); list_add(0);
        sb_clear(); sb_str("Space used:        "); sb_num(LEN as u32 * 100 / MAXT as u32); sb_str("% of "); sb_num(MAXT as u32); sb_str(" characters"); list_add(0);
        list_dialog("Statistics", false, 0);
    }
}

fn word_goal() {
    unsafe {
        sb_clear();
        sb_num(GOAL);
        let mut g = [0u8; 12];
        let mut n = 0;
        for c in sb().bytes() {
            if c != b',' {
                g[n] = c;
                n += 1;
            }
        }
        if input_box("Word Goal", "~W~ords to write this session", str_from_bytes(&g[..n])) {
            let v = parse_int(input_text());
            if v > 0 && v < 65536 {
                GOAL = v as u32;
                SESSION_BASE = WORDS;
                set_msg("Goal set");
            }
        }
    }
}

fn page_length() {
    unsafe {
        if input_box("Page Length", "~L~ines per page (10-99)", "") {
            let v = parse_int(input_text());
            if v >= 10 && v <= 99 {
                PAGE_LINES = v as u32;
            }
        }
    }
}

fn choose_colors() {
    unsafe {
        let old = THEME;
        list_clear();
        for t in 0..NTHEMES {
            sb_clear();
            sb_str(theme_name(t));
            list_add(t);
        }
        // Preview each theme as it is highlighted.
        let mut sel = THEME as usize;
        loop {
            apply_theme(sel as u32);
            draw();
            dialog_box(6, 24, 10, 32, "Colors");
            for t in 0..NTHEMES as usize {
                attrset(if t == sel { P_LIST_SEL } else { P_LIST });
                mv(8 + t as i32, 27);
                hline(b' ', 26);
                mvaddstr(8 + t as i32, 28, theme_name(t as u32));
            }
            attrset(P_DLG);
            mvaddstr(13, 27, "Enter keeps, Esc cancels");
            curs_set(false);
            refresh();
            match getch() {
                KEY_UP => if sel > 0 { sel -= 1; },
                KEY_DOWN => if sel + 1 < NTHEMES as usize { sel += 1; },
                KEY_ENTER => {
                    THEME = sel as u32;
                    return;
                }
                KEY_ESC => {
                    THEME = old;
                    apply_theme(old);
                    return;
                }
                _ => {}
            }
        }
    }
}

fn insert_scene_break() {
    unsafe {
        undo_mark(0);
        let q = para_end(CUR);
        CUR = q;
        insert_char(b'\n', STYLE[q]);
        insert_str("* * *", 0);
        let q2 = para_end(CUR);
        STYLE[q2] = K_SCENE | (AL_CENTER << 4);
        relayout(para_start(q2), q2, 0);
        new_paragraph();
    }
}

fn help() {
    list_clear();
    sb_clear(); sb_str("MENUS     F10 or Alt + the highlighted letter"); list_add(0);
    sb_clear(); sb_str("FILES     Ctrl+N new  Ctrl+O open  Ctrl+S save  Alt+X exit"); list_add(0);
    sb_clear(); sb_str("EDIT      Shift+arrows select   Ctrl+X/C/V cut/copy/paste"); list_add(0);
    sb_clear(); sb_str("          Ctrl+Z undo  Ctrl+A select all  Ctrl+Y delete para"); list_add(0);
    sb_clear(); sb_str("MOVE      Ctrl+Left/Right word  Ctrl+Home/End document"); list_add(0);
    sb_clear(); sb_str("          Ctrl+PgUp/PgDn previous/next chapter"); list_add(0);
    sb_clear(); sb_str("SEARCH    Ctrl+F find  F3 next  Ctrl+H replace  Ctrl+G page"); list_add(0);
    sb_clear(); sb_str("TEXT      Ctrl+B bold  Ctrl+I italic  Ctrl+U underline"); list_add(0);
    sb_clear(); sb_str("          Ctrl+P plain (on a selection or for new text)"); list_add(0);
    sb_clear(); sb_str("ALIGN     Ctrl+L left  Ctrl+E center  Ctrl+R right  Ctrl+J just."); list_add(0);
    sb_clear(); sb_str("STYLE     Alt+1 normal  Alt+2 chapter  Alt+3 section"); list_add(0);
    sb_clear(); sb_str("          Alt+4 quote   Alt+5 scene    Alt+6 writer's note"); list_add(0);
    sb_clear(); sb_str("BOOK      F5 outline  F9 statistics  F7 focus mode"); list_add(0);
    sb_clear(); list_add(0);
    sb_clear(); sb_str("Writer's notes are kept in the document but left out of the"); list_add(0);
    sb_clear(); sb_str("word count and of exported text. Export writes Markdown-style"); list_add(0);
    sb_clear(); sb_str("text (# chapter, ## section, > quote) that Open reads back."); list_add(0);
    list_dialog("Keys", false, 0);
}

// ===================================================== main loop

fn command(c: u32) -> bool {
    unsafe {
        match c {
            1 => new_doc(),
            2 => open_doc(),
            3 => { save(); }
            4 => { save_as(); }
            5 => export_doc(),
            6 => rename_file(),
            7 => delete_file(),
            8 => return confirm_discard(),
            10 => undo(),
            11 => {
                if copy_sel() {
                    delete_selection();
                    set_msg("Cut");
                }
            }
            12 => { copy_sel(); }
            13 => paste(),
            14 => {
                ANCHOR = 0;
                CUR = LEN - 1;
                SEL = true;
            }
            15 => delete_paragraph(),
            20 => find_dialog(),
            21 => find_next(),
            22 => replace_all(),
            23 => goto_page(),
            30 => toggle_style(S_BOLD),
            31 => toggle_style(S_ITALIC),
            32 => toggle_style(S_UNDER),
            33 => plain_text(),
            34 => set_align(AL_LEFT),
            35 => set_align(AL_CENTER),
            36 => set_align(AL_RIGHT),
            37 => set_align(AL_JUSTIFY),
            40 => set_kind(K_NORMAL),
            41 => set_kind(K_CHAPTER),
            42 => set_kind(K_SECTION),
            43 => set_kind(K_QUOTE),
            44 => set_kind(K_SCENE),
            45 => set_kind(K_NOTE),
            50 => FOCUS = !FOCUS,
            51 => SHOW_BREAKS = !SHOW_BREAKS,
            52 => TYPEWRITER = !TYPEWRITER,
            60 => outline(),
            61 => statistics(),
            62 => word_goal(),
            63 => next_chapter(1),
            64 => next_chapter(-1),
            65 => insert_scene_break(),
            70 => choose_colors(),
            71 => {
                FIRST_INDENT = !FIRST_INDENT;
                full_layout();
            }
            72 => AUTOSAVE = !AUTOSAVE,
            73 => {
                page_length();
            }
            74 => save_options(),
            80 => help(),
            81 => alert("About R++ Write", "R++ Write 2.0 - a word processor for books,", "written in R++ with Rustland R++ on rusty486."),
            _ => {}
        }
        false
    }
}

// Start or end a selection before moving the cursor.
fn begin_move() {
    unsafe {
        if SHIFT {
            if !SEL {
                SEL = true;
                ANCHOR = CUR;
            }
        } else {
            SEL = false;
        }
        LAST_KIND = 0;
    }
}

fn pick_style() {
    unsafe {
        if CUR > 0 && TEXT[CUR - 1] != b'\n' {
            TYPING = STYLE[CUR - 1] & 7;
        }
    }
}

fn move_vert(d: i32) {
    unsafe {
        let li = line_of(CUR) as i32;
        let x = line_x(li as usize, CUR - LSTART[li as usize] as usize);
        let mut t = li + d;
        if t < 0 {
            t = 0;
        }
        if t >= NLINES as i32 {
            t = NLINES as i32 - 1;
        }
        let t = t as usize;
        // Keep the screen column when moving between lines.
        let n = LLEN[t] as usize;
        let mut col: usize = 0;
        while col < n && line_x(t, col + 1) <= x {
            col += 1;
        }
        CUR = LSTART[t] as usize + col;
    }
}

fn word_left() {
    unsafe {
        while CUR > 0 && TEXT[CUR - 1] == b' ' {
            CUR -= 1;
        }
        while CUR > 0 && TEXT[CUR - 1] != b' ' && TEXT[CUR - 1] != b'\n' {
            CUR -= 1;
        }
    }
}

fn word_right() {
    unsafe {
        while CUR + 1 < LEN && TEXT[CUR] != b' ' && TEXT[CUR] != b'\n' {
            CUR += 1;
        }
        while CUR + 1 < LEN && (TEXT[CUR] == b' ' || TEXT[CUR] == b'\n') {
            CUR += 1;
        }
    }
}

// Handle one key; returns true to quit.
fn handle(k: i32) -> bool {
    unsafe {
        MSGLEN = 0;
        if k == KEY_F10 {
            let c = menu_loop(0);
            return command(c);
        }
        if k >= ALT_KEY {
            let s = k - ALT_KEY;
            for m in 0..NMENUS {
                if menu_scan(m) == s {
                    let c = menu_loop(m);
                    return command(c);
                }
            }
            return match s {
                0x2D => command(8),
                0x0E => command(10),
                0x02 => command(40),
                0x03 => command(41),
                0x04 => command(42),
                0x05 => command(43),
                0x06 => command(44),
                0x07 => command(45),
                _ => false,
            };
        }
        let c = match k {
            1 => 14,
            2 => 30,
            3 => 12,
            5 => 35,
            6 => 20,
            7 => 23,
            8 => 22,
            9 => 31,
            10 => 37,
            12 => 34,
            14 => 1,
            15 => 2,
            16 => 33,
            18 => 36,
            19 => 3,
            21 => 32,
            22 => 13,
            24 => 11,
            25 => 15,
            26 => 10,
            KEY_F1 => 80,
            KEY_F3 => 21,
            KEY_F5 => 60,
            KEY_F7 => 50,
            KEY_F9 => 61,
            0x349 => 64,
            0x351 => 63,
            _ => 0,
        };
        if c != 0 {
            return command(c as u32);
        }
        match k {
            KEY_ESC => {
                if FOCUS {
                    FOCUS = false;
                } else {
                    SEL = false;
                }
            }
            KEY_ENTER => new_paragraph(),
            KEY_TAB => {
                delete_selection();
                undo_mark(1);
                insert_str("    ", TYPING);
            }
            KEY_BKSP => backspace(),
            KEY_DEL => delete_key(),
            KEY_LEFT => {
                begin_move();
                if CUR > 0 {
                    CUR -= 1;
                }
                pick_style();
            }
            KEY_RIGHT => {
                begin_move();
                if CUR + 1 < LEN {
                    CUR += 1;
                }
                pick_style();
            }
            KEY_UP => {
                begin_move();
                move_vert(-1);
            }
            KEY_DOWN => {
                begin_move();
                move_vert(1);
            }
            KEY_PGUP => {
                begin_move();
                move_vert(0 - text_rows() as i32);
            }
            KEY_PGDN => {
                begin_move();
                move_vert(text_rows() as i32);
            }
            KEY_HOME => {
                begin_move();
                CUR = LSTART[line_of(CUR)] as usize;
            }
            KEY_END => {
                begin_move();
                let li = line_of(CUR);
                CUR = (LSTART[li] + LLEN[li]) as usize;
            }
            0x34B => {
                begin_move();
                word_left();
            }
            0x34D => {
                begin_move();
                word_right();
            }
            0x347 => {
                begin_move();
                CUR = 0;
            }
            0x34F => {
                begin_move();
                CUR = LEN - 1;
            }
            _ => {
                if k >= 32 && k < 127 {
                    type_key(k as u8);
                }
            }
        }
        // Auto-save a named document every few hundred edits.
        if AUTOSAVE && NAMED && EDITS >= 300 {
            if save_wp(fname()) {
                DIRTY = false;
                EDITS = 0;
                set_msg("Auto-saved");
            }
        }
        false
    }
}

fn welcome() {
    unsafe {
        reset_doc();
        full_layout();
        set_kind(K_CHAPTER);
        insert_str("Chapter One", 0);
        new_paragraph();
        set_align(AL_JUSTIFY);
        insert_str("Welcome to R++ Write 2.0, a word processor for writing books. Press F10 or Alt with a highlighted letter to open the menus, and F1 for the keys. Text can be ", 0);
        insert_str("bold", S_BOLD);
        insert_str(", ", 0);
        insert_str("italic", S_ITALIC);
        insert_str(" or ", 0);
        insert_str("underlined", S_UNDER);
        insert_str(".", 0);
        new_paragraph();
        set_align(AL_JUSTIFY);
        insert_str("Paragraphs have styles: chapter and section headings, block quotes, scene breaks and writer's notes (Style menu, Alt+1 to Alt+6). F5 shows the outline of your book with the words in each chapter; F9 shows statistics; the status bar tracks your word goal.", 0);
        new_paragraph();
        set_kind(K_QUOTE);
        insert_str("A block quote is indented on both sides and set in italics.", 0);
        new_paragraph();
        set_kind(K_NOTE);
        insert_str("Writer's note: notes stay out of the word count and the export.", 0);
        insert_scene_break();
        set_align(AL_JUSTIFY);
        insert_str("Choose colors under Options, and save documents with Ctrl+S.", 0);
        CUR = 0;
        DIRTY = false;
        EDITS = 0;
        UNDO_COUNT = 0;
        SESSION_BASE = WORDS;
        set_fname("UNTITLED.WP");
        NAMED = false;
    }
}

fn main() {
    set_video_mode(3);
    load_options();
    unsafe { apply_theme(THEME); }
    undo_init();
    let a = args();
    if a.len() > 0 {
        welcome();
        open_named(a);
    } else {
        welcome();
    }
    loop {
        draw();
        let mut quit = handle(getch());
        // Handle keys typed while drawing before the next redraw.
        while !quit && key_pressed() {
            quit = handle(getch());
        }
        if quit {
            break;
        }
    }
    set_video_mode(3);
    println!("Thanks for using R++ Write!");
}
