//! Compiles R++ programs and runs them under TinyDOS in a headless rusty486.

use rusty486_core::devices::disk::DiskImage;
use rusty486_core::machine::{Drive, Machine};
use rusty486_firmware::{tinydos_floppy, BIOS};

struct Pc {
    m: Machine,
}

impl Pc {
    fn boot(files: &[(&str, &[u8])]) -> Pc {
        let disk = tinydos_floppy("TEST", files).expect("build disk");
        let mut m = Machine::new(BIOS.to_vec());
        m.insert_disk(Drive::A, DiskImage::new(disk));
        let mut pc = Pc { m };
        pc.run_ms(1500);
        pc
    }
    fn run_ms(&mut self, ms: u64) {
        let hz = self.m.clock_hz;
        self.m.run_cycles(hz / 1000 * ms);
        assert!(!self.m.cpu.shutdown, "CPU shutdown\n{}", self.m.screen_text());
    }
    fn key(&mut self, c: char) {
        let (code, ascii) = if c == '\r' { (0x1C, 13) } else { (0x02, c as u8) };
        self.m.key_event(code, false, true, ascii, 0, false);
        self.m.key_event(code, false, false, 0, 0, false);
        self.run_ms(3);
    }
    fn type_line(&mut self, s: &str, ms: u64) {
        for c in s.chars() {
            self.key(c);
        }
        self.key('\r');
        self.run_ms(ms);
    }
    fn screen(&self) -> String {
        self.m.screen_text()
    }
}

fn compile(src: &str) -> Vec<u8> {
    match rusty486_lang::compile(src) {
        Ok(exe) => exe,
        Err(e) => panic!("compile error: {e}"),
    }
}

/// Run a program; returns the screen text after the command.
fn run(src: &str) -> String {
    run_with(src, "", &[], 1500)
}

fn run_with(src: &str, args: &str, input: &[&str], ms: u64) -> String {
    let exe = compile(src);
    let mut pc = Pc::boot(&[("PROG.EXE", &exe)]);
    let cmd = if args.is_empty() { String::from("PROG") } else { format!("PROG {args}") };
    pc.type_line(&cmd, 50);
    for line in input {
        pc.type_line(line, 50);
    }
    pc.run_ms(ms);
    let s = pc.screen();
    let start = s.find(&format!("A:\\>{cmd}")).expect("command echo") + cmd.len() + 4;
    s[start..].trim_end().to_string()
}

fn expect(src: &str, want: &str) {
    let out = run(src);
    let out_lines: Vec<&str> = out.lines().map(|l| l.trim_end()).collect();
    let got = out_lines.join("\n");
    assert!(got.trim_start().starts_with(want), "expected output starting with:\n{want}\n--- got:\n{got}");
}

fn compile_err(src: &str) -> String {
    match rusty486_lang::compile(src) {
        Ok(_) => panic!("expected a compile error"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn hello_world() {
    expect(
        r#"
fn main() {
    println!("Hello, world!");
    let x = 6;
    let y: u8 = 7;
    println!("{} * {} = {}", x, y, x * y as i32);
}
"#,
        "Hello, world!\n6 * 7 = 42",
    );
}

#[test]
fn control_flow_and_functions() {
    expect(
        r#"
fn fib(n: u32) -> u32 {
    if n < 2 { n } else { fib(n - 1) + fib(n - 2) }
}

fn classify(n: i32) -> &str {
    match n {
        i32::MIN..=-1 => "negative",
        0 => "zero",
        1 | 2 | 3 => "small",
        _ => "large",
    }
}

fn main() {
    let mut total = 0;
    for i in 0..10 {
        if i % 2 == 0 { continue; }
        total += i;
    }
    println!("odd sum {}", total);
    let mut n = 0u32;
    while n < 5 { n += 1; }
    let mut k = 0;
    loop {
        k += 3;
        if k > 10 { break; }
    }
    println!("{} {} fib {}", n, k, fib(15));
    for v in [-5, 0, 2, 99] {
        print!("{} ", classify(v));
    }
    println!();
    for i in (0..4).rev() { print!("{}", i); }
    for i in (0..10).step_by(3) { print!(" {}", i); }
    println!();
}
"#,
        "odd sum 25\n5 12 fib 610\nnegative zero small large\n3210 0 3 6 9",
    );
}

#[test]
fn structs_methods_enums() {
    expect(
        r#"
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape { Square, Circle, Triangle }

#[derive(Debug)]
struct Point { x: i32, y: i32 }

struct Rect { origin: Point, w: i32, h: i32, kind: Shape }

impl Rect {
    fn new(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { origin: Point { x, y }, w, h, kind: Shape::Square }
    }
    fn area(&self) -> i32 { self.w * self.h }
    fn grow(&mut self, by: i32) { self.w += by; self.h += by; }
    fn corner(&self) -> Point { Point { x: self.origin.x + self.w, y: self.origin.y + self.h } }
}

fn name(s: Shape) -> &'static str {
    match s {
        Shape::Square => "square",
        Shape::Circle => "circle",
        _ => "other",
    }
}

fn main() {
    let mut r = Rect::new(1, 2, 3, 4);
    println!("area {}", r.area());
    r.grow(2);
    let c = r.corner();
    println!("area {} corner {} {}", r.area(), c.x, c.y);
    println!("{:?}", c);
    let p = &mut r;
    p.origin.x = 10;
    println!("{} {} {:?}", r.origin.x, name(Shape::Circle), Shape::Triangle);
    println!("{}", r.kind == Shape::Square);
}
"#,
        "area 12\narea 30 corner 6 8\nPoint { x: 6, y: 8 }\n10 circle Triangle\ntrue",
    );
}

#[test]
fn arrays_slices_strings() {
    expect(
        r#"
const N: usize = 5;
static GREETING: &str = "hi";
static mut COUNTER: u32 = 0;

fn sum(xs: &[i32]) -> i32 {
    let mut s = 0;
    for x in xs { s += *x; }
    s
}

fn bump() { unsafe { COUNTER += 1; } }

fn main() {
    let mut a = [0i32; N];
    for i in 0..N { a[i] = (i * i) as i32; }
    println!("{:?} sum {} len {}", a, sum(&a), a.len());
    println!("{}", sum(&a[1..3]));
    let s = "  Hello R++  ";
    let t = s.trim();
    println!("[{}] {} {}", t, t.len(), t.starts_with("Hel"));
    let mut upper = [0u8; 16];
    let b = t.as_bytes();
    for i in 0..b.len() { upper[i] = b[i].to_ascii_uppercase(); }
    println!("{}", str_from_bytes(&upper[..b.len()]));
    for c in "abc".chars() { print!("{}", c.to_ascii_uppercase()); }
    println!(" {}", GREETING);
    bump(); bump();
    println!("{}", unsafe { COUNTER });
    println!("{:>5}|{:<4}|{:05}|{:x}|{:X}|{:b}", 42, "ab", -7, 255, 255u32, 5);
    let words = ["one", "two", "three"];
    for w in words.iter() { print!("{} ", w); }
    println!("{}", if "abc" == "abc" { "eq" } else { "ne" });
}
"#,
        "[0, 1, 4, 9, 16] sum 30 len 5\n5\n[Hello R++] 9 true\nHELLO R++\nABC hi\n2\n   42|ab  |-0007|ff|FF|101\none two three eq",
    );
}

#[test]
fn pointers_and_heap() {
    expect(
        r#"
struct Node { value: i32, next: *mut Node }

fn main() {
    let mut head: *mut Node = 0 as *mut Node;
    for i in 1..=4 {
        let n = alloc(8) as *mut Node;
        unsafe {
            (*n).value = i * 10;
            (*n).next = head;
        }
        head = n;
    }
    let mut p = head;
    while !p.is_null() {
        unsafe {
            print!("{} ", (*p).value);
            p = (*p).next;
        }
    }
    println!();
    let mut x = 5;
    let r = &mut x;
    *r += 1;
    println!("{}", x);
}
"#,
        "40 30 20 10\n6",
    );
}

#[test]
fn panics_and_exit_codes() {
    let out = run(r#"
fn main() {
    let a = [1, 2, 3];
    let i = 7;
    println!("before");
    println!("{}", a[i]);
}
"#);
    assert!(out.contains("before"), "{out}");
    assert!(out.contains("index out of bounds: the len is 3 but the index is 7"), "{out}");
    let out = run("fn main() { let z = 0; assert_eq!(1 + 1, 3, \"math\"); println!(\"{}\", 1 / z); }");
    assert!(out.contains("assertion `left == right` failed: math"), "{out}");
    assert!(out.contains("left: 2"), "{out}");
}

#[test]
fn reads_input_and_args() {
    let out = run_with(
        r#"
fn main() {
    println!("args=[{}]", args());
    print!("Name? ");
    let mut buf = [0u8; 40];
    let n = read_line(&mut buf);
    let name = str_from_bytes(&buf[..n]);
    println!("Hello, {}! ({} letters)", name, n);
    let mut nb = [0u8; 10];
    let k = read_line(&mut nb);
    println!("double {}", parse_int(str_from_bytes(&nb[..k])) * 2);
}
"#,
        "-x  y",
        &["Ferris", "21"],
        1000,
    );
    assert!(out.contains("args=[-x  y]"), "{out}");
    assert!(out.contains("Hello, Ferris! (6 letters)"), "{out}");
    assert!(out.contains("double 42"), "{out}");
}

#[test]
fn big_statics_and_memory_moves() {
    expect(
        r#"
static mut BIG: [u8; 100000] = [0; 100000];
static mut WORDS: [u32; 4] = [0, 0, 0, 0];
static mut INIT: [u8; 3] = [1, 2, 3];

fn main() {
    unsafe {
        for i in 0..10 { BIG[i] = b'a' + i as u8; }
        // Overlapping move to the right, then to the left.
        move_bytes(BIG.as_mut_ptr().add(3), BIG.as_ptr(), 10);
        println!("{}", str_from_bytes(&BIG[..13]));
        move_bytes(BIG.as_mut_ptr(), BIG.as_ptr().add(3), 10);
        println!("{}", str_from_bytes(&BIG[..10]));
        fill_bytes(BIG.as_mut_ptr(), b'z', 4);
        copy_bytes(BIG.as_mut_ptr().add(99990), BIG.as_ptr(), 4);
        println!("{} {} {} {}", str_from_bytes(&BIG[99990..99994]), BIG[99999], WORDS[3], INIT[2]);
    }
}
"#,
        "abcabcdefghij
abcdefghij
zzzz 0 0 3",
    );
    // The zero-filled array is not stored in the executable.
    let exe = rusty486_lang::compile("static mut A: [u8; 200000] = [0; 200000];
fn main() { unsafe { A[5] = 1; } }").unwrap();
    assert!(exe.len() < 20000, "{}", exe.len());
}

#[test]
fn files() {
    expect(
        r#"
fn main() {
    let ok = file_write("OUT.TXT", b"saved by R++");
    let mut buf = [0u8; 64];
    let n = file_read("OUT.TXT", &mut buf);
    println!("{} {} {}", ok, n, str_from_bytes(&buf[..n as usize]));
    println!("{}", file_read("NOPE.TXT", &mut buf));
}
"#,
        "true 12 saved by R++\n-1",
    );
}

#[test]
fn compile_errors() {
    assert!(compile_err("fn main() { let x = 1; x = 2; }").contains("1:24: cannot assign twice to immutable variable `x`"));
    assert!(compile_err("fn main() { let x: u8 = 300; }").contains("literal out of range for `u8`"));
    assert!(compile_err("fn main() { let x: i32 = \"a\"; }").contains("mismatched types: expected `i32`, found `&str`"));
    assert!(compile_err("fn main() { foo(); }").contains("cannot find function `foo`"));
    assert!(compile_err("fn f() -> i32 { 1 }\nfn main() { let a: u8 = f(); }").contains("2:25: mismatched types"));
    assert!(compile_err("fn main() {\n  println!(\"{}\");\n}").contains("2:"));
    assert!(compile_err("fn main() { let v: Vec<i32> = 0; }").contains("expected"));
    assert!(compile_err("struct P { x: i32 }\nfn main() { let p = P { x: 1 }; p.y; }").contains("no field `y`"));
    assert!(compile_err("fn f() -> i32 { }\nfn main() {}").contains("missing return value"));
    assert!(compile_err("fn main() { let x = 1 }").contains("expected `;`"));
}

fn example(name: &str) -> String {
    let p = format!("{}/../../dos/rustland/examples/{name}", env!("CARGO_MANIFEST_DIR"));
    std::fs::read_to_string(p).unwrap()
}

#[test]
fn examples_compile() {
    for name in ["HELLO.RS", "GUESS.RS", "PRIMES.RS", "SHAPES.RS", "PLASMA.RS", "SNAKE.RS", "WRITE.RS"] {
        if let Err(e) = rusty486_lang::compile(&example(name)) {
            panic!("{name}: {e}");
        }
    }
}

#[test]
fn examples_run() {
    let out = run_with(&example("HELLO.RS"), "Ferris", &[], 500);
    assert!(out.contains("Hello from Rustland R++!") && out.contains("Arguments: Ferris") && out.contains("= 55"), "{out}");
    let out = run(&example("PRIMES.RS"));
    assert!(out.contains("46 primes, 25 below 100") && out.contains("  199"), "{out}");
    let out = run(&example("SHAPES.RS"));
    assert!(out.contains("Circle of size 3 has area 28 (medium)"), "{out}");
    assert!(out.contains("Shape { kind: Circle, size: 13 }"), "{out}");
    assert!(out.contains("Triangle of size 20 has area 200 (huge)"), "{out}");
    let out = run_with(&example("GUESS.RS"), "", &["50", "25", "75", "12", "88", "6", "94", "3", "97", "1", "100"], 500);
    assert!(out.contains("I'm thinking of a number") && out.contains("Your guess:"), "{out}");
}
