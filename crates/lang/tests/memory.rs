//! Memory use of the compiler on a large program (WRITE.RS).

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering::Relaxed};
struct Count;
static CUR: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);
fn class(l: &Layout) -> usize { l.size().max(l.align()).max(16).next_power_of_two() }
unsafe impl GlobalAlloc for Count {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let c = CUR.fetch_add(class(&l), Relaxed) + class(&l);
        PEAK.fetch_max(c, Relaxed);
        System.alloc(l)
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) { CUR.fetch_sub(class(&l), Relaxed); System.dealloc(p, l) }
}
#[global_allocator]
static A: Count = Count;
/// The compiler runs inside the IDE with a few MB of heap and a
/// power-of-two allocator: keep the peak for a large program in check.
#[test]
fn compiling_a_large_program_stays_small() {
    let src = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../../dos/rustland/examples/WRITE.RS")).unwrap();
    let base = CUR.load(Relaxed);
    PEAK.store(base, Relaxed);
    let toks = rusty486_lang::lexer::lex(&src).unwrap();
    println!("tokens: {} -> {} KB", toks.len(), (CUR.load(Relaxed) - base) / 1024);
    let items = rusty486_lang::parser::Parser::new(&toks).program().unwrap();
    println!("tokens+ast: {} KB", (CUR.load(Relaxed) - base) / 1024);
    drop(items); drop(toks);
    PEAK.store(CUR.load(Relaxed), Relaxed);
    let exe = rusty486_lang::compile(&src).unwrap();
    let peak = (PEAK.load(Relaxed) - base) / 1024;
    println!("compile peak (power-of-two classes): {peak} KB, exe {}", exe.len());
    // Measured with 64-bit pointers; the IDE (32-bit) needs less.
    assert!(peak < 4600, "{peak} KB");
}
