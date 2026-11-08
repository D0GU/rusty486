//! rppc: the R++ compiler on the host. usage: rppc <input.rs> [output.exe]

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: rppc <input.rs> [output.exe]");
        std::process::exit(2);
    }
    let src = std::fs::read_to_string(&args[1]).unwrap_or_else(|e| panic!("{}: {e}", args[1]));
    match rusty486_lang::compile(&src) {
        Ok(exe) => {
            let out = args.get(2).cloned().unwrap_or_else(|| {
                let p = std::path::Path::new(&args[1]);
                p.with_extension("EXE").to_string_lossy().into_owned()
            });
            std::fs::write(&out, &exe).unwrap();
            println!("{out}: {} bytes", exe.len());
        }
        Err(e) => {
            eprintln!("{}:{e}", args[1]);
            std::process::exit(1);
        }
    }
}
