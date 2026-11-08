// TEXT.RS - Text 1.0, a Notepad-style editor for plain text files.
//
// Part 1: a small ncurses-style screen library (shared with R++ Write).
// Part 2: the text: lines, optional word wrap, undo.
// Part 3: the screen and menus (MS-DOS EDIT colours).
// Part 4: files, dialogs, search and the main loop.

// ===================================================== 1. curses library

const A_BOLD: u32 = 0x100;
const A_REVERSE: u32 = 0x200;
const A_UNDERLINE: u32 = 0x400;
const A_ITALIC: u32 = 0x800;
const A_STRIKE: u32 = 0x1000;
const A_MARK: u32 = 0x2000;

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
const KEY_INS: i32 = 0x152;
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
static mut STRIKE_FG: u8 = 8;
static mut MARK_BG: u8 = 6;
static mut MARK_FG: u8 = 255;

fn init_pair(n: u32, fg: u8, bg: u8) {
    unsafe { PAIRS[n as usize] = fg | (bg << 4); }
}

fn vga(a: u32) -> u8 {
    unsafe {
        let c = PAIRS[(a & 0x1F) as usize];
        let mut fg = c & 0x0F;
        let mut bg = c >> 4;
        if a & A_MARK != 0 {
            bg = MARK_BG;
            if MARK_FG != 255 {
                fg = MARK_FG;
            }
        }
        if a & A_STRIKE != 0 {
            fg = STRIKE_FG;
        }
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


// ===================================================== 2. the text

const MAXT: usize = 2000000;
const MAXR: usize = 120000;
const TMAXR: usize = 4096;

// The file as bytes; lines end in '\n' and the buffer always ends with one
// (a sentinel that is not saved if the file had no final newline).
static mut TEXT: [u8; 2000000] = [0; 2000000];
static mut LEN: usize = 0;
static mut CUR: usize = 0;
static mut ANCHOR: usize = 0;
static mut SEL: bool = false;
static mut TOP: usize = 0;
static mut LEFT: usize = 0;
static mut DIRTY: bool = false;
static mut OVERWRITE: bool = false;
static mut CRLF: bool = true;
static mut FINAL_NL: bool = true;
static mut WANT_X: usize = 0;

// Settings (TEXT.CFG).
static mut WRAP: bool = false;
static mut LINE_NUMBERS: bool = false;
static mut SHOW_STATUS: bool = true;
static mut TABW: usize = 8;

// Screen rows: start, length, line number, first row of its line.
static mut RSTART: [u32; 120000] = [0; 120000];
static mut RLEN: [u32; 120000] = [0; 120000];
static mut RLINE: [u32; 120000] = [0; 120000];
static mut RFIRST: [u8; 120000] = [0; 120000];
static mut NR: usize = 0;
static mut TSTART: [u32; 4096] = [0; 4096];
static mut TLEN: [u32; 4096] = [0; 4096];
static mut TLINE: [u32; 4096] = [0; 4096];
static mut TFIRST: [u8; 4096] = [0; 4096];
static mut NT: usize = 0;
static mut T_OVER: bool = false;

fn line_end(p: usize) -> usize {
    let mut i = p;
    unsafe {
        while TEXT[i] != b'\n' {
            i += 1;
        }
    }
    i
}

fn line_start(p: usize) -> usize {
    let mut i = p;
    unsafe {
        while i > 0 && TEXT[i - 1] != b'\n' {
            i -= 1;
        }
    }
    i
}

// Columns a character takes at screen column x (tabs go to the next stop).
fn char_w(c: u8, x: usize) -> usize {
    unsafe {
        if c == b'\t' { TABW - x % TABW } else { 1 }
    }
}

fn gutter() -> usize {
    unsafe { if LINE_NUMBERS { 7 } else { 0 } }
}

// Text columns on screen (column 79 is the scroll bar).
fn text_w() -> usize {
    79 - gutter()
}

fn text_h() -> usize {
    unsafe { if SHOW_STATUS { 23 } else { 24 } }
}

fn push_row(s: usize, n: usize, line: u32, first: bool) {
    unsafe {
        if NT >= TMAXR {
            T_OVER = true;
            return;
        }
        TSTART[NT] = s as u32;
        TLEN[NT] = n as u32;
        TLINE[NT] = line;
        TFIRST[NT] = first as u8;
        NT += 1;
    }
}

// Rows of the line [p, q) (q is its '\n'): one row, or wrapped at spaces.
fn layout_line(p: usize, q: usize, line: u32) {
    unsafe {
        let w = text_w();
        let mut s = p;
        let mut first = true;
        loop {
            if !WRAP {
                push_row(s, q - s, line, first);
                return;
            }
            let mut x: usize = 0;
            let mut i = s;
            let mut space: usize = 0;
            while i < q {
                let cw = char_w(TEXT[i], x);
                if x + cw > w {
                    break;
                }
                x += cw;
                if TEXT[i] == b' ' {
                    space = i + 1;
                }
                i += 1;
            }
            if i >= q {
                push_row(s, q - s, line, first);
                return;
            }
            let e = if space > s { space } else { i };
            push_row(s, e - s, line, first);
            s = e;
            first = false;
        }
    }
}

fn first_row_ge(pos: usize) -> usize {
    let mut lo: usize = 0;
    let mut hi: usize = unsafe { NR };
    while lo < hi {
        let mid = (lo + hi) / 2;
        if unsafe { RSTART[mid] } < pos as u32 {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    lo
}

fn row_of(pos: usize) -> usize {
    let i = first_row_ge(pos + 1);
    if i == 0 { 0 } else { i - 1 }
}

fn copy_rows(at: usize, n: usize) {
    unsafe {
        copy_bytes(RSTART.as_mut_ptr().add(at) as *mut u8, TSTART.as_ptr() as *const u8, n * 4);
        copy_bytes(RLEN.as_mut_ptr().add(at) as *mut u8, TLEN.as_ptr() as *const u8, n * 4);
        copy_bytes(RLINE.as_mut_ptr().add(at) as *mut u8, TLINE.as_ptr() as *const u8, n * 4);
        copy_bytes(RFIRST.as_mut_ptr().add(at), TFIRST.as_ptr(), n);
    }
}

fn move_rows(from: usize, to: usize, n: usize) {
    unsafe {
        move_bytes(RSTART.as_mut_ptr().add(to) as *mut u8, RSTART.as_ptr().add(from) as *const u8, n * 4);
        move_bytes(RLEN.as_mut_ptr().add(to) as *mut u8, RLEN.as_ptr().add(from) as *const u8, n * 4);
        move_bytes(RLINE.as_mut_ptr().add(to) as *mut u8, RLINE.as_ptr().add(from) as *const u8, n * 4);
        move_bytes(RFIRST.as_mut_ptr().add(to), RFIRST.as_ptr().add(from), n);
    }
}

fn full_layout() {
    unsafe {
        NR = 0;
        let mut p: usize = 0;
        let mut line: u32 = 0;
        while p < LEN {
            let q = line_end(p);
            NT = 0;
            layout_line(p, q, line);
            let mut nt = NT;
            if NR + nt > MAXR {
                nt = MAXR - NR;
            }
            copy_rows(NR, nt);
            NR += nt;
            line += 1;
            p = q + 1;
        }
    }
}

// Re-wrap lines a..=z (new positions; z is a '\n') after the text changed
// length by d. Earlier rows stay; later rows shift (position and number).
fn relayout(a: usize, z: usize, d: i32) {
    unsafe {
        let ra = first_row_ge(a);
        let z_old = (z as i32 - d) as usize;
        let rb = first_row_ge(z_old + 1);
        let line0 = RLINE[ra];
        NT = 0;
        T_OVER = false;
        let mut p = a;
        let mut line = line0;
        while p <= z {
            let q = line_end(p);
            layout_line(p, q, line);
            line += 1;
            p = q + 1;
        }
        if T_OVER {
            full_layout();
            return;
        }
        let old_next = if rb < NR { RLINE[rb] } else { line };
        let dl = line as i32 - old_next as i32;
        let tail = NR - rb;
        let mut nt = NT;
        if ra + nt + tail > MAXR {
            nt = MAXR - ra - tail;
        }
        move_rows(rb, ra + nt, tail);
        NR = ra + nt + tail;
        if d != 0 || dl != 0 {
            for i in ra + nt..NR {
                RSTART[i] = (RSTART[i] as i32 + d) as u32;
                RLINE[i] = (RLINE[i] as i32 + dl) as u32;
            }
        }
        copy_rows(ra, nt);
    }
}

// Screen column of position pos within its row.
fn col_x(r: usize, pos: usize) -> usize {
    unsafe {
        let s = RSTART[r] as usize;
        let mut x: usize = 0;
        let mut i = s;
        while i < pos {
            x += char_w(TEXT[i], x);
            i += 1;
        }
        x
    }
}

// The position in row r nearest to screen column x.
fn pos_at_x(r: usize, x: usize) -> usize {
    unsafe {
        let s = RSTART[r] as usize;
        let n = RLEN[r] as usize;
        let mut cx: usize = 0;
        let mut i = s;
        // A wrapped row ends before its last space; the line end may be at s + n.
        let last = if r + 1 < NR && RFIRST[r + 1] == 0 { s + n - 1 } else { s + n };
        while i < last {
            let w = char_w(TEXT[i], cx);
            if cx + w > x {
                break;
            }
            cx += w;
            i += 1;
        }
        i
    }
}

// ----------------------------------------------------- undo

const U_MARK: u8 = 0;
const U_INS: u8 = 1;
const U_DEL: u8 = 2;
const ULOG_CAP: usize = 1048576;
static mut ULOG: [u8; 1048576] = [0; 1048576];
static mut ULEN: usize = 0;
static mut LOGGING: bool = true;
static mut LAST_KIND: u32 = 0;

fn ulog_u32(at: usize, v: u32) {
    unsafe {
        ULOG[at] = (v & 0xFF) as u8;
        ULOG[at + 1] = ((v >> 8) & 0xFF) as u8;
        ULOG[at + 2] = ((v >> 16) & 0xFF) as u8;
        ULOG[at + 3] = (v >> 24) as u8;
    }
}

fn ulog_get(at: usize) -> u32 {
    unsafe { ULOG[at] as u32 | (ULOG[at + 1] as u32) << 8 | (ULOG[at + 2] as u32) << 16 | (ULOG[at + 3] as u32) << 24 }
}

// Start an undo record: a header (type, position, count), the deleted text
// for U_DEL, and the record size so the log can be read backwards.
fn ulog_begin(t: u8, pos: usize, n: usize) -> usize {
    unsafe {
        if !LOGGING {
            return 0;
        }
        let data = if t == U_DEL { n } else { 0 };
        let size = 9 + data + 4;
        if size > ULOG_CAP / 2 {
            ULEN = 0;
            return 0;
        }
        if ULEN + size > ULOG_CAP {
            let mut cut: usize = 0;
            while cut < ULEN && (cut < ULEN / 2 || ULOG[cut] != U_MARK) {
                let rt = ULOG[cut];
                let rn = ulog_get(cut + 5) as usize;
                cut += 9 + (if rt == U_DEL { rn } else { 0 }) + 4;
            }
            move_bytes(ULOG.as_mut_ptr(), ULOG.as_ptr().add(cut), ULEN - cut);
            ULEN -= cut;
        }
        let at = ULEN;
        ULOG[at] = t;
        ulog_u32(at + 1, pos as u32);
        ulog_u32(at + 5, n as u32);
        ulog_u32(at + size - 4, size as u32);
        ULEN += size;
        at + 9
    }
}

// Runs of typing (1) or deleting (2) are one undo step.
fn undo_mark(kind: u32) {
    unsafe {
        if kind != 0 && kind == LAST_KIND {
            return;
        }
        LAST_KIND = kind;
        ulog_begin(U_MARK, CUR, 0);
    }
}

fn undo() {
    unsafe {
        if ULEN == 0 {
            set_msg("Nothing to undo");
            return;
        }
        LOGGING = false;
        while ULEN > 0 {
            let size = ulog_get(ULEN - 4) as usize;
            let at = ULEN - size;
            let t = ULOG[at];
            let pos = ulog_get(at + 1) as usize;
            let n = ulog_get(at + 5) as usize;
            ULEN = at;
            if t == U_MARK {
                CUR = pos;
                break;
            } else if t == U_INS {
                del_bytes(pos, n);
            } else {
                ins_bytes(pos, ULOG.as_ptr().add(at + 9), n);
            }
        }
        LOGGING = true;
        if CUR >= LEN {
            CUR = LEN - 1;
        }
        SEL = false;
        LAST_KIND = 0;
        DIRTY = true;
    }
}

// ----------------------------------------------------- editing

fn ins_bytes(pos: usize, t: *const u8, n: usize) -> bool {
    unsafe {
        if LEN + n > MAXT || NR + 64 > MAXR {
            set_msg("The file is full (2 MB)");
            return false;
        }
        ulog_begin(U_INS, pos, n);
        move_bytes(TEXT.as_mut_ptr().add(pos + n), TEXT.as_ptr().add(pos), LEN - pos);
        copy_bytes(TEXT.as_mut_ptr().add(pos), t, n);
        LEN += n;
        relayout(line_start(pos), line_end(pos + n), n as i32);
        DIRTY = true;
        true
    }
}

fn del_bytes(pos: usize, count: usize) {
    unsafe {
        if pos + 1 >= LEN {
            return;
        }
        let mut n = count;
        if pos + n >= LEN {
            n = LEN - 1 - pos;
        }
        if n == 0 {
            return;
        }
        let d = ulog_begin(U_DEL, pos, n);
        if d > 0 {
            copy_bytes(ULOG.as_mut_ptr().add(d), TEXT.as_ptr().add(pos), n);
        }
        let ls = line_start(pos);
        move_bytes(TEXT.as_mut_ptr().add(pos), TEXT.as_ptr().add(pos + n), LEN - pos - n);
        LEN -= n;
        relayout(ls, line_end(pos), 0 - n as i32);
        DIRTY = true;
    }
}

static mut ONE: [u8; 1] = [0; 1];

fn insert_char(c: u8) -> bool {
    unsafe {
        ONE[0] = c;
        if !ins_bytes(CUR, ONE.as_ptr(), 1) {
            return false;
        }
        CUR += 1;
        true
    }
}

fn insert_str(s: &str) {
    for c in s.bytes() {
        if !insert_char(c) {
            return;
        }
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
        del_bytes(a, sel_b() - a);
        CUR = a;
        SEL = false;
        true
    }
}

fn type_key(c: u8) {
    unsafe {
        delete_selection();
        undo_mark(1);
        // Overwrite replaces a character, but never a line end.
        if OVERWRITE && TEXT[CUR] != b'\n' {
            del_bytes(CUR, 1);
        }
        insert_char(c);
    }
}

static mut CLIP: [u8; 262144] = [0; 262144];
static mut CLIPLEN: usize = 0;

fn copy_sel() -> bool {
    unsafe {
        if !has_sel() {
            set_msg("Select text first (Shift+arrows)");
            return false;
        }
        let a = sel_a();
        let mut n = sel_b() - a;
        if n > 262144 {
            n = 262144;
            set_msg("Only the first 256 KB was copied");
        }
        copy_bytes(CLIP.as_mut_ptr(), TEXT.as_ptr().add(a), n);
        CLIPLEN = n;
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
        if ins_bytes(CUR, CLIP.as_ptr(), CLIPLEN) {
            CUR += CLIPLEN;
        }
    }
}

// ===================================================== 3. screen

const P_TEXT: u32 = 1;
const P_MENU: u32 = 2;
const P_MENU_HOT: u32 = 3;
const P_MENU_SEL: u32 = 4;
const P_MENU_SEL_HOT: u32 = 5;
const P_STATUS: u32 = 6;
const P_STATUS_HI: u32 = 7;
const P_SELECT: u32 = 8;
const P_GUTTER: u32 = 9;
const P_SCROLL: u32 = 10;
const P_DLG: u32 = 14;
const P_DLG_FRAME: u32 = 15;
const P_BUTTON: u32 = 16;
const P_BUTTON_FOCUS: u32 = 17;
const P_INPUT: u32 = 18;
const P_LIST: u32 = 19;
const P_LIST_SEL: u32 = 20;
const P_SHADOW: u32 = 23;
const P_DLG_HOT: u32 = 25;
const P_BUTTON_HOT: u32 = 27;
const P_BUTTON_SHADOW: u32 = 28;

// MS-DOS EDIT colours.
fn colors() {
    init_pair(P_TEXT, 7, 1);
    init_pair(P_MENU, 0, 7);
    init_pair(P_MENU_HOT, 15, 7);
    init_pair(P_MENU_SEL, 7, 0);
    init_pair(P_MENU_SEL_HOT, 15, 0);
    init_pair(P_STATUS, 15, 3);
    init_pair(P_STATUS_HI, 14, 3);
    init_pair(P_SELECT, 1, 7);
    init_pair(P_GUTTER, 3, 1);
    init_pair(P_SCROLL, 0, 7);
    init_pair(P_DLG, 0, 7);
    init_pair(P_DLG_FRAME, 15, 7);
    init_pair(P_DLG_HOT, 15, 7);
    init_pair(P_BUTTON, 0, 7);
    init_pair(P_BUTTON_FOCUS, 15, 0);
    init_pair(P_BUTTON_HOT, 15, 7);
    init_pair(P_BUTTON_SHADOW, 0, 7);
    init_pair(P_INPUT, 15, 1);
    init_pair(P_LIST, 0, 3);
    init_pair(P_LIST_SEL, 15, 1);
    init_pair(P_SHADOW, 8, 0);
}

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

static mut FNAME: [u8; 128] = [0; 128];
static mut FNLEN: usize = 0;
static mut NAMED: bool = false;

fn set_fname(s: &str) {
    unsafe {
        FNLEN = 0;
        for c in s.bytes() {
            if FNLEN < 128 {
                FNAME[FNLEN] = c.to_ascii_uppercase();
                FNLEN += 1;
            }
        }
    }
}

fn fname() -> &'static str {
    unsafe { str_from_bytes(&FNAME[..FNLEN]) }
}

fn scroll_to_cursor() {
    unsafe {
        let r = row_of(CUR);
        let h = text_h();
        if r < TOP {
            TOP = r;
        }
        if r >= TOP + h {
            TOP = r + 1 - h;
        }
        if WRAP {
            LEFT = 0;
        } else {
            let x = col_x(r, CUR);
            let w = text_w();
            if x < LEFT {
                LEFT = x;
            }
            if x >= LEFT + w {
                LEFT = x + 1 - w;
            }
        }
    }
}

fn draw_row(r: usize, y: i32) {
    unsafe {
        let g = gutter() as i32;
        let w = text_w();
        attrset(P_TEXT);
        mv(y, g);
        hline(b' ', w as i32);
        if r >= NR {
            if LINE_NUMBERS {
                attrset(P_GUTTER);
                mv(y, 0);
                hline(b' ', g);
            }
            return;
        }
        if LINE_NUMBERS {
            attrset(P_GUTTER);
            mv(y, 0);
            hline(b' ', g);
            if RFIRST[r] != 0 {
                sb_clear();
                sb_num(RLINE[r] + 1);
                // Line numbers without the thousands separator fit 6 columns.
                let mut d = [0u8; 12];
                let mut nd = 0;
                for k in 0..SBLEN {
                    if SB[k] != b',' {
                        d[nd] = SB[k];
                        nd += 1;
                    }
                }
                mvaddstr(y, 6 - nd as i32, str_from_bytes(&d[..nd]));
            }
        }
        let s = RSTART[r] as usize;
        let n = RLEN[r] as usize;
        let sa = if has_sel() { sel_a() } else { 0 };
        let sb2 = if has_sel() { sel_b() } else { 0 };
        let mut x: usize = 0;
        for k in 0..n {
            let p = s + k;
            let c = TEXT[p];
            let cw = char_w(c, x);
            let a = if p >= sa && p < sb2 { P_SELECT } else { P_TEXT };
            for j in 0..cw {
                let sx = x + j;
                if sx >= LEFT && sx < LEFT + w {
                    attrset(a);
                    mv(y, g + (sx - LEFT) as i32);
                    addch(if c == b'\t' || c < 32 { b' ' } else { c });
                }
            }
            x += cw;
        }
        // A selected line end shows as one selected blank.
        let e = s + n;
        if e < LEN && TEXT[e] == b'\n' && e >= sa && e < sb2 && x >= LEFT && x < LEFT + w {
            attrset(P_SELECT);
            mv(y, g + (x - LEFT) as i32);
            addch(b' ');
        }
    }
}

fn draw_menu_bar(sel: i32) {
    attrset(P_MENU);
    mv(0, 0);
    hline(b' ', 80);
    let mut x = 2;
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
    unsafe {
        attrset(P_MENU);
        sb_clear();
        if NAMED {
            // Long paths keep their end.
            let f = fname();
            if f.len() > 44 {
                sb_str("...");
                sb_str(str_from_bytes(&f.as_bytes()[f.len() - 41..]));
            } else {
                sb_str(f);
            }
        } else {
            sb_str("Untitled");
        }
        if DIRTY {
            sb_str(" *");
        }
        mvaddstr(0, 79 - SBLEN as i32, sb());
    }
}

static mut SB_SAVE: [u8; 256] = [0; 256];
static mut SB_SAVE_LEN: usize = 0;

// Dialogs pass the string builder's text while draw() uses it for the
// status bar, so draw() puts it back afterwards.
fn draw() {
    unsafe {
        copy_bytes(SB_SAVE.as_mut_ptr(), SB.as_ptr(), SBLEN);
        SB_SAVE_LEN = SBLEN;
        draw_screen();
        copy_bytes(SB.as_mut_ptr(), SB_SAVE.as_ptr(), SB_SAVE_LEN);
        SBLEN = SB_SAVE_LEN;
    }
}

fn draw_screen() {
    unsafe {
        scroll_to_cursor();
        let h = text_h();
        draw_menu_bar(-1);
        for i in 0..h {
            draw_row(TOP + i, 1 + i as i32);
        }
        // Scroll bar.
        attrset(P_SCROLL);
        mv(1, 79);
        addch(0x1E);
        for y in 2..h as i32 {
            mv(y, 79);
            addch(0xB0);
        }
        mv(h as i32, 79);
        addch(0x1F);
        if NR > 1 && h > 3 {
            let t = 2 + (TOP * (h - 3) / (NR - 1)) as i32;
            mv(if t > h as i32 - 1 { h as i32 - 1 } else { t }, 79);
            addch(0xDB);
        }
        if SHOW_STATUS {
            attrset(P_STATUS);
            mv(24, 0);
            hline(b' ', 80);
            if MSGLEN > 0 {
                attrset(P_STATUS_HI);
                mv(24, 1);
                for k in 0..MSGLEN {
                    addch(MSGBUF[k]);
                }
                attrset(P_STATUS);
            } else {
                mvaddstr(24, 1, "F1=Help  F10=Menu  Ctrl+S=Save  Alt+X=Exit");
            }
            let r = row_of(CUR);
            let ls = line_start(CUR);
            let mut col: usize = 0;
            for i in ls..CUR {
                col += char_w(TEXT[i], col);
            }
            sb_clear();
            sb_str("Ln ");
            sb_num(RLINE[r] + 1);
            sb_str(", Col ");
            sb_num(col as u32 + 1);
            sb_str(if OVERWRITE { "  OVR" } else { "     " });
            sb_str(if CRLF { "  CRLF" } else { "  LF  " });
            mvaddstr(24, 79 - SBLEN as i32, sb());
        }
        // The cursor.
        let r = row_of(CUR);
        let x = col_x(r, CUR);
        mv(1 + (r - TOP) as i32, gutter() as i32 + (x as i32 - LEFT as i32));
        curs_set(true);
    }
}

// Show the editor: draw() only builds the screen, so a dialog drawn over
// it is shown in one go, without the editor flashing through.
fn show_editor() {
    refresh();
    let mut rr = Regs::new();
    rr.eax = 0x0100;
    rr.ecx = if unsafe { OVERWRITE } { 0x0007 } else { 0x0607 };
    dos_int(0x10, &mut rr);
}

// ----------------------------------------------------- menus

const NMENUS: usize = 5;
static MENU_START: [usize; 6] = [0, 6, 15, 19, 23, 25];

fn menu_title(m: usize) -> &'static str {
    match m {
        0 => "~F~ile",
        1 => "~E~dit",
        2 => "~S~earch",
        3 => "~V~iew",
        _ => "~H~elp",
    }
}

fn menu_scan(m: usize) -> i32 {
    match m {
        0 => 0x21,
        1 => 0x12,
        2 => 0x1F,
        3 => 0x2F,
        _ => 0x23,
    }
}

fn item_def(i: usize) -> &'static str {
    match i {
        0 => "~N~ew|Ctrl+N|1",
        1 => "~O~pen...|Ctrl+O|2",
        2 => "~S~ave|Ctrl+S|3",
        3 => "Save ~A~s...||4",
        4 => "-||0",
        5 => "E~x~it|Alt+X|8",
        6 => "~U~ndo|Ctrl+Z|10",
        7 => "-||0",
        8 => "Cu~t~|Ctrl+X|11",
        9 => "~C~opy|Ctrl+C|12",
        10 => "~P~aste|Ctrl+V|13",
        11 => "De~l~ete|Del|16",
        12 => "-||0",
        13 => "Select ~A~ll|Ctrl+A|14",
        14 => "Time/~D~ate|F5|17",
        15 => "~F~ind...|Ctrl+F|20",
        16 => "Find ~N~ext|F3|21",
        17 => "~R~eplace...|Ctrl+H|22",
        18 => "~G~o To Line...|Ctrl+G|23",
        19 => "~W~ord Wrap|Ctrl+W|30",
        20 => "~L~ine Numbers||31",
        21 => "~S~tatus Bar||32",
        22 => "~T~ab Width...||33",
        23 => "~K~eys|F1|80",
        _ => "~A~bout Text||81",
    }
}

fn cmd_checked(c: u32) -> bool {
    unsafe {
        match c {
            30 => WRAP,
            31 => LINE_NUMBERS,
            32 => SHOW_STATUS,
            _ => false,
        }
    }
}

fn menu_loop(start: usize) -> u32 {
    let mut m = start;
    let mut item = MENU_START[m];
    loop {
        draw();
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
            if cmd_checked(item_cmd(first + k)) {
                mv(y, x + 1);
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

fn menu_x(m: usize) -> i32 {
    let mut x = 2;
    for k in 0..m {
        x += hot_len(menu_title(k)) + 2;
    }
    x
}

// ----------------------------------------------------- dialogs

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
            while j > 0 && (LPOS[j] < LPOS[j - 1] || (LPOS[j] == LPOS[j - 1] && str_less(list_item(j), list_item(j - 1)))) {
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

// ===================================================== 4. files

static mut IOBUF: [u8; 32768] = [0; 32768];
static mut IOLEN: usize = 0;

fn out_flush(h: i32) -> bool {
    unsafe {
        let ok = f_write(h, IOBUF.as_ptr(), IOLEN);
        IOLEN = 0;
        ok
    }
}

fn out_byte(h: i32, c: u8) -> bool {
    unsafe {
        if IOLEN >= 32768 && !out_flush(h) {
            return false;
        }
        IOBUF[IOLEN] = c;
        IOLEN += 1;
        true
    }
}

fn file_size(h: i32) -> u32 {
    // Seek to the end (AX=4202h) and back to the start.
    let mut r = Regs::new();
    r.eax = 0x4202;
    r.ebx = h as u32;
    dos_int(0x21, &mut r);
    let size = (r.edx & 0xFFFF) << 16 | (r.eax & 0xFFFF);
    let mut q = Regs::new();
    q.eax = 0x4200;
    q.ebx = h as u32;
    dos_int(0x21, &mut q);
    size
}

fn reset_doc() {
    unsafe {
        TEXT[0] = b'\n';
        LEN = 1;
        CUR = 0;
        TOP = 0;
        LEFT = 0;
        SEL = false;
        DIRTY = false;
        CRLF = true;
        FINAL_NL = true;
        ULEN = 0;
        LAST_KIND = 0;
        full_layout();
    }
}

// Load a file: CR LF or LF line ends (remembered), tabs and all other bytes
// as they are.
fn load(name: &str) -> i32 {
    unsafe {
        let h = f_open(name, 0);
        if h < 0 {
            return -1;
        }
        let size = file_size(h) as usize;
        if size + 1 > MAXT {
            f_close(h);
            return -2;
        }
        let n = f_read(h, TEXT.as_mut_ptr(), size);
        f_close(h);
        // Drop CRs before LFs; a Ctrl+Z at the very end is an old DOS EOF.
        let mut crs: usize = 0;
        let mut lfs: usize = 0;
        let mut w: usize = 0;
        let mut end = n;
        if end > 0 && TEXT[end - 1] == 0x1A {
            end -= 1;
        }
        for i in 0..end {
            let c = TEXT[i];
            if c == b'\r' && i + 1 < end && TEXT[i + 1] == b'\n' {
                crs += 1;
                continue;
            }
            if c == b'\n' {
                lfs += 1;
            }
            TEXT[w] = c;
            w += 1;
        }
        CRLF = lfs == 0 || crs * 2 >= lfs;
        FINAL_NL = w > 0 && TEXT[w - 1] == b'\n';
        if !FINAL_NL {
            TEXT[w] = b'\n';
            w += 1;
        }
        LEN = w;
        CUR = 0;
        TOP = 0;
        LEFT = 0;
        SEL = false;
        DIRTY = false;
        ULEN = 0;
        LAST_KIND = 0;
        full_layout();
        0
    }
}

fn save_to(name: &str) -> bool {
    unsafe {
        let h = f_create(name);
        if h < 0 {
            return false;
        }
        IOLEN = 0;
        let end = if FINAL_NL { LEN } else { LEN - 1 };
        let mut ok = true;
        for i in 0..end {
            let c = TEXT[i];
            if c == b'\n' && CRLF {
                ok = ok && out_byte(h, b'\r');
            }
            ok = ok && out_byte(h, c);
        }
        ok = ok && out_flush(h);
        f_close(h);
        ok
    }
}

fn save() -> bool {
    unsafe {
        if !NAMED {
            return save_as();
        }
        if save_to(fname()) {
            DIRTY = false;
            set_msg("Saved");
            true
        } else {
            alert("Error", "The file could not be saved:", fname());
            false
        }
    }
}

fn save_as() -> bool {
    unsafe {
        if !file_dialog("Save As", true) {
            return false;
        }
        // Add .TXT when the file name (after the folder) has no extension.
        let t = input_text().as_bytes();
        let mut s = t.len();
        while s > 0 && t[s - 1] != b'\\' {
            s -= 1;
        }
        let ext = str_from_bytes(&t[s..]).contains(".");
        sb_clear();
        sb_str(input_text());
        if !ext {
            sb_str(".TXT");
        }
        set_input(sb());
        let name = input_text();
        if exists(name) && message_box("Save As", name, "already exists. Replace it?", "  ~Y~es  |  ~N~o  ") != 0 {
            return false;
        }
        set_fname(name);
        NAMED = true;
        browse_folder_of(name);
        save()
    }
}

fn confirm_discard() -> bool {
    unsafe {
        if !DIRTY {
            return true;
        }
        sb_clear();
        if NAMED {
            sb_str(fname());
        } else {
            sb_str("Untitled");
        }
        sb_str(" has changed.");
        match message_box("Text", sb(), "Save the changes?", "  ~Y~es  |  ~N~o  | Cancel ") {
            0 => save(),
            1 => true,
            _ => false,
        }
    }
}

fn open_name(n: &str) {
    unsafe {
        sb_clear();
        for c in n.bytes() {
            sb_char(c.to_ascii_uppercase());
        }
        set_input(sb());
        let name = input_text();
        browse_folder_of(name);
        match load(name) {
            0 => {
                set_fname(name);
                NAMED = true;
            }
            -2 => {
                alert("Open", "The file is too large (the limit is 2 MB):", name);
                reset_doc();
            }
            _ => {
                // A new file with this name.
                reset_doc();
                set_fname(name);
                NAMED = true;
                set_msg("New file");
            }
        }
    }
}

// ----------------------------------------------------- disk browser

// The folder being browsed: "X:\" or "X:\DIR\SUB\" (always ends in '\').
static mut BDIR: [u8; 128] = [0; 128];
static mut BDLEN: usize = 0;
// A path being built (by full_path and the listing).
static mut PATHBUF: [u8; 160] = [0; 160];
static mut PATHLEN: usize = 0;
static mut RAW: [u8; 240] = [0; 240];
// Which files the list shows.
static mut FILTER: [u8; 16] = [0; 16];
static mut FILTERLEN: usize = 0;

fn bdir() -> &'static str {
    unsafe { str_from_bytes(&BDIR[..BDLEN]) }
}

fn path_str() -> &'static str {
    unsafe { str_from_bytes(&PATHBUF[..PATHLEN]) }
}

fn filter() -> &'static str {
    unsafe { str_from_bytes(&FILTER[..FILTERLEN]) }
}

fn path_char(c: u8) {
    unsafe {
        if PATHLEN < 160 {
            PATHBUF[PATHLEN] = c.to_ascii_uppercase();
            PATHLEN += 1;
        }
    }
}

fn path_add(s: &str) {
    for c in s.bytes() {
        path_char(c);
    }
}

// Browse the folder in PATHBUF.
fn enter_folder() {
    unsafe {
        if PATHLEN == 0 || PATHBUF[PATHLEN - 1] != b'\\' {
            path_char(b'\\');
        }
        BDLEN = if PATHLEN > 128 { 128 } else { PATHLEN };
        copy_bytes(BDIR.as_mut_ptr(), PATHBUF.as_ptr(), BDLEN);
    }
}

fn current_drive() -> u32 {
    let mut r = Regs::new();
    r.eax = 0x1900;
    dos_int(0x21, &mut r);
    r.eax & 0xFF
}

// Start browsing in the current drive and directory.
fn browse_cwd() {
    unsafe {
        PATHLEN = 0;
        path_char(b'A' + current_drive() as u8);
        path_add(":\\");
        let x = dos_buffer();
        *x = 0;
        let mut q = Regs::new();
        q.eax = 0x4700;
        q.ds = dos_buffer_seg();
        dos_int(0x21, &mut q);
        if !q.carry() {
            let mut i: usize = if *x == b'\\' { 1 } else { 0 };
            while i < 64 && *x.add(i) != 0 {
                path_char(*x.add(i));
                i += 1;
            }
        }
        enter_folder();
    }
}

// TinyDOS marks the drives it found at boot; selecting a missing one
// leaves the current drive unchanged.
fn drive_exists(d: u32) -> bool {
    let cur = current_drive();
    let mut s = Regs::new();
    s.eax = 0x0E00;
    s.edx = d;
    dos_int(0x21, &mut s);
    let ok = current_drive() == d;
    let mut b = Regs::new();
    b.eax = 0x0E00;
    b.edx = cur;
    dos_int(0x21, &mut b);
    ok
}

// Turn a name typed or picked in the browser into a full, tidy path in
// PATHBUF: relative names start in BDIR; "." and ".." are resolved.
// Returns true if the name can only be a folder (ends in \ or :, or is ..).
fn full_path(name: &str) -> bool {
    unsafe {
        let b = name.as_bytes();
        let mut n: usize = 0;
        let mut drive = BDIR[0];
        let mut s: usize = 0;
        let mut root = false;
        if b.len() >= 2 && b[1] == b':' {
            drive = b[0];
            s = 2;
            root = true;
        }
        if s < b.len() && b[s] == b'\\' {
            root = true;
        }
        if !root {
            for i in 3..BDLEN {
                RAW[n] = BDIR[i];
                n += 1;
            }
        }
        for i in s..b.len() {
            if n < 240 {
                RAW[n] = b[i];
                n += 1;
            }
        }
        let last = if b.len() > 0 { b[b.len() - 1] } else { b'\\' };
        let folder = last == b'\\' || last == b':' || name == "." || name == ".." || name.ends_with("\\..");
        PATHLEN = 0;
        path_char(drive);
        path_add(":\\");
        let mut i: usize = 0;
        while i < n {
            while i < n && RAW[i] == b'\\' {
                i += 1;
            }
            let st = i;
            while i < n && RAW[i] != b'\\' {
                i += 1;
            }
            let len = i - st;
            if len == 0 || (len == 1 && RAW[st] == b'.') {
                continue;
            }
            if len == 2 && RAW[st] == b'.' && RAW[st + 1] == b'.' {
                // Up one folder.
                if PATHLEN > 3 {
                    PATHLEN -= 1;
                    while PATHLEN > 3 && PATHBUF[PATHLEN - 1] != b'\\' {
                        PATHLEN -= 1;
                    }
                }
                continue;
            }
            for k in st..i {
                path_char(RAW[k]);
            }
            path_char(b'\\');
        }
        if !folder && PATHLEN > 3 {
            PATHLEN -= 1;
        }
        folder
    }
}

// Find-first into the DTA at dos_buffer() + 256; returns the DOS error (0 = found).
fn find_first(pattern: &str, attr: u32) -> u32 {
    let mut r = Regs::new();
    r.eax = 0x1A00;
    r.edx = 256;
    r.ds = dos_buffer_seg();
    dos_int(0x21, &mut r);
    put_name(0, pattern);
    let mut q = Regs::new();
    q.eax = 0x4E00;
    q.ecx = attr;
    q.ds = dos_buffer_seg();
    dos_int(0x21, &mut q);
    if q.carry() { q.eax & 0xFFFF } else { 0 }
}

fn find_more() -> bool {
    let mut q = Regs::new();
    q.eax = 0x4F00;
    q.ds = dos_buffer_seg();
    dos_int(0x21, &mut q);
    !q.carry()
}

fn found_attr() -> u8 {
    unsafe { *dos_buffer().add(256 + 21) }
}

// The found name into the string builder.
fn found_name() {
    let x = dos_buffer();
    sb_clear();
    let mut i: usize = 0;
    unsafe {
        while i < 13 && *x.add(256 + 30 + i) != 0 {
            sb_char(*x.add(256 + 30 + i));
            i += 1;
        }
    }
}

fn is_dir(path: &str) -> bool {
    find_first(path, 0x10) == 0 && found_attr() & 0x10 != 0
}

// List kinds (kept in the list's position field, and sorted by it).
const L_UP: u32 = 0;
const L_DIR: u32 = 1;
const L_FILE: u32 = 2;
const L_DRIVE: u32 = 3;

// The browsed folder: "..", its folders, the files matching the filter,
// then the drives.
fn browse_list() {
    unsafe {
        list_clear();
        if BDLEN > 3 {
            sb_clear();
            sb_str("..\\");
            list_add(L_UP);
        }
        PATHLEN = 0;
        path_add(bdir());
        path_add("*.*");
        if find_first(path_str(), 0x10) == 0 {
            loop {
                let a = found_attr();
                found_name();
                if a & 0x10 != 0 && SB[0] != b'.' {
                    sb_char(b'\\');
                    list_add(L_DIR);
                }
                if !find_more() {
                    break;
                }
            }
        }
        PATHLEN = 0;
        path_add(bdir());
        path_add(filter());
        if find_first(path_str(), 0) == 0 {
            loop {
                if found_attr() & 0x18 == 0 {
                    found_name();
                    list_add(L_FILE);
                }
                if !find_more() {
                    break;
                }
            }
        }
        for d in 0..4 {
            if drive_exists(d) {
                sb_clear();
                sb_char(b'[');
                sb_char(b'A' + d as u8);
                sb_str(":]");
                list_add(L_DRIVE);
            }
        }
        list_sort();
    }
}

// Browse the folder a file is in (when its path has one).
fn browse_folder_of(name: &str) {
    unsafe {
        let b = name.as_bytes();
        let mut s = b.len();
        while s > 0 && b[s - 1] != b'\\' {
            s -= 1;
        }
        if s >= 3 && b[1] == b':' {
            PATHLEN = 0;
            path_add(str_from_bytes(&b[..s]));
            enter_folder();
        }
    }
}

fn has_wild(s: &str) -> bool {
    s.contains("*") || s.contains("?")
}

// Open and Save As: a name field over the folder, its subfolders and the
// drives. Enter on a folder or drive goes into it; a name with * or ?
// filters the files; a path can be typed. Leaves the full path in INBUF.
fn file_dialog(title: &str, saving: bool) -> bool {
    unsafe {
        if BDLEN == 0 {
            browse_cwd();
        }
        if FILTERLEN == 0 {
            FILTERLEN = 3;
            FILTER[0] = b'*';
            FILTER[1] = b'.';
            FILTER[2] = b'*';
        }
        browse_list();
        if saving && NAMED {
            // The current name, without its folder.
            let f = fname();
            let b = f.as_bytes();
            let mut s = b.len();
            while s > 0 && b[s - 1] != b'\\' && b[s - 1] != b':' {
                s -= 1;
            }
            set_input(str_from_bytes(&b[s..]));
        } else {
            set_input(filter());
        }
        let mut sel: usize = 0;
        let mut top: usize = 0;
        // After picking from the list (or with Save As's current name),
        // typing starts a new name.
        let mut picked = saving && NAMED;
        loop {
            draw();
            dialog_box(2, 12, 21, 56, title);
            mv(4, 15);
            addhot("File ~n~ame", P_DLG, P_DLG_HOT);
            attrset(P_INPUT);
            mv(5, 15);
            hline(b' ', 36);
            // Long input scrolls: show its end.
            let it = input_text();
            let skip = if it.len() > 34 { it.len() - 34 } else { 0 };
            mvaddstr(5, 16, str_from_bytes(&it.as_bytes()[skip..]));
            attrset(P_DLG);
            let d = bdir();
            let dskip = if d.len() > 36 { d.len() - 36 } else { 0 };
            mvaddstr(7, 15, str_from_bytes(&d.as_bytes()[dskip..]));
            mv(8, 15);
            addhot("~F~iles and folders", P_DLG, P_DLG_HOT);
            attrset(P_LIST);
            fill(9, 15, 12, 36, b' ');
            if sel < top {
                top = sel;
            }
            if sel >= top + 12 {
                top = sel - 11;
            }
            for r in 0..12 {
                let i = top + r;
                if i >= LN {
                    break;
                }
                attrset(if i == sel { P_LIST_SEL } else { P_LIST });
                mv(9 + r as i32, 15);
                hline(b' ', 36);
                mvaddstr(9 + r as i32, 16, list_item(i));
            }
            if LN == 0 {
                mvaddstr(9, 16, "(empty)");
            }
            button(5, 54, if saving { " ~S~ave  " } else { "  ~O~K  " }, true);
            button(8, 54, " Cancel ", false);
            attrset(P_DLG);
            mvaddstr(21, 15, "Enter: open a folder or file   Esc: cancel");
            curs_set(true);
            mv(5, 16 + (it.len() - skip) as i32);
            refresh();
            let k = getch();
            let last = if LN > 0 { LN - 1 } else { 0 };
            match k {
                KEY_ESC => return false,
                KEY_ENTER => {
                    let mut name = input_text();
                    if has_wild(name) {
                        if name != filter() {
                            // A new filter.
                            FILTERLEN = if INLEN > 16 { 16 } else { INLEN };
                            copy_bytes(FILTER.as_mut_ptr(), INBUF.as_ptr(), FILTERLEN);
                            for j in 0..FILTERLEN {
                                FILTER[j] = FILTER[j].to_ascii_uppercase();
                            }
                            browse_list();
                            sel = 0;
                            continue;
                        }
                        if LN == 0 {
                            continue;
                        }
                        name = list_item(sel);
                    }
                    if INLEN == 0 && !has_wild(name) {
                        continue;
                    }
                    // A drive from the list: "[C:]".
                    if name.len() == 4 && name.as_bytes()[0] == b'[' {
                        PATHLEN = 0;
                        path_char(name.as_bytes()[1]);
                        path_add(":\\");
                        enter_folder();
                        browse_list();
                        set_input(filter());
                        sel = 0;
                        continue;
                    }
                    let folder = full_path(name);
                    if folder || is_dir(path_str()) {
                        enter_folder();
                        browse_list();
                        set_input(filter());
                        sel = 0;
                        continue;
                    }
                    set_input(path_str());
                    return true;
                }
                KEY_UP => {
                    if sel > 0 {
                        sel -= 1;
                    }
                }
                KEY_DOWN => {
                    if sel < last {
                        sel += 1;
                    }
                }
                KEY_PGUP => sel = if sel > 12 { sel - 12 } else { 0 },
                KEY_PGDN => sel = if sel + 12 < last { sel + 12 } else { last },
                KEY_HOME => sel = 0,
                KEY_END => sel = last,
                KEY_BKSP => {
                    picked = false;
                    if INLEN > 0 {
                        INLEN -= 1;
                    }
                }
                _ => {
                    if k > 32 && k < 127 && INLEN < 78 {
                        if picked || (has_wild(input_text()) && input_text() == filter()) {
                            INLEN = 0;
                            picked = false;
                        }
                        INBUF[INLEN] = (k as u8).to_ascii_uppercase();
                        INLEN += 1;
                    }
                }
            }
            // Moving in the list shows the item in the name field.
            if (k == KEY_UP || k == KEY_DOWN || k == KEY_PGUP || k == KEY_PGDN || k == KEY_HOME || k == KEY_END) && LN > 0 {
                set_input(list_item(sel));
                picked = true;
            }
        }
    }
}

fn load_options() {
    let mut b = [0u8; 8];
    let n = file_read("TEXT.CFG", &mut b);
    unsafe {
        if n >= 5 && b[0] == b'T' {
            WRAP = b[1] != 0;
            LINE_NUMBERS = b[2] != 0;
            SHOW_STATUS = b[3] != 0;
            TABW = b[4] as usize;
            if TABW != 2 && TABW != 4 && TABW != 8 {
                TABW = 8;
            }
        }
    }
}

fn save_options() {
    let mut b = [0u8; 5];
    unsafe {
        b[0] = b'T';
        b[1] = WRAP as u8;
        b[2] = LINE_NUMBERS as u8;
        b[3] = SHOW_STATUS as u8;
        b[4] = TABW as u8;
    }
    file_write("TEXT.CFG", &b);
}

// ===================================================== 5. search and commands

static mut FINDBUF: [u8; 64] = [0; 64];
static mut FINDLEN: usize = 0;

fn matches_at(i: usize) -> bool {
    unsafe {
        if i + FINDLEN > LEN {
            return false;
        }
        for k in 0..FINDLEN {
            if TEXT[i + k].to_ascii_lowercase() != FINDBUF[k].to_ascii_lowercase() {
                return false;
            }
        }
        true
    }
}

// Find (case-insensitive) after the cursor, wrapping at the end.
fn find_next() -> bool {
    unsafe {
        if FINDLEN == 0 {
            return false;
        }
        let start = if has_sel() { sel_a() + 1 } else { CUR };
        let mut i = start;
        while i + FINDLEN <= LEN {
            if matches_at(i) {
                ANCHOR = i;
                CUR = i + FINDLEN;
                SEL = true;
                return true;
            }
            i += 1;
        }
        i = 0;
        while i < start && i + FINDLEN <= LEN {
            if matches_at(i) {
                ANCHOR = i;
                CUR = i + FINDLEN;
                SEL = true;
                set_msg("Search wrapped to the top");
                return true;
            }
            i += 1;
        }
        sb_clear();
        sb_str("Cannot find \"");
        sb_str(str_from_bytes(&FINDBUF[..FINDLEN]));
        sb_str("\"");
        set_msg(sb());
        false
    }
}

fn find_dialog() {
    unsafe {
        if input_box("Find", "Find ~w~hat", str_from_bytes(&FINDBUF[..FINDLEN])) && INLEN > 0 {
            FINDLEN = INLEN;
            copy_bytes(FINDBUF.as_mut_ptr(), INBUF.as_ptr(), INLEN);
            find_next();
        }
    }
}

fn replace_dialog() {
    unsafe {
        if !input_box("Replace", "Find ~w~hat", str_from_bytes(&FINDBUF[..FINDLEN])) || INLEN == 0 {
            return;
        }
        FINDLEN = INLEN;
        copy_bytes(FINDBUF.as_mut_ptr(), INBUF.as_ptr(), INLEN);
        if !input_box("Replace", "Replace ~w~ith", "") {
            return;
        }
        let mut rep = [0u8; 64];
        let rl = INLEN;
        copy_bytes(rep.as_mut_ptr(), INBUF.as_ptr(), rl);
        undo_mark(0);
        let mut count: u32 = 0;
        let mut i: usize = 0;
        while i + FINDLEN <= LEN {
            if matches_at(i) {
                del_bytes(i, FINDLEN);
                if rl > 0 && !ins_bytes(i, rep.as_ptr(), rl) {
                    break;
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
        alert("Replace", sb(), "");
    }
}

fn goto_line() {
    unsafe {
        if input_box("Go To Line", "~L~ine number", "") {
            let n = parse_int(input_text());
            if n >= 1 {
                // The first row of that line.
                let target = (n - 1) as u32;
                let mut lo: usize = 0;
                let mut hi = NR;
                while lo < hi {
                    let mid = (lo + hi) / 2;
                    if RLINE[mid] < target {
                        lo = mid + 1;
                    } else {
                        hi = mid;
                    }
                }
                if lo >= NR {
                    lo = NR - 1;
                }
                CUR = RSTART[lo] as usize;
                SEL = false;
            }
        }
    }
}

// F5: insert the time and date, like Notepad.
fn time_date() {
    let mut t = Regs::new();
    t.eax = 0x2C00;
    dos_int(0x21, &mut t);
    let mut d = Regs::new();
    d.eax = 0x2A00;
    dos_int(0x21, &mut d);
    let mut b = [0u8; 16];
    let hh = (t.ecx >> 8) & 0xFF;
    let mm = t.ecx & 0xFF;
    let year = d.ecx & 0xFFFF;
    let month = (d.edx >> 8) & 0xFF;
    let day = d.edx & 0xFF;
    b[0] = b'0' + (hh / 10) as u8;
    b[1] = b'0' + (hh % 10) as u8;
    b[2] = b':';
    b[3] = b'0' + (mm / 10) as u8;
    b[4] = b'0' + (mm % 10) as u8;
    b[5] = b' ';
    b[6] = b'0' + (year / 1000) as u8;
    b[7] = b'0' + (year / 100 % 10) as u8;
    b[8] = b'0' + (year / 10 % 10) as u8;
    b[9] = b'0' + (year % 10) as u8;
    b[10] = b'-';
    b[11] = b'0' + (month / 10) as u8;
    b[12] = b'0' + (month % 10) as u8;
    b[13] = b'-';
    b[14] = b'0' + (day / 10) as u8;
    b[15] = b'0' + (day % 10) as u8;
    unsafe {
        delete_selection();
        undo_mark(0);
        if ins_bytes(CUR, b.as_ptr(), 16) {
            CUR += 16;
        }
    }
}

fn tab_width() {
    unsafe {
        TABW = match TABW {
            8 => 4,
            4 => 2,
            _ => 8,
        };
        sb_clear();
        sb_str("Tab width: ");
        sb_num(TABW as u32);
        set_msg(sb());
        full_layout();
        save_options();
    }
}

fn help() {
    list_clear();
    sb_clear(); sb_str("FILES    Ctrl+N new  Ctrl+O open  Ctrl+S save  Alt+X exit"); list_add(0);
    sb_clear(); sb_str("EDIT     Shift+arrows select   Ctrl+X/C/V cut/copy/paste"); list_add(0);
    sb_clear(); sb_str("         Ctrl+Z undo  Ctrl+A select all  Del delete"); list_add(0);
    sb_clear(); sb_str("         Ins insert/overwrite  Tab inserts a tab"); list_add(0);
    sb_clear(); sb_str("MOVE     Ctrl+Left/Right word  Home/End line"); list_add(0);
    sb_clear(); sb_str("         Ctrl+Home/End file  PgUp/PgDn page"); list_add(0);
    sb_clear(); sb_str("SEARCH   Ctrl+F find  F3 next  Ctrl+H replace  Ctrl+G line"); list_add(0);
    sb_clear(); sb_str("OTHER    F5 time/date  Ctrl+W word wrap  F10 menu"); list_add(0);
    sb_clear(); list_add(0);
    sb_clear(); sb_str("Text keeps files exactly as they are: CR LF or LF line ends,"); list_add(0);
    sb_clear(); sb_str("tabs, and a missing last newline. Files up to 2 MB."); list_add(0);
    sb_clear(); sb_str("View settings are saved in TEXT.CFG."); list_add(0);
    list_dialog("Keys", false, 0);
}

fn command(c: u32) -> bool {
    unsafe {
        match c {
            1 => {
                if confirm_discard() {
                    reset_doc();
                    NAMED = false;
                }
            }
            2 => {
                if confirm_discard() && file_dialog("Open", false) {
                    open_name(input_text());
                }
            }
            3 => {
                save();
            }
            4 => {
                save_as();
            }
            8 => return confirm_discard(),
            10 => undo(),
            11 => {
                if copy_sel() {
                    delete_selection();
                }
            }
            12 => {
                copy_sel();
            }
            13 => paste(),
            14 => {
                ANCHOR = 0;
                CUR = LEN - 1;
                SEL = true;
            }
            16 => {
                if !delete_selection() {
                    undo_mark(2);
                    del_bytes(CUR, 1);
                }
            }
            17 => time_date(),
            20 => find_dialog(),
            21 => {
                if FINDLEN == 0 {
                    find_dialog();
                } else {
                    find_next();
                }
            }
            22 => replace_dialog(),
            23 => goto_line(),
            30 => {
                WRAP = !WRAP;
                full_layout();
                save_options();
            }
            31 => {
                LINE_NUMBERS = !LINE_NUMBERS;
                full_layout();
                save_options();
            }
            32 => {
                SHOW_STATUS = !SHOW_STATUS;
                save_options();
            }
            33 => tab_width(),
            80 => help(),
            81 => alert("About Text", "Text 1.0 - a plain text editor for TinyDOS,", "written in R++ with Rustland R++ on rusty486."),
            _ => {}
        }
        false
    }
}

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

// Up/down by n rows, keeping the screen column.
fn move_rows_by(d: i32) {
    unsafe {
        let r = row_of(CUR) as i32;
        let mut t = r + d;
        if t < 0 {
            t = 0;
        }
        if t >= NR as i32 {
            t = NR as i32 - 1;
        }
        CUR = pos_at_x(t as usize, WANT_X);
    }
}

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
            if s == 0x2D {
                return command(8);
            }
            if s == 0x0E {
                return command(10);
            }
            return false;
        }
        let c = match k {
            1 => 14,
            3 => 12,
            6 => 20,
            7 => 23,
            8 => 22,
            14 => 1,
            15 => 2,
            19 => 3,
            22 => 13,
            23 => 30,
            24 => 11,
            26 => 10,
            KEY_F1 => 80,
            KEY_F3 => 21,
            KEY_F5 => 17,
            _ => 0,
        };
        if c != 0 {
            return command(c as u32);
        }
        let vertical = k == KEY_UP || k == KEY_DOWN || k == KEY_PGUP || k == KEY_PGDN;
        match k {
            KEY_ESC => SEL = false,
            KEY_ENTER => type_key(b'\n'),
            KEY_TAB => type_key(b'\t'),
            KEY_BKSP => {
                if !delete_selection() && CUR > 0 {
                    undo_mark(2);
                    CUR -= 1;
                    del_bytes(CUR, 1);
                }
            }
            KEY_DEL => {
                command(16);
            }
            KEY_INS => OVERWRITE = !OVERWRITE,
            KEY_LEFT => {
                begin_move();
                if CUR > 0 {
                    CUR -= 1;
                }
            }
            KEY_RIGHT => {
                begin_move();
                if CUR + 1 < LEN {
                    CUR += 1;
                }
            }
            KEY_UP => {
                begin_move();
                move_rows_by(-1);
            }
            KEY_DOWN => {
                begin_move();
                move_rows_by(1);
            }
            KEY_PGUP => {
                begin_move();
                move_rows_by(0 - text_h() as i32);
                TOP = if TOP > text_h() { TOP - text_h() } else { 0 };
            }
            KEY_PGDN => {
                begin_move();
                move_rows_by(text_h() as i32);
            }
            KEY_HOME => {
                begin_move();
                CUR = RSTART[row_of(CUR)] as usize;
            }
            KEY_END => {
                begin_move();
                let r = row_of(CUR);
                let e = (RSTART[r] + RLEN[r]) as usize;
                // A wrapped row ends before the next row's first character.
                CUR = if r + 1 < NR && RFIRST[r + 1] == 0 && e > 0 { e - 1 } else { e };
            }
            0x34B => {
                begin_move();
                while CUR > 0 && !TEXT[CUR - 1].is_ascii_alphanumeric() {
                    CUR -= 1;
                }
                while CUR > 0 && TEXT[CUR - 1].is_ascii_alphanumeric() {
                    CUR -= 1;
                }
            }
            0x34D => {
                begin_move();
                while CUR + 1 < LEN && TEXT[CUR].is_ascii_alphanumeric() {
                    CUR += 1;
                }
                while CUR + 1 < LEN && !TEXT[CUR].is_ascii_alphanumeric() {
                    CUR += 1;
                }
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
                if k >= 32 && k < 256 && k != 127 {
                    type_key(k as u8);
                }
            }
        }
        // Remember the column for up/down movement.
        if !vertical {
            let r = row_of(CUR);
            WANT_X = col_x(r, CUR);
        }
        false
    }
}

fn main() {
    set_video_mode(3);
    colors();
    load_options();
    reset_doc();
    // Names on the command line may be relative or full paths.
    browse_cwd();
    let a = args();
    if a.len() > 0 {
        full_path(a);
        open_name(path_str());
    }
    loop {
        draw();
        show_editor();
        let mut quit = handle(getch());
        while !quit && key_pressed() {
            quit = handle(getch());
        }
        if quit {
            break;
        }
    }
    set_video_mode(3);
}
