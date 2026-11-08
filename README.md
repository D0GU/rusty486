# rusty486

A fantasy 486 PC written in Rust: a real i486 CPU core paired with a mix of
genuine PC chips and simple custom hardware, plus (eventually) a PICO-8-style
built-in editor and an MCP server so Claude Code can drive the machine.

## Status

**Milestone 1, done:** real-mode CPU, BIOS, text mode, boots a floppy.

- CPU: i486 real mode, including 32-bit operand/address prefixes and the
  386/486 two-byte opcodes. Validated against the
  [SingleStepTests 80386](https://github.com/SingleStepTests/80386) real-mode
  suite: all 1,758,700 tests pass, except 32 documented 386EX-specific cases
  (`crates/core/tests/singlestep_known.txt`).
- BIOS (NASM): INT 10h video, INT 13h disk (CHS + LBA extensions), INT 19h
  boot, plus the small services DOS expects.
- Frontend (SDL2): 80x25 colour text mode, throttled to 33 MHz.

**Milestone 2, done:** MCP server, so Claude Code can see and drive the running machine.

**Milestone 3, done:** DOS-style code editor with a built-in NASM-syntax assembler.

- `rusty486-asm`: assembles the full 486 integer instruction set, `%define`,
  `%macro`, `%rep`, `times`, local labels and `$`/`$$` expressions. Its output
  is byte-identical to NASM for the BIOS and an ~450-form coverage file.
- Editor: press **Esc** to switch between the machine and the editor. Grey
  menu bar, blue editing window, syntax highlighting, mouse and keyboard
  selection, clipboard, undo, and inline error highlighting. F2 saves, F9
  builds, **Ctrl+F9** (or Ctrl+R) builds and runs. It reloads files changed on
  disk by other programs.

**Milestone 4, done:** 8259 PIC, 8254 PIT, RTC/CMOS and the custom
keyboard/mouse device; BIOS keyboard (INT 09h/16h) and timer (INT 08h/1Ah)
services.

**Milestone 5, done:** TinyDOS, a small MS-DOS 5 compatible OS written in
assembly (`dos/`): FAT12/FAT16 with subdirectories, an INT 21h subset (console,
files, directories, find first/next, memory, EXEC of .COM and MZ .EXE,
TSRs), an INT 33h mouse driver and COMMAND.COM (DIR, TYPE, COPY, DEL, REN,
MD, RD, CD, MEM, DATE, TIME, ECHO, PAUSE, batch files, AUTOEXEC.BAT).
Drive C: is a persistent 32 MB TinyDOS disk in `%USERPROFILE%
usty486\hdd.img`.

**Milestone 6, done:** protected mode (descriptors, privilege levels, call
gates, interrupt/trap gates with TSS stack switches, LDT/TR, I/O permission
bitmap), virtual-8086 mode and paging (TLB, accessed/dirty bits, page
faults). Passes the complete [test386.asm](https://github.com/barotto/test386.asm)
ROM (fetch it with `scripts/fetch-test386.ps1`), including its arithmetic
reference output. Not emulated: hardware task switching.

**Milestone 7, done:** graphics. Mode 13h (320x200x256) and VESA VBE 2.0
modes up to 640x480 at 8/15/16/32 bpp (banked window at A0000h and a linear
framebuffer at E0000000h), a 256-colour DAC, retrace status at 3DAh, BIOS
pixel/palette/font services and text drawing in mode 13h.

## Roadmap

1. ~~Real-mode boot to text~~
2. ~~MCP server (local HTTP + token) so Claude Code can drive the machine~~
3. ~~Code editor with a built-in NASM-syntax assembler~~
4. ~~Interrupts, timer, RTC, keyboard and mouse~~
5. ~~TinyDOS~~
6. ~~Protected mode and paging~~
7. ~~VBE graphics~~
8. OPL2 + Sound Blaster + PC speaker
9. Sprite, map, sound and disk editors
10. A Borland-style IDE and compiler for a Rust subset, running inside TinyDOS

## Building

Requirements: Rust (MSVC toolchain on Windows), CMake and a C compiler
(Visual Studio Build Tools). The BIOS and TinyDOS are assembled at build time
by rusty486's own assembler; NASM is only used (if present) by tests that
compare against it.

```sh
cargo run --release -p rusty486-sdl                     # boots the built-in demo floppy
cargo run --release -p rusty486-sdl -- --fda disk.img   # boot your own image
```

Options: `--project DIR`, `--fda/--fdb/--hda/--hdb IMG`, `--mhz N`,
`--mcp-port N`, `--no-mcp`. Keys: F12 opens the editor (Esc returns), Ctrl+F9
builds and runs, F11 resets, Pause pauses; everything else goes to the guest. Disk images the guest writes to are saved when
you close the window.

## Projects

A project is a folder (default `%USERPROFILE%\rusty486\default`, created on
first run from a template):

```text
project.toml      name = "...", entry = "src/main.asm"
src/main.asm      your program: org 0x100, loaded at 1000:0100 like a DOS .COM
build/main.bin    the assembled program
build/disk.img    a bootable 1.44 MB floppy: bios/loader.asm + the program
```

The assembler is also available on the command line:
`cargo run -p rusty486-asm --example r486asm -- in.asm out.bin`.

## MCP server (Claude Code integration)

While the window is open, rusty486 serves MCP over streamable HTTP at
`http://127.0.0.1:8486/mcp`. It only listens on localhost and requires a bearer
token, generated on first run and stored in `%APPDATA%\rusty486\mcp-token`. On
startup it prints the exact command to register it:

```sh
claude mcp add --scope user --transport http rusty486 http://127.0.0.1:8486/mcp \
  --header "Authorization: Bearer <token>"
```

| Tool | What it does |
|------|--------------|
| `status` | Running/paused (and why), CS:IP, cycles, disks, breakpoints |
| `screenshot`, `screen_text` | The screen as a PNG or as plain text with the cursor position |
| `get_registers`, `set_register` | CPU state |
| `read_memory`, `write_memory` | Hex dump / poke; addresses as `SEG:OFF` (e.g. `cs:0100`) or linear hex |
| `pause`, `resume`, `step`, `run`, `reset` | Execution control; `run` waits for emulated time or a breakpoint |
| `breakpoint_add`, `breakpoint_remove` | Execution breakpoints |
| `debug_log` | Text written to debug port E9h (the BIOS mirrors its output there) |
| `disk_insert`, `disk_eject`, `disk_save`, `disk_list` | Mount raw `.img` files (can create blank ones) |
| `load_file` | Copy a flat binary into memory and optionally jump to it |
| `project_info`, `read_source`, `write_source` | The open project and its source files |
| `assemble`, `build`, `build_and_run` | Built-in assembler; `build_and_run` is the editor's Ctrl+F9 |
| `set_view` | Show the machine or the editor (`screenshot` takes `view` too) |
| `type_text`, `press_key` | Keyboard input for the guest |
| `dos_command` | Type a TinyDOS command and return the screen |
| `disk_files`, `disk_read_file`, `disk_write_file`, `disk_delete` | Files on mounted FAT images |

Tool calls run on the UI thread between frames, so Claude and the window
always see the same machine.

## Tests

```sh
cargo test --release                    # unit + boot tests
scripts/fetch-tests.ps1                 # download the 386 CPU suite (~576 MB)
cargo test -p rusty486-core --release --test singlestep -- --nocapture
```

`RUSTY486_TEST_FILTER=0F` limits the CPU suite to matching opcode files, and
`RUSTY486_TEST_VERBOSE=N` prints details for up to N failures per file.

## Layout

| Path | What |
|------|------|
| `crates/core` | Emulator core: CPU, bus, devices. No host I/O (WASM-ready). |
| `crates/asm` | NASM-syntax assembler. No host I/O (WASM-ready). |
| `crates/firmware` | BIOS + TinyDOS, assembled at build time; TinyDOS disk builder. |
| `dos/` | TinyDOS sources: boot sector, kernel, COMMAND.COM. |
| `crates/sdl` | Desktop frontend (`rusty486` binary). |
| `bios/` | BIOS and demo boot sector (NASM). |
| `scripts/` | Helper scripts. |

## Hardware reference

| Device | Ports / memory |
|--------|----------------|
| Disk controller (custom, bus-mastering) | 300h-317h, see `crates/core/src/devices/disk.rs` |
| Text video + CRTC cursor | B8000h, 3D4h/3D5h |
| Fast A20 gate | 92h |
| Debug console (Bochs-style) | E9h |

## License

Dual-licensed under [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE).
