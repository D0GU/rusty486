//! rusty486 projects: a folder with `project.toml`, sources under `src/`,
//! data files under `data/`, and build output under `build/`.
//!
//! ```text
//! my-game/
//!   project.toml      name = "my-game", entry = "src/main.asm"
//!   src/main.asm      the program (org 0x100, a DOS .COM)
//!   data/...          files copied onto the program's disk
//!   build/NAME.COM    assembled program
//!   build/disk.img    bootable TinyDOS floppy that runs it
//! ```

use std::path::{Path, PathBuf};

use rusty486_asm::{AsmError, Output};
use rusty486_core::fat::Fs;

/// .COM programs load at PSP:0100 and must fit in one 64 KB segment.
pub const MAX_PROGRAM: usize = 0xFF00;
const HDD_SIZE: usize = 32 * 1024 * 1024;

const TEMPLATE: &str = r#"; rusty486 program
;
; Ctrl+F9 assembles this file, puts it on a TinyDOS boot floppy with the
; files from the project's data\ folder, and runs it. It's a DOS .COM
; program: loaded at offset 100h with CS = DS = ES = SS.

        org 0x100
        bits 16

start:
        mov ax, 0x0003          ; 80x25 colour text mode (clears the screen)
        int 0x10

        mov dx, hello           ; print a '$'-terminated string
        mov ah, 0x09
        int 0x21

        mov ah, 0x00            ; wait for a key
        int 0x16

        mov ax, 0x4C00          ; exit to DOS with code 0
        int 0x21

hello   db 'Hello from my rusty486 program!', 13, 10
        db 'Press any key to return to TinyDOS.', 13, 10, '$'
"#;

pub struct Project {
    pub dir: PathBuf,
    pub name: String,
    /// Entry source file, relative to `dir`.
    pub entry: String,
}

/// Result of building the project.
pub struct Build {
    pub output: Result<Output, Vec<AsmError>>,
    /// The bootable floppy image, when the build succeeded.
    pub disk: Option<Vec<u8>>,
}

impl Project {
    /// Open the project in `dir`, creating a new one from the template if
    /// there's no `project.toml` yet.
    pub fn open_or_create(dir: &Path) -> std::io::Result<Project> {
        let manifest = dir.join("project.toml");
        if !manifest.exists() {
            std::fs::create_dir_all(dir.join("src"))?;
            std::fs::create_dir_all(dir.join("data"))?;
            let name = dir.file_name().map_or("untitled".into(), |n| n.to_string_lossy().into_owned());
            std::fs::write(&manifest, format!("name = \"{name}\"\nentry = \"src/main.asm\"\n"))?;
            let main = dir.join("src/main.asm");
            if !main.exists() {
                std::fs::write(main, TEMPLATE)?;
            }
        }
        let text = std::fs::read_to_string(&manifest)?;
        let get = |key: &str| {
            text.lines().find_map(|l| {
                let (k, v) = l.split_once('=')?;
                (k.trim() == key).then(|| v.trim().trim_matches('"').to_string())
            })
        };
        Ok(Project {
            dir: dir.to_path_buf(),
            name: get("name").unwrap_or_else(|| "untitled".into()),
            entry: get("entry").unwrap_or_else(|| "src/main.asm".into()),
        })
    }

    pub fn entry_path(&self) -> PathBuf {
        self.dir.join(&self.entry)
    }

    pub fn disk_path(&self) -> PathBuf {
        self.dir.join("build/disk.img")
    }

    /// The program's 8.3 file name on the disk, e.g. "MYGAME.COM".
    pub fn program_name(&self) -> String {
        let base: String = self
            .name
            .chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(8)
            .collect::<String>()
            .to_ascii_uppercase();
        format!("{}.COM", if base.is_empty() { "PROGRAM".into() } else { base })
    }

    /// Resolve a project-relative path, refusing to escape the project folder.
    pub fn resolve(&self, rel: &str) -> Result<PathBuf, String> {
        let rel = rel.replace('\\', "/");
        let p = Path::new(&rel);
        if p.is_absolute() || p.components().any(|c| matches!(c, std::path::Component::ParentDir)) {
            return Err(format!("'{rel}' must be a path inside the project"));
        }
        Ok(self.dir.join(p))
    }

    /// Source files under `src/`, relative to the project folder.
    pub fn sources(&self) -> Vec<String> {
        list_files(&self.dir, "src")
    }

    /// Resolver for `%include`: paths relative to the entry file's folder.
    fn include(&self, name: &str) -> Option<String> {
        let base = self.entry_path().parent().map(Path::to_path_buf).unwrap_or_else(|| self.dir.clone());
        std::fs::read_to_string(base.join(name)).ok()
    }

    /// Assemble `source` and, on success, write the program and a bootable
    /// TinyDOS floppy that runs it.
    pub fn build(&self, source: &str) -> Build {
        let resolver = |n: &str| self.include(n);
        let output = rusty486_asm::assemble_with(source, &resolver).and_then(|out| {
            let err = |message: String| Err(vec![AsmError { line: 1, file: None, message }]);
            if out.bytes.len() > MAX_PROGRAM {
                return err(format!("program is {} bytes; the limit is {MAX_PROGRAM}", out.bytes.len()));
            }
            if out.origin != 0x100 {
                return err(format!("programs must use 'org 0x100' (found org {:#x})", out.origin));
            }
            Ok(out)
        });
        let disk = match &output {
            Ok(out) => match self.make_disk(&out.bytes) {
                Ok(d) => Some(d),
                Err(message) => {
                    return Build { output: Err(vec![AsmError { line: 1, file: None, message }]), disk: None }
                }
            },
            Err(_) => None,
        };
        if let (Ok(out), Some(disk)) = (&output, &disk) {
            let build = self.dir.join("build");
            let _ = std::fs::create_dir_all(&build);
            let _ = std::fs::write(build.join(self.program_name()), &out.bytes);
            let _ = std::fs::write(build.join("disk.img"), disk);
        }
        Build { output, disk }
    }

    fn make_disk(&self, program: &[u8]) -> Result<Vec<u8>, String> {
        let prog = self.program_name();
        let autoexec = format!("@ECHO OFF\r\n{}\r\n", prog.trim_end_matches(".COM"));
        let mut data_files: Vec<(String, Vec<u8>)> = Vec::new();
        for rel in list_files(&self.dir, "data") {
            let bytes = std::fs::read(self.dir.join(&rel)).map_err(|e| format!("{rel}: {e}"))?;
            data_files.push((rel.trim_start_matches("data/").to_string(), bytes));
        }
        let mut files: Vec<(&str, &[u8])> = vec![(&prog, program), ("AUTOEXEC.BAT", autoexec.as_bytes())];
        for (p, d) in &data_files {
            files.push((p, d));
        }
        rusty486_firmware::tinydos_floppy(&self.name, &files).map_err(|e| format!("building the disk: {e}"))
    }
}

/// Files under `dir/sub`, recursively, as "sub/a/b.txt" paths.
fn list_files(dir: &Path, sub: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![sub.to_string()];
    while let Some(rel) = stack.pop() {
        if let Ok(rd) = std::fs::read_dir(dir.join(&rel)) {
            for e in rd.flatten() {
                let child = format!("{rel}/{}", e.file_name().to_string_lossy());
                if e.path().is_dir() {
                    stack.push(child);
                } else {
                    out.push(child);
                }
            }
        }
    }
    out.sort();
    out
}

fn rusty486_home() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusty486")
}

/// Default project folder: %USERPROFILE%\rusty486\default.
pub fn default_dir() -> PathBuf {
    rusty486_home().join("default")
}

/// The persistent hard disk image: %USERPROFILE%\rusty486\hdd.img.
pub fn hdd_path() -> PathBuf {
    rusty486_home().join("hdd.img")
}

/// Load the hard disk image, creating a 32 MB TinyDOS disk if it doesn't
/// exist. TinyDOS disks get the current kernel and COMMAND.COM.
pub fn open_or_create_hdd(path: &Path) -> std::io::Result<Vec<u8>> {
    if let Ok(mut data) = std::fs::read(path) {
        if rusty486_firmware::is_tinydos(&data) {
            refresh_system(&mut data);
        }
        return Ok(data);
    }
    let mut data = vec![0u8; HDD_SIZE];
    rusty486_firmware::format_tinydos(&mut data, "RUSTY486")
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    {
        let mut fs = Fs::open(&mut data).map_err(|e| std::io::Error::other(e.to_string()))?;
        let readme = b"Welcome to TinyDOS on rusty486!\r\n\r\nThis is drive C:, saved in hdd.img in your rusty486 folder.\r\nType HELP for a list of commands.\r\n";
        let _ = fs.write("README.TXT", readme);
    }
    install_rustland(&mut data);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, &data)?;
    Ok(data)
}

/// Update the kernel (in the reserved sectors) and COMMAND.COM on an
/// existing TinyDOS disk, if the reserved area is big enough.
fn refresh_system(data: &mut [u8]) {
    let reserved = u16::from_le_bytes([data[14], data[15]]) as usize;
    let kernel = rusty486_firmware::DOS_KERNEL;
    if (reserved - 1) * 512 >= kernel.len() {
        data[512..512 + kernel.len()].copy_from_slice(kernel);
        if let Ok(mut fs) = Fs::open(data) {
            let _ = fs.write("COMMAND.COM", rusty486_firmware::COMMAND_COM);
        }
    }
    install_rustland(data);
}

/// The Rustland R++ IDE (built by scripts/build-rustland.ps1).
const RUSTLAND_EXE: &[u8] = include_bytes!("../../../dos/rustland/RUSTLAND.EXE");

/// Example programs, copied to C:\RPP unless already there.
const RPP_EXAMPLES: &[(&str, &[u8])] = &[
    ("RPP/HELLO.RS", include_bytes!("../../../dos/rustland/examples/HELLO.RS")),
    ("RPP/GUESS.RS", include_bytes!("../../../dos/rustland/examples/GUESS.RS")),
    ("RPP/PRIMES.RS", include_bytes!("../../../dos/rustland/examples/PRIMES.RS")),
    ("RPP/SHAPES.RS", include_bytes!("../../../dos/rustland/examples/SHAPES.RS")),
    ("RPP/PLASMA.RS", include_bytes!("../../../dos/rustland/examples/PLASMA.RS")),
    ("RPP/SNAKE.RS", include_bytes!("../../../dos/rustland/examples/SNAKE.RS")),
];

/// Put the current RUSTLAND.EXE in C:\ and the examples in C:\RPP.
fn install_rustland(data: &mut [u8]) {
    let Ok(mut fs) = Fs::open(data) else { return };
    if fs.read("RUSTLAND.EXE").ok().as_deref() != Some(RUSTLAND_EXE) {
        let _ = fs.write("RUSTLAND.EXE", RUSTLAND_EXE);
    }
    for (path, bytes) in RPP_EXAMPLES {
        if !fs.exists(path) {
            // DOS text files use CR LF.
            let text = String::from_utf8_lossy(bytes).replace("\r\n", "\n").replace('\n', "\r\n");
            let _ = fs.write_creating_dirs(path, text.as_bytes());
        }
    }
}
