//! Command-line front end: `cargo run -p rusty486-asm --example r486asm -- in.asm out.bin`

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [input, output] = args.as_slice() else {
        eprintln!("usage: r486asm <input.asm> <output.bin>");
        std::process::exit(2);
    };
    let src = std::fs::read_to_string(input).unwrap_or_else(|e| {
        eprintln!("{input}: {e}");
        std::process::exit(1);
    });
    match rusty486_asm::assemble(&src) {
        Ok(out) => {
            std::fs::write(output, &out.bytes).expect("write output");
            if std::env::var_os("R486ASM_MAP").is_some() {
                for l in &out.lines {
                    println!("{} {:08X} {}", l.line, l.addr - out.origin, l.len);
                }
            }
            println!("{} bytes, origin {:#x}", out.bytes.len(), out.origin);
        }
        Err(errors) => {
            for e in errors {
                eprintln!("{input}:{}: error: {}", e.line, e.message);
            }
            std::process::exit(1);
        }
    }
}
