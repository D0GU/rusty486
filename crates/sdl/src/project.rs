//! rusty486 projects: a folder with `project.toml`, sources under `src/`, and
//! build output under `build/`.
//!
//! ```text
//! my-game/
//!   project.toml      name = "my-game", entry = "src/main.asm"
//!   src/main.asm
//!   build/main.bin    assembled program
//!   build/disk.img    bootable 1.44 MB floppy: loader + program
//! ```

use std::path::{Path, PathBuf};

use rusty486_asm::{AsmError, Output};

pub const LOADER: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/loader.bin"));
/// Where the build patches the program's sector count into the loader.
const LOADER_SECTORS_OFFSET: usize = 0x1FC;
const FLOPPY_SIZE: usize = 1_474_560;
/// Programs load at 1000:0100 and must fit in that 64 KB segment.
pub const MAX_PROGRAM: usize = 0xFF00;

const TEMPLATE: &str = r#"; rusty486 program
;
; Ctrl+F9 (or Ctrl+R) assembles this file, writes it to a boot floppy and runs it.
; It is loaded at 1000:0100 like a DOS .COM file: CS = DS = ES = SS.
; Print text with the BIOS (INT 10h) and log to the host with OUT 0E9h, AL.

        org 0x100
        bits 16

start:
        mov ax, 0x0003          ; 80x25 colour text mode (clears the screen)
        int 0x10

        mov si, hello
        call print

        ret                     ; returns to a halt stub

; Print the NUL-terminated string at DS:SI.
print:
        lodsb
        test al, al
        jz .done
        mov ah, 0x0E            ; teletype output
        mov bx, 0x0007
        int 0x10
        jmp print
.done:  ret

hello   db 'Hello from my rusty486 program!', 13, 10, 0
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
        let mut out = Vec::new();
        if let Ok(rd) = std::fs::read_dir(self.dir.join("src")) {
            for e in rd.flatten() {
                if e.path().is_file() {
                    out.push(format!("src/{}", e.file_name().to_string_lossy()));
                }
            }
        }
        out.sort();
        out
    }

    /// Assemble `source` and, on success, write build/main.bin and build/disk.img.
    pub fn build(&self, source: &str) -> Build {
        let output = rusty486_asm::assemble(source).and_then(|out| {
            if out.bytes.len() > MAX_PROGRAM {
                return Err(vec![AsmError {
                    line: 1,
                    message: format!("program is {} bytes; the limit is {MAX_PROGRAM}", out.bytes.len()),
                }]);
            }
            if out.origin != 0x100 {
                return Err(vec![AsmError {
                    line: 1,
                    message: format!("programs must use 'org 0x100' (found org {:#x})", out.origin),
                }]);
            }
            Ok(out)
        });
        let disk = output.as_ref().ok().map(|out| make_disk(&out.bytes));
        if let (Ok(out), Some(disk)) = (&output, &disk) {
            let build = self.dir.join("build");
            let _ = std::fs::create_dir_all(&build);
            let _ = std::fs::write(build.join("main.bin"), &out.bytes);
            let _ = std::fs::write(build.join("disk.img"), disk);
        }
        Build { output, disk }
    }
}

/// A bootable floppy: the loader in sector 0, the program from sector 1.
pub fn make_disk(program: &[u8]) -> Vec<u8> {
    let mut disk = vec![0u8; FLOPPY_SIZE];
    disk[..512].copy_from_slice(LOADER);
    let sectors = program.len().div_ceil(512) as u16;
    disk[LOADER_SECTORS_OFFSET..LOADER_SECTORS_OFFSET + 2].copy_from_slice(&sectors.to_le_bytes());
    disk[512..512 + program.len()].copy_from_slice(program);
    disk
}

/// Default project folder: %USERPROFILE%\rusty486\default.
pub fn default_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusty486")
        .join("default")
}
