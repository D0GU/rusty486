//! Error reporting and a few semantics NASM tests don't cover.

use rusty486_asm::assemble;

fn errors(src: &str) -> Vec<(usize, String)> {
    assemble(src).err().unwrap_or_default().into_iter().map(|e| (e.line, e.message)).collect()
}

#[test]
fn reports_line_numbers() {
    let e = errors("nop\nmvo ax, bx\nnop\n");
    assert_eq!(e.len(), 1);
    assert_eq!(e[0].0, 2);
    assert!(e[0].1.contains("unknown instruction 'mvo'"), "{e:?}");
}

#[test]
fn reports_all_errors() {
    let e = errors("mvo ax, bx\njmp nowhere\n");
    assert_eq!(e.len(), 2, "{e:?}");
    assert_eq!((e[0].0, e[1].0), (1, 2));
}

#[test]
fn undefined_symbol() {
    let e = errors("jmp nowhere\n");
    assert!(e[0].1.contains("undefined symbol 'nowhere'"), "{e:?}");
}

#[test]
fn short_jump_out_of_range() {
    let e = errors("jmp short far_away\ntimes 200 nop\nfar_away:\n");
    assert!(e[0].1.contains("out of range"), "{e:?}");
}

#[test]
fn size_needed() {
    let e = errors("mov [bx], 5\n");
    assert!(e[0].1.contains("size not specified"), "{e:?}");
}

#[test]
fn local_labels_and_symbols() {
    let out = assemble("org 100h\nmain:\n.loop: jmp .loop\nother:\n.loop: jmp main.loop\n").unwrap();
    assert_eq!(out.symbols["main.loop"], 0x100);
    assert_eq!(out.symbols["other.loop"], 0x102);
    assert_eq!(out.bytes, vec![0xEB, 0xFE, 0xEB, 0xFC]);
    assert_eq!(out.lines[1].line, 5);
}

#[test]
fn forward_jumps_shrink_to_short() {
    let out = assemble("jmp done\nnop\ndone: ret\n").unwrap();
    assert_eq!(out.bytes, vec![0xEB, 0x01, 0x90, 0xC3]);
}
