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

## Roadmap

1. ~~Real-mode boot to text~~
2. ~~MCP server (local HTTP + token) so Claude Code can drive the machine~~
3. Code editor with a built-in NASM-syntax assembler
4. Protected mode and paging, VBE graphics, mini-DOS, sprite/map/sound editors,
   OPL2 + Sound Blaster, and a drive manager

## Building

Requirements: Rust (MSVC toolchain on Windows), CMake, a C compiler (Visual
Studio Build Tools) and [NASM](https://nasm.us) on `PATH` (or set `NASM`).

```sh
cargo run --release -p rusty486-sdl                     # boots the built-in demo floppy
cargo run --release -p rusty486-sdl -- --fda disk.img   # boot your own image
```

Options: `--fda/--fdb/--hda/--hdb IMG`, `--mhz N`, `--mcp-port N`, `--no-mcp`.
Keys: F11 resets, Pause pauses. Disk images the guest writes to are saved when
you close the window.

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
