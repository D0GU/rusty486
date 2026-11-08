//! The R++ runtime library, written in R++ itself and compiled into every
//! program. Names starting with `__` are used by the compiler.

pub const SOURCE: &str = r#"
// ---------------------------------------------------------------- DOS access

/// Registers for `dos_int` (the layout the PM stub expects).
struct Regs {
    eax: u32, ebx: u32, ecx: u32, edx: u32,
    esi: u32, edi: u32, ebp: u32,
    ds: u16, es: u16, flags: u16, pad: u16,
}

impl Regs {
    fn new() -> Regs {
        Regs { eax: 0, ebx: 0, ecx: 0, edx: 0, esi: 0, edi: 0, ebp: 0, ds: 0, es: 0, flags: 0, pad: 0 }
    }
    /// Carry flag after the call (DOS error).
    fn carry(&self) -> bool {
        self.flags & 1 == 1
    }
}

static mut HEAP: u32 = 0;
static mut HEAP_END: u32 = 0;
static mut SEED: u32 = 0x2545F491;

fn __init() {
    unsafe {
        HEAP = *((__systab() + 24) as *const u32);
        HEAP_END = *((__systab() + 28) as *const u32);
        SEED = SEED ^ ticks();
    }
}

/// Call real-mode interrupt `n` (DOS: 0x21, video BIOS: 0x10, ...).
fn dos_int(n: u32, r: &mut Regs) {
    __int(n, r);
}

/// The 32 KB buffer in conventional memory shared with DOS.
fn dos_buffer() -> *mut u8 {
    unsafe { *((__systab() + 8) as *const u32) as *mut u8 }
}

/// Real-mode segment of `dos_buffer()`.
fn dos_buffer_seg() -> u16 {
    unsafe { *((__systab() + 12) as *const u32) as u16 }
}

fn exit(code: i32) -> ! {
    let mut r = Regs::new();
    r.eax = 0x4C00 | (code as u32 & 0xFF);
    __int(0x21, &mut r);
    loop {}
}

/// The command line after the program name.
fn args() -> &str {
    let p = unsafe { *((__systab() + 20) as *const u32) } as *const u8;
    let n = unsafe { *p } as usize;
    str_from_bytes(slice_from_raw(p.add(1), n)).trim()
}

// ---------------------------------------------------------------- output

fn __write(handle: u32, p: *const u8, n: usize) {
    let buf = dos_buffer();
    let mut i: usize = 0;
    while i < n {
        let mut k: usize = 0;
        while i < n && k < 32000 {
            let c = unsafe { *p.add(i) };
            if c == b'\n' && (i == 0 || unsafe { *p.add(i - 1) } != b'\r') {
                unsafe { *buf.add(k) = b'\r'; }
                k += 1;
            }
            unsafe { *buf.add(k) = c; }
            k += 1;
            i += 1;
        }
        let mut r = Regs::new();
        r.eax = 0x4000;
        r.ebx = handle;
        r.ecx = k as u32;
        r.ds = dos_buffer_seg();
        __int(0x21, &mut r);
    }
}

fn __print_str(s: &str) {
    __write(1, s.as_ptr(), s.len());
}

fn __pad(n: u32, c: &str) {
    let mut i: u32 = 0;
    while i < n {
        __print_str(c);
        i += 1;
    }
}

// flags: bit 0 = zero padding, bits 1-2 = alignment (0 default, 1 <, 2 >, 3 ^)
fn __print_str_w(s: &str, width: u32, flags: u32) {
    let len = s.len() as u32;
    let align = flags >> 1;
    let pad = if width > len { width - len } else { 0 };
    if align == 2 {
        __pad(pad, " ");
    } else if align == 3 {
        __pad(pad / 2, " ");
    }
    __print_str(s);
    if align == 0 || align == 1 {
        __pad(pad, " ");
    } else if align == 3 {
        __pad(pad - pad / 2, " ");
    }
}

fn __print_int(v: u32, signed: bool, radix: u32, upper: bool, width: u32, flags: u32) {
    let mut buf = [0u8; 34];
    let mut i: usize = 34;
    let neg = signed && (v as i32) < 0;
    let mut n = if neg { (0 - (v as i32)) as u32 } else { v };
    loop {
        let d = (n % radix) as u8;
        i -= 1;
        buf[i] = if d < 10 { b'0' + d } else if upper { b'A' + d - 10 } else { b'a' + d - 10 };
        n = n / radix;
        if n == 0 {
            break;
        }
    }
    let digits = (34 - i) as u32;
    let total = if neg { digits + 1 } else { digits };
    let zero = flags & 1 == 1;
    let align = flags >> 1;
    let pad = if width > total { width - total } else { 0 };
    if zero {
        if neg {
            __print_str("-");
        }
        __pad(pad, "0");
    } else {
        if align == 0 || align == 2 {
            __pad(pad, " ");
        } else if align == 3 {
            __pad(pad / 2, " ");
        }
        if neg {
            __print_str("-");
        }
    }
    __print_str(str_from_bytes(&buf[i..]));
    if !zero {
        if align == 1 {
            __pad(pad, " ");
        } else if align == 3 {
            __pad(pad - pad / 2, " ");
        }
    }
}

fn __print_char(c: char) {
    let mut b = [0u8; 1];
    b[0] = if (c as u32) < 256 { c as u8 } else { b'?' };
    __write(1, b.as_ptr(), 1);
}

fn __print_bool(b: bool, width: u32, flags: u32) {
    if b {
        __print_str_w("true", width, flags);
    } else {
        __print_str_w("false", width, flags);
    }
}

fn __str_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let x = a.as_bytes();
    let y = b.as_bytes();
    let mut i: usize = 0;
    while i < x.len() {
        if x[i] != y[i] {
            return false;
        }
        i += 1;
    }
    true
}

fn __panic(msg: &str, line: u32, col: u32) -> ! {
    print!("thread 'main' panicked at {}:{}:\n{}\n", line, col, msg);
    exit(101)
}

fn __panic_bounds(idx: usize, len: usize, line: u32, col: u32) -> ! {
    print!("thread 'main' panicked at {}:{}:\nindex out of bounds: the len is {} but the index is {}\n", line, col, len, idx);
    exit(101)
}

// ---------------------------------------------------------------- input

/// Read a line from standard input into `buf`; returns its length (without
/// the line ending).
fn read_line(buf: &mut [u8]) -> usize {
    let x = dos_buffer();
    let mut n: usize = 0;
    loop {
        let mut r = Regs::new();
        r.eax = 0x3F00;
        r.ecx = 1;
        r.ds = dos_buffer_seg();
        __int(0x21, &mut r);
        if r.eax == 0 || r.carry() {
            break;
        }
        let c = unsafe { *x };
        if c == b'\n' {
            break;
        }
        if c != b'\r' && n < buf.len() {
            buf[n] = c;
            n += 1;
        }
    }
    n
}

/// Wait for a key: (scan code << 8) | ASCII.
fn read_key() -> u32 {
    let mut r = Regs::new();
    __int(0x16, &mut r);
    r.eax & 0xFFFF
}

/// Whether a key is waiting (see `read_key`).
fn key_pressed() -> bool {
    let mut r = Regs::new();
    r.eax = 0x0100;
    __int(0x16, &mut r);
    r.flags & 0x40 == 0
}

/// Parse a decimal integer (leading spaces and a sign allowed).
fn parse_int(s: &str) -> i32 {
    let b = s.trim().as_bytes();
    let mut i: usize = 0;
    let mut neg = false;
    if b.len() > 0 && (b[0] == b'-' || b[0] == b'+') {
        neg = b[0] == b'-';
        i = 1;
    }
    let mut v: i32 = 0;
    while i < b.len() && b[i].is_ascii_digit() {
        v = v * 10 + (b[i] - b'0') as i32;
        i += 1;
    }
    if neg { -v } else { v }
}

// ---------------------------------------------------------------- screen

/// Set a video mode: 3 = 80x25 text, 0x13 = 320x200 in 256 colours.
fn set_video_mode(mode: u32) {
    let mut r = Regs::new();
    r.eax = mode & 0xFF;
    __int(0x10, &mut r);
}

/// Plot a pixel in mode 0x13.
fn put_pixel(x: i32, y: i32, color: u8) {
    if x >= 0 && x < 320 && y >= 0 && y < 200 {
        unsafe { *((0xA0000 + y * 320 + x) as *mut u8) = color; }
    }
}

fn get_pixel(x: i32, y: i32) -> u8 {
    if x >= 0 && x < 320 && y >= 0 && y < 200 {
        unsafe { *((0xA0000 + y * 320 + x) as *const u8) }
    } else {
        0
    }
}

/// Fill a rectangle in mode 0x13.
fn fill_rect(x: i32, y: i32, w: i32, h: i32, color: u8) {
    let mut j = y;
    while j < y + h {
        let mut i = x;
        while i < x + w {
            put_pixel(i, j, color);
            i += 1;
        }
        j += 1;
    }
}

/// Clear the text screen and home the cursor.
fn clear_screen() {
    let mut r = Regs::new();
    r.eax = 0x0600;
    r.ebx = 0x0700;
    r.edx = 0x184F;
    __int(0x10, &mut r);
    gotoxy(0, 0);
}

/// Move the text cursor (column, row).
fn gotoxy(x: u32, y: u32) {
    let mut r = Regs::new();
    r.eax = 0x0200;
    r.edx = (y << 8) | (x & 0xFF);
    __int(0x10, &mut r);
}

/// Write a character with a colour attribute straight to text video memory.
fn put_char_at(x: u32, y: u32, c: u8, attr: u8) {
    if x < 80 && y < 25 {
        let p = (0xB8000 + (y * 80 + x) * 2) as *mut u8;
        unsafe {
            *p = c;
            *p.add(1) = attr;
        }
    }
}

/// Write a string with a colour attribute straight to text video memory.
fn put_str_at(x: u32, y: u32, s: &str, attr: u8) {
    let b = s.as_bytes();
    let mut i: usize = 0;
    while i < b.len() {
        put_char_at(x + i as u32, y, b[i], attr);
        i += 1;
    }
}

// ---------------------------------------------------------------- time and sound

/// BIOS timer ticks since midnight (18.2 per second).
fn ticks() -> u32 {
    unsafe { *(0x46C as *const u32) }
}

fn sleep_ms(ms: u32) {
    let us = ms * 1000;
    let mut r = Regs::new();
    r.eax = 0x8600;
    r.ecx = us >> 16;
    r.edx = us & 0xFFFF;
    __int(0x15, &mut r);
}

/// Start a tone on the PC speaker.
fn sound_on(freq: u32) {
    if freq == 0 {
        return;
    }
    let div = 1193182 / freq;
    outb(0x43, 0xB6);
    outb(0x42, (div & 0xFF) as u8);
    outb(0x42, (div >> 8) as u8);
    outb(0x61, inb(0x61) | 3);
}

fn sound_off() {
    outb(0x61, inb(0x61) & 0xFC);
}

fn beep(freq: u32, ms: u32) {
    sound_on(freq);
    sleep_ms(ms);
    sound_off();
}

// ---------------------------------------------------------------- random, memory

fn random() -> u32 {
    unsafe {
        SEED = SEED ^ (SEED << 13);
        SEED = SEED ^ (SEED >> 17);
        SEED = SEED ^ (SEED << 5);
        SEED
    }
}

/// A random number in lo..hi.
fn random_range(lo: i32, hi: i32) -> i32 {
    if hi <= lo {
        return lo;
    }
    lo + (random() % ((hi - lo) as u32)) as i32
}

fn seed_random(s: u32) {
    unsafe { SEED = s | 1; }
}

/// Allocate `size` zeroed bytes (never freed).
fn alloc(size: usize) -> *mut u8 {
    unsafe {
        let p = (HEAP + 15) & 0xFFFFFFF0;
        if p + size as u32 > HEAP_END {
            panic!("out of memory");
        }
        HEAP = p + size as u32;
        __memset(p as *mut u8, 0, size);
        p as *mut u8
    }
}

// ---------------------------------------------------------------- files

fn __asciiz(path: &str) {
    let b = path.as_bytes();
    let x = dos_buffer();
    let mut i: usize = 0;
    while i < b.len() && i < 127 {
        unsafe { *x.add(i) = b[i]; }
        i += 1;
    }
    unsafe { *x.add(i) = 0; }
}

/// Read a file into `buf`; returns the bytes read, or -1 on error.
fn file_read(path: &str, buf: &mut [u8]) -> i32 {
    __asciiz(path);
    let mut r = Regs::new();
    r.eax = 0x3D00;
    r.ds = dos_buffer_seg();
    __int(0x21, &mut r);
    if r.carry() {
        return -1;
    }
    let h = r.eax & 0xFFFF;
    let x = dos_buffer();
    let mut total: usize = 0;
    loop {
        let mut want = buf.len() - total;
        if want == 0 {
            break;
        }
        if want > 32768 {
            want = 32768;
        }
        let mut q = Regs::new();
        q.eax = 0x3F00;
        q.ebx = h;
        q.ecx = want as u32;
        q.ds = dos_buffer_seg();
        __int(0x21, &mut q);
        let got = (q.eax & 0xFFFF) as usize;
        if q.carry() || got == 0 {
            break;
        }
        __memcpy(buf.as_mut_ptr().add(total), x, got);
        total += got;
    }
    let mut c = Regs::new();
    c.eax = 0x3E00;
    c.ebx = h;
    __int(0x21, &mut c);
    total as i32
}

/// Create or replace a file with `data`; returns false on error.
fn file_write(path: &str, data: &[u8]) -> bool {
    __asciiz(path);
    let mut r = Regs::new();
    r.eax = 0x3C00;
    r.ds = dos_buffer_seg();
    __int(0x21, &mut r);
    if r.carry() {
        return false;
    }
    let h = r.eax & 0xFFFF;
    let x = dos_buffer();
    let mut done: usize = 0;
    let mut ok = true;
    while done < data.len() {
        let mut n = data.len() - done;
        if n > 32768 {
            n = 32768;
        }
        __memcpy(x, data.as_ptr().add(done), n);
        let mut q = Regs::new();
        q.eax = 0x4000;
        q.ebx = h;
        q.ecx = n as u32;
        q.ds = dos_buffer_seg();
        __int(0x21, &mut q);
        if q.carry() {
            ok = false;
            break;
        }
        done += n;
    }
    let mut c = Regs::new();
    c.eax = 0x3E00;
    c.ebx = h;
    __int(0x21, &mut c);
    ok
}

// ---------------------------------------------------------------- methods

impl u8 {
    fn is_ascii_digit(&self) -> bool { *self >= b'0' && *self <= b'9' }
    fn is_ascii_lowercase(&self) -> bool { *self >= b'a' && *self <= b'z' }
    fn is_ascii_uppercase(&self) -> bool { *self >= b'A' && *self <= b'Z' }
    fn is_ascii_alphabetic(&self) -> bool { self.is_ascii_lowercase() || self.is_ascii_uppercase() }
    fn is_ascii_alphanumeric(&self) -> bool { self.is_ascii_alphabetic() || self.is_ascii_digit() }
    fn is_ascii_whitespace(&self) -> bool { *self == b' ' || *self == b'\t' || *self == b'\n' || *self == b'\r' }
    fn is_ascii_punctuation(&self) -> bool {
        (*self >= 33 && *self <= 47) || (*self >= 58 && *self <= 64) || (*self >= 91 && *self <= 96) || (*self >= 123 && *self <= 126)
    }
    fn to_ascii_uppercase(&self) -> u8 { if self.is_ascii_lowercase() { *self - 32 } else { *self } }
    fn to_ascii_lowercase(&self) -> u8 { if self.is_ascii_uppercase() { *self + 32 } else { *self } }
}

impl char {
    fn is_ascii_digit(&self) -> bool { *self >= '0' && *self <= '9' }
    fn is_ascii_lowercase(&self) -> bool { *self >= 'a' && *self <= 'z' }
    fn is_ascii_uppercase(&self) -> bool { *self >= 'A' && *self <= 'Z' }
    fn is_ascii_alphabetic(&self) -> bool { self.is_ascii_lowercase() || self.is_ascii_uppercase() }
    fn is_alphabetic(&self) -> bool { self.is_ascii_alphabetic() }
    fn is_ascii_alphanumeric(&self) -> bool { self.is_ascii_alphabetic() || self.is_ascii_digit() }
    fn is_alphanumeric(&self) -> bool { self.is_ascii_alphanumeric() }
    fn is_numeric(&self) -> bool { self.is_ascii_digit() }
    fn is_whitespace(&self) -> bool { *self == ' ' || *self == '\t' || *self == '\n' || *self == '\r' }
    fn is_ascii_whitespace(&self) -> bool { self.is_whitespace() }
    fn to_ascii_uppercase(&self) -> char {
        if self.is_ascii_lowercase() { (*self as u32 - 32) as char } else { *self }
    }
    fn to_ascii_lowercase(&self) -> char {
        if self.is_ascii_uppercase() { (*self as u32 + 32) as char } else { *self }
    }
    fn len_utf8(&self) -> usize { 1 }
}

impl str {
    fn trim(&self) -> &str {
        let b = self.as_bytes();
        let mut s: usize = 0;
        let mut e = b.len();
        while s < e && b[s].is_ascii_whitespace() {
            s += 1;
        }
        while e > s && b[e - 1].is_ascii_whitespace() {
            e -= 1;
        }
        str_from_bytes(&b[s..e])
    }
    fn starts_with(&self, p: &str) -> bool {
        self.len() >= p.len() && __str_eq(str_from_bytes(&self.as_bytes()[..p.len()]), p)
    }
    fn ends_with(&self, p: &str) -> bool {
        self.len() >= p.len() && __str_eq(str_from_bytes(&self.as_bytes()[self.len() - p.len()..]), p)
    }
    fn contains(&self, p: &str) -> bool {
        if p.len() > self.len() {
            return false;
        }
        let mut i: usize = 0;
        while i + p.len() <= self.len() {
            if __str_eq(str_from_bytes(&self.as_bytes()[i..i + p.len()]), p) {
                return true;
            }
            i += 1;
        }
        false
    }
    fn eq_ignore_ascii_case(&self, o: &str) -> bool {
        if self.len() != o.len() {
            return false;
        }
        let a = self.as_bytes();
        let b = o.as_bytes();
        let mut i: usize = 0;
        while i < a.len() {
            if a[i].to_ascii_lowercase() != b[i].to_ascii_lowercase() {
                return false;
            }
            i += 1;
        }
        true
    }
}
"#;
