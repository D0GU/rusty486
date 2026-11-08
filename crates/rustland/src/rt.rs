//! Bare-metal runtime under the PM stub: system table, real-mode calls,
//! heap, memory builtins and DOS services.

use core::alloc::{GlobalAlloc, Layout};
use core::arch::asm;
use core::ffi::c_void;

use alloc::string::String;
use alloc::vec::Vec;

/// The table the PM stub passes to the entry point (see dos/pmstub.asm).
#[repr(C)]
pub struct SysTab {
    pub rm_int: extern "C" fn(u32, *mut Regs),
    pub psp: u32,
    pub xfer: u32,
    pub xfer_seg: u32,
    pub xfer_size: u32,
    pub cmd_tail: u32,
    pub heap_start: u32,
    pub heap_end: u32,
}

#[repr(C)]
#[derive(Clone, Copy, Default)]
pub struct Regs {
    pub eax: u32,
    pub ebx: u32,
    pub ecx: u32,
    pub edx: u32,
    pub esi: u32,
    pub edi: u32,
    pub ebp: u32,
    pub ds: u16,
    pub es: u16,
    pub flags: u16,
    pub pad: u16,
}

impl Regs {
    pub fn ax(ax: u32) -> Regs {
        Regs { eax: ax, ..Default::default() }
    }
    pub fn carry(&self) -> bool {
        self.flags & 1 != 0
    }
}

static mut SYSTAB: *const SysTab = core::ptr::null();

pub fn systab() -> &'static SysTab {
    unsafe { &*SYSTAB }
}

/// Run a real-mode software interrupt.
pub fn int(n: u8, r: &mut Regs) {
    (systab().rm_int)(n as u32, r);
    unsafe { asm!("cld") };
}

pub fn set_systab(t: *const SysTab) {
    unsafe {
        SYSTAB = t;
        HEAP.init((*t).heap_start, (*t).heap_end);
    }
}

// ------------------------------------------------------------------ heap

/// Power-of-two size classes with free lists; memory comes from a bump
/// pointer and is recycled per class.
pub struct Heap {
    next: core::cell::UnsafeCell<u32>,
    end: core::cell::UnsafeCell<u32>,
    free: core::cell::UnsafeCell<[u32; 32]>,
}

unsafe impl Sync for Heap {}

impl Heap {
    unsafe fn init(&self, start: u32, end: u32) {
        *self.next.get() = (start + 15) & !15;
        *self.end.get() = end;
    }
    fn class(l: &Layout) -> usize {
        let size = l.size().max(l.align()).max(16);
        (usize::BITS - (size - 1).leading_zeros()) as usize
    }
}

unsafe impl GlobalAlloc for Heap {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let c = Self::class(&l);
        let free = &mut *self.free.get();
        if free[c] != 0 {
            let p = free[c];
            free[c] = *(p as *const u32);
            return p as *mut u8;
        }
        let size = 1u32 << c;
        let align = (l.align() as u32).max(16);
        let p = (*self.next.get() + align - 1) & !(align - 1);
        if p.checked_add(size).is_none_or(|e| e > *self.end.get()) {
            return core::ptr::null_mut();
        }
        *self.next.get() = p + size;
        p as *mut u8
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        let c = Self::class(&l);
        let free = &mut *self.free.get();
        *(p as *mut u32) = free[c];
        free[c] = p as u32;
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new_size: usize) -> *mut u8 {
        let nl = Layout::from_size_align_unchecked(new_size, l.align());
        if Self::class(&nl) == Self::class(&l) {
            return p;
        }
        let q = self.alloc(nl);
        if !q.is_null() {
            memcpy(q as *mut c_void, p as *const c_void, l.size().min(new_size));
            self.dealloc(p, l);
        }
        q
    }
}

#[global_allocator]
pub static HEAP: Heap = Heap {
    next: core::cell::UnsafeCell::new(0),
    end: core::cell::UnsafeCell::new(0),
    free: core::cell::UnsafeCell::new([0; 32]),
};

/// Bytes of heap not yet handed out by the bump pointer.
pub fn heap_free() -> u32 {
    unsafe { *HEAP.end.get() - *HEAP.next.get() }
}

// ------------------------------------------------------------------ memory builtins

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcpy(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    // ESI is reserved by LLVM: swap the source in and out around the copy.
    asm!(
        "xchg esi, {s}",
        "rep movsb",
        "xchg esi, {s}",
        s = inout(reg) s => _,
        inout("ecx") n => _,
        inout("edi") d => _,
        options(nostack, preserves_flags)
    );
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memmove(d: *mut c_void, s: *const c_void, n: usize) -> *mut c_void {
    let (d, s) = (d as *mut u8, s as *const u8);
    if (d as usize) <= (s as usize) || (d as usize) >= (s as usize) + n {
        return memcpy(d as *mut c_void, s as *const c_void, n);
    }
    if n > 0 {
        asm!(
            "xchg esi, {s}",
            "std",
            "rep movsb",
            "cld",
            "xchg esi, {s}",
            s = inout(reg) s.add(n - 1) => _,
            inout("ecx") n => _,
            inout("edi") d.add(n - 1) => _,
            options(nostack)
        );
    }
    d as *mut c_void
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memset(d: *mut c_void, v: i32, n: usize) -> *mut c_void {
    asm!("rep stosb", inout("ecx") n => _, inout("edi") d => _, in("al") v as u8, options(nostack, preserves_flags));
    d
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn memcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    let (a, b) = (a as *const u8, b as *const u8);
    let mut i = 0;
    while i < n {
        let (x, y) = (core::ptr::read_volatile(a.add(i)), core::ptr::read_volatile(b.add(i)));
        if x != y {
            return x as i32 - y as i32;
        }
        i += 1;
    }
    0
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn bcmp(a: *const c_void, b: *const c_void, n: usize) -> i32 {
    memcmp(a, b, n)
}

#[unsafe(no_mangle)]
pub unsafe extern "C" fn strlen(s: *const core::ffi::c_char) -> usize {
    let s = s as *const u8;
    let mut n = 0;
    while core::ptr::read_volatile(s.add(n)) != 0 {
        n += 1;
    }
    n
}

// ------------------------------------------------------------------ DOS

fn xfer() -> *mut u8 {
    systab().xfer as *mut u8
}
fn xfer_seg() -> u16 {
    systab().xfer_seg as u16
}

/// Copy `s` as an ASCIIZ string to the transfer buffer at `off`.
fn put_asciiz(off: usize, s: &str) {
    let b = s.as_bytes();
    let n = b.len().min(127);
    unsafe {
        memcpy(xfer().add(off) as *mut c_void, b.as_ptr() as *const c_void, n);
        *xfer().add(off + n) = 0;
    }
}

pub fn read_file(path: &str) -> Option<Vec<u8>> {
    put_asciiz(0, path);
    let mut r = Regs::ax(0x3D00);
    r.ds = xfer_seg();
    int(0x21, &mut r);
    if r.carry() {
        return None;
    }
    let h = r.eax & 0xFFFF;
    let mut out = Vec::new();
    loop {
        let mut q = Regs::ax(0x3F00);
        q.ebx = h;
        q.ecx = 0x8000;
        q.ds = xfer_seg();
        int(0x21, &mut q);
        let n = (q.eax & 0xFFFF) as usize;
        if q.carry() || n == 0 {
            break;
        }
        out.extend_from_slice(unsafe { core::slice::from_raw_parts(xfer(), n) });
    }
    close(h);
    Some(out)
}

fn close(h: u32) {
    let mut c = Regs::ax(0x3E00);
    c.ebx = h;
    int(0x21, &mut c);
}

pub fn write_file(path: &str, data: &[u8]) -> bool {
    put_asciiz(0, path);
    let mut r = Regs::ax(0x3C00);
    r.ds = xfer_seg();
    int(0x21, &mut r);
    if r.carry() {
        return false;
    }
    let h = r.eax & 0xFFFF;
    let mut ok = true;
    for chunk in data.chunks(0x8000) {
        unsafe { memcpy(xfer() as *mut c_void, chunk.as_ptr() as *const c_void, chunk.len()) };
        let mut q = Regs::ax(0x4000);
        q.ebx = h;
        q.ecx = chunk.len() as u32;
        q.ds = xfer_seg();
        int(0x21, &mut q);
        if q.carry() || (q.eax & 0xFFFF) as usize != chunk.len() {
            ok = false;
            break;
        }
    }
    close(h);
    ok
}

/// Names of files matching `pattern` (e.g. "*.RS") in the current directory.
pub fn list_files(pattern: &str) -> Vec<String> {
    const DTA: usize = 512;
    let mut r = Regs::ax(0x1A00);
    r.ds = xfer_seg();
    r.edx = DTA as u32;
    int(0x21, &mut r);
    put_asciiz(0, pattern);
    let mut out = Vec::new();
    let mut r = Regs::ax(0x4E00);
    r.ecx = 0;
    r.ds = xfer_seg();
    loop {
        int(0x21, &mut r);
        if r.carry() {
            break;
        }
        let name = unsafe { core::slice::from_raw_parts(xfer().add(DTA + 30), 13) };
        let n = name.iter().position(|&c| c == 0).unwrap_or(13);
        out.push(String::from_utf8_lossy(&name[..n]).into_owned());
        r = Regs::ax(0x4F00);
        r.ds = xfer_seg();
        if out.len() > 500 {
            break;
        }
    }
    out.sort();
    out
}

/// Current drive and directory, like `C:\GAMES`.
pub fn current_dir() -> String {
    let mut r = Regs::ax(0x1900);
    int(0x21, &mut r);
    let drive = (b'A' + (r.eax & 0xFF) as u8) as char;
    let mut q = Regs::ax(0x4700);
    q.edx = 0;
    q.ds = xfer_seg();
    q.esi = 0;
    int(0x21, &mut q);
    let p = unsafe { core::slice::from_raw_parts(xfer(), 64) };
    let n = p.iter().position(|&c| c == 0).unwrap_or(64);
    let mut s = String::new();
    s.push(drive);
    s.push_str(":\\");
    s.push_str(&String::from_utf8_lossy(&p[..n]));
    s
}

/// Run a program and wait for it; returns its exit code, or None if it
/// could not be started.
pub fn exec(path: &str, args: &str) -> Option<u8> {
    put_asciiz(0, path);
    let tail = args.as_bytes();
    let n = tail.len().min(125);
    let seg = xfer_seg() as u32;
    unsafe {
        let t = xfer().add(128);
        *t = (n + 1) as u8;
        *t.add(1) = b' ';
        memcpy(t.add(2) as *mut c_void, tail.as_ptr() as *const c_void, n);
        *t.add(2 + n) = 13;
        // Parameter block at 256: env, tail far ptr, two FCB far ptrs.
        let pb = xfer().add(256) as *mut u16;
        let words = [0u16, 128, seg as u16, 272, seg as u16, 272, seg as u16];
        for (i, w) in words.iter().enumerate() {
            *pb.add(i) = *w;
        }
        memset(xfer().add(272) as *mut c_void, 0, 40);
    }
    let mut r = Regs::ax(0x4B00);
    r.ds = seg as u16;
    r.es = seg as u16;
    r.ebx = 256;
    r.edx = 0;
    int(0x21, &mut r);
    if r.carry() {
        return None;
    }
    let mut q = Regs::ax(0x4D00);
    int(0x21, &mut q);
    Some(q.eax as u8)
}

/// An environment variable from our PSP's environment block.
pub fn getenv(name: &str) -> Option<String> {
    let psp = systab().psp;
    let env_seg = unsafe { *((psp * 16 + 0x2C) as *const u16) } as u32;
    let mut p = (env_seg * 16) as *const u8;
    loop {
        let mut entry = Vec::new();
        unsafe {
            while *p != 0 {
                entry.push(*p);
                p = p.add(1);
            }
            p = p.add(1);
        }
        if entry.is_empty() {
            return None;
        }
        if entry.len() > name.len() && entry[..name.len()].eq_ignore_ascii_case(name.as_bytes()) && entry[name.len()] == b'=' {
            return Some(String::from_utf8_lossy(&entry[name.len() + 1..]).into_owned());
        }
    }
}

/// The command line after the program name.
pub fn command_tail() -> String {
    let p = systab().cmd_tail as *const u8;
    let n = unsafe { *p } as usize;
    let bytes = unsafe { core::slice::from_raw_parts(p.add(1), n.min(126)) };
    String::from(String::from_utf8_lossy(bytes).trim())
}

pub fn exit(code: u8) -> ! {
    let mut r = Regs::ax(0x4C00 | code as u32);
    int(0x21, &mut r);
    loop {}
}

// ------------------------------------------------------------------ keyboard

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Key {
    pub scan: u8,
    pub ascii: u8,
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
}

pub fn shift_flags() -> u8 {
    unsafe { core::ptr::read_volatile(0x417 as *const u8) }
}

pub fn read_key() -> Key {
    let mut r = Regs::ax(0x1000);
    int(0x16, &mut r);
    let f = shift_flags();
    let scan = (r.eax >> 8) as u8;
    let mut ascii = r.eax as u8;
    if ascii == 0xE0 && scan != 0 {
        ascii = 0;
    }
    Key { scan, ascii, shift: f & 3 != 0, ctrl: f & 4 != 0, alt: f & 8 != 0 }
}

pub fn key_waiting() -> bool {
    let mut r = Regs::ax(0x1100);
    int(0x16, &mut r);
    r.flags & 0x40 == 0
}

// ------------------------------------------------------------------ video

pub fn set_text_mode() {
    let mut r = Regs::ax(0x0003);
    int(0x10, &mut r);
}

pub fn set_cursor(x: u32, y: u32) {
    let mut r = Regs::ax(0x0200);
    r.edx = y << 8 | x;
    int(0x10, &mut r);
}

/// Underline cursor (insert), block cursor (overwrite) or hidden.
pub fn cursor_shape(shape: u8) {
    let mut r = Regs::ax(0x0100);
    r.ecx = match shape {
        0 => 0x0607,
        1 => 0x0007,
        _ => 0x2000,
    };
    int(0x10, &mut r);
}

/// Print with DOS (for the user screen after running a program).
pub fn dos_print(s: &str) {
    for chunk in s.as_bytes().chunks(0x4000) {
        unsafe { memcpy(xfer() as *mut c_void, chunk.as_ptr() as *const c_void, chunk.len()) };
        let mut r = Regs::ax(0x4000);
        r.ebx = 1;
        r.ecx = chunk.len() as u32;
        r.ds = xfer_seg();
        int(0x21, &mut r);
    }
}

/// Referenced by the prebuilt `alloc`; never called with panic=abort.
#[unsafe(no_mangle)]
pub extern "C" fn _Unwind_Resume() -> ! {
    loop {}
}
