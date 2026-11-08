//! The help window: an R++ language and keyboard reference.

use crate::rt;
use crate::scr::{Screen, H, W};

const HELP: &str = "\
 RUSTLAND R++ HELP                                  Esc closes, arrows scroll

 ~KEYS~
   F1  Help              F2  Save              F3  Open
   F9  Make              Alt+F9  Compile       Ctrl+F9  Run
   Alt+F5  User screen   F6  Next error        F10  Menu
   Alt+X  Exit           Alt+BkSp  Undo        Ins  Insert/Overwrite
   Shift+arrows  Select  Ctrl+Ins  Copy        Shift+Ins  Paste
   Shift+Del  Cut        Ctrl+Y  Delete line   Ctrl+F / Ctrl+L  Find
   Ctrl+G  Go to line    Ctrl+Left/Right  Word Ctrl+Home/End  Top/bottom

 ~THE R++ LANGUAGE~
 R++ is a subset of Rust. Programs compile to 32-bit protected-mode
 .EXE files that run under TinyDOS. A program needs `fn main()`.

   fn add(a: i32, b: i32) -> i32 { a + b }
   const N: usize = 10;
   static mut SCORE: u32 = 0;
   struct Point { x: i32, y: i32 }
   impl Point { fn len2(&self) -> i32 { self.x * self.x + self.y * self.y } }
   enum Dir { Up, Down, Left = 10, Right }

 Types: i8 u8 i16 u16 i32 u32 isize usize bool char &str
        [T; N] arrays, &[T] slices, &T / &mut T, *const T / *mut T,
        structs and C-like enums.
 Statements: let / let mut, if / else, while, loop, break, continue,
        return, for i in 0..n, for i in (0..n).rev(), for x in &arr,
        for c in s.chars(), match with literals, ranges (1..=5), |, _
        and guards (if ...).
 Expressions: + - * / % & | ^ << >> == != < <= > >= && || ! as,
        a[i], &a[1..3], s.field, p.method(), Type::function(), *ptr.

 ~MACROS~
   print!  println!  panic!  assert!  assert_eq!  assert_ne!
   Format: {} {:?} {:x} {:X} {:b} {:5} {:<5} {:>5} {:05} {name}

 ~BUILT-IN FUNCTIONS~
   exit(code)                     args() -> &str (command line)
   read_line(&mut buf) -> usize   parse_int(s) -> i32
   read_key() -> u32              key_pressed() -> bool
   clear_screen()  gotoxy(x, y)   put_char_at(x, y, ch, attr)
   put_str_at(x, y, s, attr)      set_video_mode(0x13 or 3)
   put_pixel(x, y, color)  get_pixel(x, y)  fill_rect(x, y, w, h, c)
   sleep_ms(ms)  ticks() -> u32   beep(freq, ms)  sound_on(f)  sound_off()
   random() -> u32  random_range(lo, hi) -> i32  seed_random(s)
   file_read(path, &mut buf) -> i32   file_write(path, &data) -> bool
   alloc(bytes) -> *mut u8        str_from_bytes(&[u8]) -> &str
   outb(port, v)  inb(port) -> u8 dos_int(n, &mut Regs)

 Methods: len() is_empty() as_bytes() as_ptr() trim() starts_with()
   ends_with() contains() eq_ignore_ascii_case() on &str;
   is_ascii_digit() to_ascii_uppercase() ... on u8 and char;
   abs() min() max() wrapping_add() on integers; p.add(n) on pointers.

 ~NOT SUPPORTED~
 Generics, traits, closures, tuples, Option/Result, String/Vec/Box,
 floating point, 64-bit integers, modules and the borrow checker.
 Arithmetic wraps on overflow; array indexing is bounds-checked.
";

/// Show the help window over the current screen until Esc.
pub fn show(s: &mut Screen) {
    let lines: alloc::vec::Vec<&str> = HELP.lines().collect();
    let (x, y, w, h) = (1, 1, W - 2, H - 2);
    let rows = h - 2;
    let mut top = 0usize;
    loop {
        s.frame(x, y, w, h, 0x3F, true, Some(0x30));
        s.text(x + (w - 6) / 2, y, " Help ", 0x3F);
        for r in 0..rows {
            if let Some(l) = lines.get(top + r) {
                let t: alloc::string::String = l.chars().take(w - 3).collect();
                s.hot_text(x + 1, y + 1 + r, &t, 0x30, 0x3E);
            }
        }
        s.shadow(x, y, w, h);
        s.flush();
        rt::cursor_shape(2);
        let k = rt::read_key();
        let max = lines.len().saturating_sub(rows);
        match k.scan {
            0x01 => return,
            0x48 => top = top.saturating_sub(1),
            0x50 => top = (top + 1).min(max),
            0x49 => top = top.saturating_sub(rows),
            0x51 => top = (top + rows).min(max),
            0x47 => top = 0,
            0x4F => top = max,
            _ => {}
        }
    }
}
