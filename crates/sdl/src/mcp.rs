//! MCP server: lets Claude Code (or any MCP client) see and drive the running
//! machine over streamable HTTP on localhost, protected by a bearer token.
//!
//! The server runs on its own thread with a tokio runtime. Every tool turns
//! into a `Job` that the UI thread executes between frames, so the window and
//! the MCP client always see the same machine.

use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::{Request, State};
use axum::http::{header, StatusCode};
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use base64::Engine;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock, ServerCapabilities, ServerConfig};
use rmcp::transport::streamable_http_server::session::local::LocalSessionManager;
use rmcp::transport::streamable_http_server::{StreamableHttpServerConfig, StreamableHttpService};
use rmcp::{schemars, tool, tool_handler, tool_router, ErrorData as McpError, ServerHandler};
use rusty486_core::cpu::{self, flags, Cpu};
use rusty486_core::devices::disk::DiskImage;
use rusty486_core::fat::Fs;
use rusty486_core::machine::Drive;
use rusty486_core::Bus;
use serde::Deserialize;
use serde_json::{json, Value};
use tokio::sync::oneshot;

use crate::editor::Level;
use crate::keymap;
use crate::emu::{BuildSummary, Emu, Job, View, Waiter};

pub const DEFAULT_PORT: u16 = 8486;

/// Longest a single `run` call may wait, in real time.
const MAX_RUN_WAIT: Duration = Duration::from_secs(120);

// ------------------------------------------------------------------ startup

/// Load the bearer token from the config directory, creating one if needed.
pub fn load_or_create_token() -> std::io::Result<(String, PathBuf)> {
    let dir = config_dir();
    let path = dir.join("mcp-token");
    if let Ok(t) = std::fs::read_to_string(&path) {
        let t = t.trim().to_string();
        if t.len() >= 32 {
            return Ok((t, path));
        }
    }
    let mut buf = [0u8; 24];
    getrandom::fill(&mut buf).map_err(|e| std::io::Error::other(e.to_string()))?;
    let token: String = buf.iter().map(|b| format!("{b:02x}")).collect();
    std::fs::create_dir_all(&dir)?;
    std::fs::write(&path, &token)?;
    Ok((token, path))
}

fn config_dir() -> PathBuf {
    std::env::var_os("APPDATA")
        .or_else(|| std::env::var_os("XDG_CONFIG_HOME"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from("."))
        .join("rusty486")
}

/// Start the server on a background thread. Returns the URL it listens on.
pub fn start(port: u16, token: String, jobs: Sender<Job>) -> Result<String, String> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .map_err(|e| e.to_string())?;
    let listener = rt
        .block_on(tokio::net::TcpListener::bind(("127.0.0.1", port)))
        .map_err(|e| format!("cannot listen on 127.0.0.1:{port}: {e}"))?;

    let server = Rusty486Mcp::new(jobs);
    let service = StreamableHttpService::new(
        move || Ok(server.clone()),
        Arc::new(LocalSessionManager::default()),
        StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new()
        .nest_service("/mcp", service)
        .layer(middleware::from_fn_with_state(Arc::new(token), require_token));

    std::thread::Builder::new()
        .name("mcp-server".into())
        .spawn(move || {
            rt.block_on(async move {
                if let Err(e) = axum::serve(listener, router).await {
                    eprintln!("MCP server stopped: {e}");
                }
            })
        })
        .map_err(|e| e.to_string())?;
    Ok(format!("http://127.0.0.1:{port}/mcp"))
}

/// Reject requests without `Authorization: Bearer <token>`.
async fn require_token(State(token): State<Arc<String>>, req: Request, next: Next) -> Response {
    let presented = req
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .unwrap_or("");
    // Compare in constant time so the token can't be guessed byte by byte.
    let ok = presented.len() == token.len()
        && presented.bytes().zip(token.bytes()).fold(0u8, |acc, (a, b)| acc | (a ^ b)) == 0;
    if ok {
        next.run(req).await
    } else {
        (StatusCode::UNAUTHORIZED, "missing or invalid bearer token").into_response()
    }
}

// ------------------------------------------------------------------ parameters

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AddressArg {
    /// Address: "SEG:OFF" (hex, SEG may be a register name like "cs") or a
    /// linear hex address like "B8000" / "0xB8000".
    pub address: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ReadMemoryArgs {
    /// Address: "SEG:OFF" (hex, SEG may be a register name like "cs") or a linear hex address.
    pub address: String,
    /// Number of bytes to read (max 4096).
    pub length: u32,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WriteMemoryArgs {
    /// Address: "SEG:OFF" (hex, SEG may be a register name like "cs") or a linear hex address.
    pub address: String,
    /// Bytes to write as hex, e.g. "B8 00 4C CD 21" (spaces optional).
    pub bytes: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetRegisterArgs {
    /// Register name: eax..edi, ax..di, al..bh, eip/ip, eflags/flags, cs/ds/es/ss/fs/gs, cr0.
    pub name: String,
    /// New value in hex.
    pub value: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct StepArgs {
    /// Number of instructions to execute (default 1, max 100000).
    pub count: Option<u32>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct RunArgs {
    /// Emulated milliseconds to run (default 1000). Runs at the machine's real
    /// clock speed, so this is also roughly the wall-clock wait. Max 120000.
    pub milliseconds: Option<u64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DebugLogArgs {
    /// Clear the log after reading it (default false).
    pub clear: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DiskInsertArgs {
    /// Drive letter: A, B (floppies) or C, D (hard disks).
    pub drive: String,
    /// Path to a raw disk image on the host.
    pub path: String,
    /// If the file doesn't exist, create an image of this many KB
    /// (1440 for a 1.44 MB floppy).
    pub create_kb: Option<u32>,
    /// Format for a newly created image: "fat" (default), "tinydos" (bootable) or "blank".
    pub format: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DriveArg {
    /// Drive letter: A, B, C or D.
    pub drive: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct LoadFileArgs {
    /// Path to a file on the host.
    pub path: String,
    /// Where to load it: "SEG:OFF" or a linear hex address.
    pub address: String,
    /// Also point CS:IP at the load address (needs "SEG:OFF" form). Default false.
    pub jump: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ScreenshotArgs {
    /// "machine" (default), "editor", "current" (whatever the window shows),
    /// or "settings" (the window with the Settings overlay, if it is open).
    pub view: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SetSettingArgs {
    /// fullscreen, aspect ("Crt", "Square", "Stretch"), integer_scale, smooth,
    /// scanlines, window_scale (1-4), mhz, pause_in_menu, muted, volume (0-100).
    pub name: String,
    /// The new value (bool, number or string).
    pub value: Value,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SettingsMenuArgs {
    /// "open", "close", or "key" to press a key in the menu.
    pub action: String,
    /// For "key": an SDL key name such as "Down", "Return", "Tab", "Escape", "Left".
    pub key: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct ViewArgs {
    /// "machine", "editor", or an editor tab: "code", "sprites", "map", "sound", "disks".
    pub view: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct SourceArgs {
    /// Path relative to the project folder (default: the entry file, src/main.asm).
    pub path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct WriteSourceArgs {
    /// Path relative to the project folder (default: the entry file, src/main.asm).
    pub path: Option<String>,
    /// The complete new file contents.
    pub content: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct TypeTextArgs {
    /// Text to type into the machine (US keyboard layout; \n = Enter).
    pub text: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct PressKeyArgs {
    /// Key or combination: "enter", "esc", "f1", "up", "a", "ctrl+c", "alt+x", "shift+tab".
    pub key: String,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DiskPathArgs {
    /// Drive letter: A, B, C or D.
    pub drive: String,
    /// Path on the FAT disk, e.g. "\\" or "GAMES\\README.TXT" (either slash works).
    pub path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DiskReadArgs {
    /// Drive letter: A, B, C or D.
    pub drive: String,
    /// File path on the FAT disk.
    pub path: String,
    /// Return a hex dump instead of text (default: text unless the file looks binary).
    pub hex: Option<bool>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DiskWriteArgs {
    /// Drive letter: A, B, C or D.
    pub drive: String,
    /// Destination path on the FAT disk (8.3 names; directories are created).
    pub path: String,
    /// Text content to write. Use host_path instead for binary files.
    pub content: Option<String>,
    /// A host file to copy onto the disk.
    pub host_path: Option<String>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct DosCommandArgs {
    /// A TinyDOS command line, e.g. "DIR", "TYPE README.TXT", "GAME.EXE".
    pub command: String,
    /// Emulated milliseconds to let it run after typing (default 1500).
    pub wait_ms: Option<u64>,
}

#[derive(Deserialize, schemars::JsonSchema)]
pub struct AssembleArgs {
    /// NASM-syntax source to assemble. Defaults to the editor's current buffer.
    pub source: Option<String>,
}

// ------------------------------------------------------------------ server

#[derive(Clone)]
pub struct Rusty486Mcp {
    tool_router: ToolRouter<Self>,
    jobs: Sender<Job>,
}

impl Rusty486Mcp {
    fn new(jobs: Sender<Job>) -> Self {
        Rusty486Mcp { tool_router: Self::tool_router() + Self::project_router(), jobs }
    }

    /// Run `f` on the UI thread and return its result.
    async fn with<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Emu) -> T + Send + 'static,
    ) -> Result<T, McpError> {
        let (tx, rx) = oneshot::channel();
        self.jobs
            .send(Box::new(move |emu: &mut Emu| {
                let _ = tx.send(f(emu));
            }))
            .map_err(|_| McpError::internal_error("emulator is shutting down", None))?;
        rx.await.map_err(|_| McpError::internal_error("emulator dropped the request", None))
    }

    /// Like `with`, for closures that can fail with a message for the caller.
    async fn try_with(
        &self,
        f: impl FnOnce(&mut Emu) -> Result<Value, String> + Send + 'static,
    ) -> Result<CallToolResult, McpError> {
        Ok(match self.with(f).await? {
            Ok(v) => json_result(v),
            Err(msg) => CallToolResult::error(vec![ContentBlock::text(msg)]),
        })
    }
}

fn json_result(v: Value) -> CallToolResult {
    let text = match v {
        Value::String(s) => s,
        v => serde_json::to_string_pretty(&v).unwrap_or_default(),
    };
    CallToolResult::success(vec![ContentBlock::text(text)])
}

#[tool_router]
impl Rusty486Mcp {
    #[tool(description = "Machine overview: running/paused (and why), cycle count, clock speed, \
        CS:IP, halted state, mounted disks and breakpoints.")]
    async fn status(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| Ok(status_json(emu))).await
    }

    #[tool(description = "Capture the screen as a PNG image (640x400, 80x25 text mode). By \
        default this is the emulated machine's display; view=\"editor\" shows the built-in editor.")]
    async fn screenshot(
        &self,
        Parameters(a): Parameters<ScreenshotArgs>,
    ) -> Result<CallToolResult, McpError> {
        let png = self
            .with(move |emu| {
                let view = match a.view.as_deref() {
                    Some("editor") => View::Editor,
                    Some("current") => emu.view,
                    _ => View::Machine,
                };
                let blink = emu.blink_on;
                emu.blink_on = true;
                let settings_view = a.view.as_deref() == Some("settings");
                emu.render(if settings_view { emu.view } else { view });
                emu.blink_on = blink;
                if settings_view && emu.menu_open() {
                    let mut m = emu.menu.take().unwrap();
                    let ui = m.render(emu, &emu.settings).to_vec();
                    emu.menu = Some(m);
                    crate::settings::composite(&mut emu.screen, &ui);
                }
                emu.screen.png()
            })
            .await?;
        let data = base64::engine::general_purpose::STANDARD.encode(png);
        Ok(CallToolResult::success(vec![ContentBlock::image(data, "image/png")]))
    }

    #[tool(description = "The host settings of the rusty486 window (video, speed, audio) and \
        whether the Settings window (Ctrl+F12) is open.")]
    async fn get_settings(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            let mut v = serde_json::to_value(&emu.settings).map_err(|e| e.to_string())?;
            v["settings_window_open"] = json!(emu.menu_open());
            Ok(v)
        })
        .await
    }

    #[tool(description = "Change one host setting of the rusty486 window and save it, e.g. \
        fullscreen=true, aspect=\"Stretch\", scanlines=true, mhz=66, volume=50.")]
    async fn set_setting(&self, Parameters(a): Parameters<SetSettingArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let mut v = serde_json::to_value(&emu.settings).map_err(|e| e.to_string())?;
            if v.get(&a.name).is_none() {
                return Err(format!("unknown setting '{}'", a.name));
            }
            v[&a.name] = a.value.clone();
            let new: crate::settings::Settings =
                serde_json::from_value(v).map_err(|e| format!("bad value for {}: {e}", a.name))?;
            emu.settings = new;
            emu.machine.clock_hz = emu.settings.mhz as u64 * 1_000_000;
            emu.settings.save();
            emu.effects.push(crate::settings::Effect::Video);
            if a.name == "window_scale" || a.name == "aspect" {
                emu.effects.push(crate::settings::Effect::WindowScale);
            }
            serde_json::to_value(&emu.settings).map_err(|e| e.to_string())
        })
        .await
    }

    #[tool(description = "Open or close the Settings window (as Ctrl+F12 does), or press a key \
        in it. Use screenshot with view \"settings\" to see it.")]
    async fn settings_menu(&self, Parameters(a): Parameters<SettingsMenuArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            match a.action.as_str() {
                "open" | "close" => {
                    let want = a.action == "open";
                    if emu.menu_open() != want {
                        emu.with_menu(|m, _, _| m.toggle());
                    }
                }
                "key" => {
                    if !emu.menu_open() {
                        return Err("the Settings window is not open".into());
                    }
                    let name = a.key.as_deref().unwrap_or("");
                    let k = sdl2::keyboard::Keycode::from_name(name).ok_or(format!("unknown key '{name}'"))?;
                    let fx = emu.with_menu(|m, e, s| m.key(k, e, s));
                    emu.effects.extend(fx);
                }
                other => return Err(format!("unknown action '{other}' (open, close, key)")),
            }
            Ok(json!({ "open": emu.menu_open() }))
        })
        .await
    }

    #[tool(description = "Read the 80x25 text screen as plain text, plus the cursor position. \
        Cheaper than a screenshot when only the text matters.")]
    async fn screen_text(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            let cur = emu.machine.bus.video.cursor_offset() as usize;
            let lines: Vec<String> = emu.machine.screen_text().lines().map(String::from).collect();
            Ok(json!({
                "cursor": { "row": cur / 80, "col": cur % 80 },
                "screen": lines.join("\n"),
            }))
        })
        .await
    }

    #[tool(description = "Read all CPU registers, segment registers, flags and control registers.")]
    async fn get_registers(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| Ok(registers_json(&emu.machine.cpu))).await
    }

    #[tool(description = "Set a CPU register. Values are hex. Segment registers use real-mode \
        semantics (base = value * 16).")]
    async fn set_register(
        &self,
        Parameters(a): Parameters<SetRegisterArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let v = parse_hex(&a.value)?;
            set_register(&mut emu.machine.cpu, &a.name, v)?;
            Ok(registers_json(&emu.machine.cpu))
        })
        .await
    }

    #[tool(description = "Read emulated memory as a hex dump with ASCII. Addresses are physical \
        (A20 enabled); the BIOS ROM is visible at F0000-FFFFF.")]
    async fn read_memory(
        &self,
        Parameters(a): Parameters<ReadMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let start = parse_address(&a.address, &emu.machine.cpu)?;
            let len = a.length.min(4096);
            let bytes: Vec<u8> =
                (0..len).map(|i| emu.machine.bus.read8(start.wrapping_add(i))).collect();
            Ok(Value::String(hex_dump(start, &bytes)))
        })
        .await
    }

    #[tool(description = "Write bytes into emulated memory (ROM writes are ignored).")]
    async fn write_memory(
        &self,
        Parameters(a): Parameters<WriteMemoryArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let start = parse_address(&a.address, &emu.machine.cpu)?;
            let bytes = parse_bytes(&a.bytes)?;
            for (i, b) in bytes.iter().enumerate() {
                emu.machine.bus.write8(start.wrapping_add(i as u32), *b);
            }
            Ok(json!({ "written": bytes.len(), "address": format!("{start:05X}") }))
        })
        .await
    }

    #[tool(description = "Pause emulation.")]
    async fn pause(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            emu.pause("paused via MCP".into());
            Ok(status_json(emu))
        })
        .await
    }

    #[tool(description = "Resume emulation at normal speed (returns immediately; use `run` to \
        wait for a while).")]
    async fn resume(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            emu.resume();
            Ok(status_json(emu))
        })
        .await
    }

    #[tool(description = "Execute instructions one at a time (pauses the machine first), then \
        return the registers. Stops early at breakpoints after the first instruction.")]
    async fn step(&self, Parameters(a): Parameters<StepArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            emu.pause("single-stepping".into());
            let n = a.count.unwrap_or(1).clamp(1, 100_000);
            let mut executed = 0;
            for i in 0..n {
                emu.machine.step();
                executed += 1;
                if i + 1 < n && emu.machine.breakpoints.contains(&emu.machine.pc()) {
                    break;
                }
            }
            let mut v = registers_json(&emu.machine.cpu);
            v["executed"] = json!(executed);
            Ok(v)
        })
        .await
    }

    #[tool(description = "Resume and wait while the machine runs for the given emulated time, \
        or until it hits a breakpoint. Returns the status afterwards.")]
    async fn run(&self, Parameters(a): Parameters<RunArgs>) -> Result<CallToolResult, McpError> {
        let ms = a.milliseconds.unwrap_or(1000).min(MAX_RUN_WAIT.as_millis() as u64);
        let (tx, rx) = oneshot::channel();
        self.with(move |emu| {
            let cycles = emu.machine.clock_hz * ms / 1000;
            emu.resume();
            emu.waiters.push(Waiter { until_cycle: emu.machine.cpu.cycles + cycles, reply: tx });
        })
        .await?;
        let outcome = match tokio::time::timeout(MAX_RUN_WAIT + Duration::from_secs(5), rx).await {
            Ok(Ok(msg)) => msg,
            _ => "timed out waiting for the emulator".to_string(),
        };
        self.try_with(move |emu| {
            let mut v = status_json(emu);
            v["run"] = json!(outcome);
            Ok(v)
        })
        .await
    }

    #[tool(description = "Hardware reset: the CPU restarts at F000:FFF0 and the BIOS boots \
        again. Memory, disks and breakpoints are kept. Leaves the machine running.")]
    async fn reset(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            emu.machine.reset();
            emu.resume();
            Ok(status_json(emu))
        })
        .await
    }

    #[tool(description = "Add an execution breakpoint. Execution pauses before the instruction \
        at this address runs.")]
    async fn breakpoint_add(
        &self,
        Parameters(a): Parameters<AddressArg>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let addr = parse_address(&a.address, &emu.machine.cpu)?;
            emu.machine.breakpoints.insert(addr);
            Ok(breakpoints_json(emu))
        })
        .await
    }

    #[tool(description = "Remove an execution breakpoint, or all of them with address \"all\".")]
    async fn breakpoint_remove(
        &self,
        Parameters(a): Parameters<AddressArg>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            if a.address.eq_ignore_ascii_case("all") {
                emu.machine.breakpoints.clear();
            } else {
                let addr = parse_address(&a.address, &emu.machine.cpu)?;
                if !emu.machine.breakpoints.remove(&addr) {
                    return Err(format!("no breakpoint at {addr:05X}"));
                }
            }
            Ok(breakpoints_json(emu))
        })
        .await
    }

    #[tool(description = "Read text written to the debug console (port E9h). The BIOS mirrors \
        everything it prints there, and guest programs can write to it for logging.")]
    async fn debug_log(
        &self,
        Parameters(a): Parameters<DebugLogArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let text = emu.log.clone();
            if a.clear.unwrap_or(false) {
                emu.log.clear();
            }
            Ok(Value::String(if text.is_empty() { "(empty)".into() } else { text }))
        })
        .await
    }

    #[tool(description = "Mount a raw disk image file in a drive (replacing any disk there, \
        which is saved first if modified). Reset afterwards to boot from it.")]
    async fn disk_insert(
        &self,
        Parameters(a): Parameters<DiskInsertArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let path = PathBuf::from(&a.path);
            let data = match std::fs::read(&path) {
                Ok(d) => d,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => match a.create_kb {
                    Some(kb) => {
                        let mut d = vec![0u8; kb as usize * 1024];
                        match a.format.as_deref().unwrap_or("fat") {
                            "blank" => {}
                            "tinydos" => rusty486_firmware::format_tinydos(&mut d, "RUSTY486").map_err(|e| e.to_string())?,
                            _ => {
                                let g = rusty486_core::fat::Geometry::for_size(d.len(), 1);
                                rusty486_core::fat::format(&mut d, &[], &g, "RUSTY486");
                            }
                        }
                        std::fs::write(&path, &d).map_err(|e| format!("{}: {e}", a.path))?;
                        d
                    }
                    None => return Err(format!("{}: file not found (pass create_kb to create it)", a.path)),
                },
                Err(e) => return Err(format!("{}: {e}", a.path)),
            };
            emu.insert_disk(drive, path, DiskImage::new(data));
            Ok(disks_json(emu))
        })
        .await
    }

    #[tool(description = "Eject the disk from a drive, saving it to its file first if modified.")]
    async fn disk_eject(&self, Parameters(a): Parameters<DriveArg>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            emu.save_disk(drive).map_err(|e| e.to_string())?;
            emu.machine.eject_disk(drive);
            emu.disk_paths[drive as usize] = None;
            Ok(disks_json(emu))
        })
        .await
    }

    #[tool(description = "Write a drive's image back to its host file now (normally this happens \
        on eject and when rusty486 exits).")]
    async fn disk_save(&self, Parameters(a): Parameters<DriveArg>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let saved = emu.save_disk(drive).map_err(|e| e.to_string())?;
            Ok(json!({ "saved": saved, "disks": disks_json(emu) }))
        })
        .await
    }

    #[tool(description = "List mounted disks with their geometry and host files.")]
    async fn disk_list(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| Ok(disks_json(emu))).await
    }

    #[tool(description = "Copy a host file straight into emulated memory, e.g. a flat binary \
        assembled with `nasm -f bin`. With jump=true, CS:IP is set to the load address so the \
        code runs next (use a SEG:OFF address, e.g. 1000:0100).")]
    async fn load_file(&self, Parameters(a): Parameters<LoadFileArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let data = std::fs::read(&a.path).map_err(|e| format!("{}: {e}", a.path))?;
            let start = parse_address(&a.address, &emu.machine.cpu)?;
            for (i, b) in data.iter().enumerate() {
                emu.machine.bus.write8(start.wrapping_add(i as u32), *b);
            }
            if a.jump.unwrap_or(false) {
                let (seg, off) = a
                    .address
                    .split_once(':')
                    .ok_or("jump needs a SEG:OFF address")?;
                let seg = parse_segment(seg, &emu.machine.cpu)?;
                let off = parse_hex(off)?;
                let cpu = &mut emu.machine.cpu;
                cpu.set_segment(cpu::CS, seg);
                cpu.eip = off;
                cpu.halted = false;
            }
            Ok(json!({ "loaded": data.len(), "address": format!("{start:05X}"), "registers": registers_json(&emu.machine.cpu) }))
        })
        .await
    }
}

#[tool_router(router = project_router)]
impl Rusty486Mcp {
    #[tool(description = "The open project: folder, entry file, source files, whether the editor \
        has unsaved changes, and the last build result.")]
    async fn project_info(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            Ok(json!({
                "name": emu.project.name,
                "folder": emu.project.dir.display().to_string(),
                "entry": emu.project.entry,
                "sources": emu.project.sources(),
                "editor_file": emu.editor.path.display().to_string(),
                "editor_unsaved_changes": emu.editor.dirty,
                "view": if emu.view == View::Editor { "editor" } else { "machine" },
                "last_build": emu.last_build.as_ref().map(build_json),
                "notes": "Programs are flat binaries with 'org 0x100', loaded at 1000:0100 \
                          (CS=DS=ES=SS=1000h) from a boot floppy built into build/disk.img. \
                          Files can also be edited directly on disk; the editor reloads them.",
            }))
        })
        .await
    }

    #[tool(description = "Read a project source file. For the file open in the editor, this \
        returns the editor's buffer (including unsaved changes).")]
    async fn read_source(&self, Parameters(a): Parameters<SourceArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let path = match &a.path {
                Some(p) => emu.project.resolve(p)?,
                None => emu.project.entry_path(),
            };
            if path == emu.editor.path {
                return Ok(Value::String(emu.editor.text()));
            }
            std::fs::read_to_string(&path).map(Value::String).map_err(|e| format!("{}: {e}", path.display()))
        })
        .await
    }

    #[tool(description = "Replace a project source file's contents and save it. If it's the \
        file open in the editor, the editor shows the new text (undoable with Ctrl+Z).")]
    async fn write_source(
        &self,
        Parameters(a): Parameters<WriteSourceArgs>,
    ) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let path = match &a.path {
                Some(p) => emu.project.resolve(p)?,
                None => emu.project.entry_path(),
            };
            if path == emu.editor.path {
                emu.editor.set_text(&a.content);
                emu.editor.save().map_err(|e| e.to_string())?;
                emu.editor.set_status("Updated via MCP", Level::Info);
            } else {
                if let Some(dir) = path.parent() {
                    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
                }
                std::fs::write(&path, &a.content).map_err(|e| format!("{}: {e}", path.display()))?;
            }
            Ok(json!({ "written": path.display().to_string(), "bytes": a.content.len() }))
        })
        .await
    }

    #[tool(description = "Assemble NASM-syntax source with rusty486's built-in assembler without \
        running it. Returns errors with line numbers, or the size, symbols and a hex dump.")]
    async fn assemble(&self, Parameters(a): Parameters<AssembleArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let src = a.source.unwrap_or_else(|| emu.editor.text());
            Ok(match rusty486_asm::assemble(&src) {
                Ok(out) => json!({
                    "ok": true,
                    "origin": format!("{:X}", out.origin),
                    "bytes": out.bytes.len(),
                    "symbols": out.symbols.iter().map(|(k, v)| (k.clone(), json!(format!("{v:X}")))).collect::<serde_json::Map<_, _>>(),
                    "hex": hex_dump(out.origin, &out.bytes[..out.bytes.len().min(512)]),
                }),
                Err(errors) => json!({
                    "ok": false,
                    "errors": errors.iter().map(|e| json!({ "line": e.line, "message": e.message })).collect::<Vec<_>>(),
                }),
            })
        })
        .await
    }

    #[tool(description = "Save and assemble the project's entry file (the editor buffer) into \
        build/main.bin and build/disk.img, without running it. Errors are also shown in the editor.")]
    async fn build(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            emu.editor.check_disk();
            let (summary, _) = emu.build();
            Ok(build_json(&summary))
        })
        .await
    }

    #[tool(description = "Like Ctrl+F9 in the editor: save, assemble, write the boot floppy to \
        drive A, reset the machine and run the program. On errors the editor is shown instead. \
        Follow with `run` and `screen_text`/`screenshot` to see the result.")]
    async fn build_and_run(&self) -> Result<CallToolResult, McpError> {
        self.try_with(|emu| {
            emu.editor.check_disk();
            let summary = emu.build_and_run();
            let mut v = build_json(&summary);
            v["status"] = status_json(emu);
            Ok(v)
        })
        .await
    }

    #[tool(description = "Type text into the emulated machine's keyboard (queued at about 60 \
        keys per second so the BIOS buffer doesn't overflow). Use \\n for Enter.")]
    async fn type_text(&self, Parameters(a): Parameters<TypeTextArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let mut skipped = String::new();
            for c in a.text.chars() {
                match keymap::key_for_char(c) {
                    Some(k) => emu.typing.push_back(k),
                    None => skipped.push(c),
                }
            }
            let n = emu.typing.len();
            Ok(json!({
                "queued_keys": n,
                "approx_seconds": n as f64 / 60.0,
                "skipped_characters": skipped,
            }))
        })
        .await
    }

    #[tool(description = "Press and release one key or a combination on the emulated keyboard, \
        e.g. \"enter\", \"esc\", \"f1\", \"up\", \"ctrl+c\", \"alt+f\".")]
    async fn press_key(&self, Parameters(a): Parameters<PressKeyArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let mut flags = 0u8;
            let parts: Vec<&str> = a.key.split('+').map(str::trim).collect();
            let (last, mods) = parts.split_last().ok_or("empty key")?;
            for m in mods {
                flags |= match m.to_ascii_lowercase().as_str() {
                    "ctrl" | "control" => rusty486_core::devices::input::shift::CTRL,
                    "alt" => rusty486_core::devices::input::shift::ALT,
                    "shift" => rusty486_core::devices::input::shift::LSHIFT,
                    other => return Err(format!("unknown modifier '{other}'")),
                };
            }
            let key = keymap::key_by_name(last).ok_or_else(|| format!("unknown key '{last}'"))?;
            emu.tap_key(key, flags);
            Ok(json!({ "pressed": a.key }))
        })
        .await
    }

    #[tool(description = "List a directory on a mounted FAT disk image (read directly from the \
        image, without involving the guest).")]
    async fn disk_files(&self, Parameters(a): Parameters<DiskPathArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let path = a.path.unwrap_or_default();
            let disk = emu.machine.disk(drive).ok_or("no disk in that drive")?;
            let mut data = disk.data.clone();
            let fs = Fs::open(&mut data).map_err(|e| e.to_string())?;
            let entries = fs.list(&path).map_err(|e| e.to_string())?;
            Ok(json!({
                "path": path,
                "free_bytes": fs.free_bytes(),
                "entries": entries.iter().map(|e| json!({
                    "name": e.name,
                    "dir": e.is_dir(),
                    "size": e.size,
                    "date": format!("{:04}-{:02}-{:02}", 1980 + (e.date >> 9), (e.date >> 5) & 15, e.date & 31),
                })).collect::<Vec<_>>(),
            }))
        })
        .await
    }

    #[tool(description = "Read a file from a mounted FAT disk image (text, or a hex dump for \
        binary files; up to 64 KB).")]
    async fn disk_read_file(&self, Parameters(a): Parameters<DiskReadArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let disk = emu.machine.disk(drive).ok_or("no disk in that drive")?;
            let mut data = disk.data.clone();
            let fs = Fs::open(&mut data).map_err(|e| e.to_string())?;
            let bytes = fs.read(&a.path).map_err(|e| e.to_string())?;
            let shown = &bytes[..bytes.len().min(65536)];
            let binary = shown.iter().any(|&b| b < 9 || (b > 13 && b < 32 && b != 26));
            if a.hex.unwrap_or(binary) {
                Ok(Value::String(hex_dump(0, shown)))
            } else {
                Ok(Value::String(String::from_utf8_lossy(shown).into_owned()))
            }
        })
        .await
    }

    #[tool(description = "Write a file onto a mounted FAT disk image (from text content or a \
        host file). The guest sees it immediately; the image is saved to its host file on exit \
        or with disk_save.")]
    async fn disk_write_file(&self, Parameters(a): Parameters<DiskWriteArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let bytes = match (&a.content, &a.host_path) {
                (Some(text), None) => text.replace("\r\n", "\n").replace('\n', "\r\n").into_bytes(),
                (None, Some(p)) => std::fs::read(p).map_err(|e| format!("{p}: {e}"))?,
                _ => return Err("give exactly one of content or host_path".into()),
            };
            let disk = emu.machine.disk_mut(drive).ok_or("no disk in that drive")?;
            let mut fs = Fs::open(&mut disk.data).map_err(|e| e.to_string())?;
            fs.write_creating_dirs(&a.path, &bytes).map_err(|e| e.to_string())?;
            disk.dirty = true;
            Ok(json!({ "written": a.path, "bytes": bytes.len() }))
        })
        .await
    }

    #[tool(description = "Delete a file or empty directory on a mounted FAT disk image.")]
    async fn disk_delete(&self, Parameters(a): Parameters<DiskPathArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            let drive = parse_drive(&a.drive)?;
            let path = a.path.ok_or("path is required")?;
            let disk = emu.machine.disk_mut(drive).ok_or("no disk in that drive")?;
            let mut fs = Fs::open(&mut disk.data).map_err(|e| e.to_string())?;
            fs.remove(&path).map_err(|e| e.to_string())?;
            disk.dirty = true;
            Ok(json!({ "deleted": path }))
        })
        .await
    }

    #[tool(description = "Type a command line into TinyDOS (followed by Enter), let the \
        machine run for a while, and return the screen text. The machine must be at a DOS prompt.")]
    async fn dos_command(&self, Parameters(a): Parameters<DosCommandArgs>) -> Result<CallToolResult, McpError> {
        let wait_ms = a.wait_ms.unwrap_or(1500).min(60_000);
        let (tx, rx) = oneshot::channel();
        let len = a.command.len() as u64;
        self.with(move |emu| {
            for c in a.command.chars().chain(std::iter::once('\n')) {
                if let Some(k) = keymap::key_for_char(c) {
                    emu.typing.push_back(k);
                }
            }
            emu.resume();
            // Allow time for typing (about 2 keys per frame) plus the wait.
            let ms = wait_ms + (len + 1) * 10;
            let cycles = emu.machine.clock_hz * ms / 1000;
            emu.waiters.push(Waiter { until_cycle: emu.machine.cpu.cycles + cycles, reply: tx });
        })
        .await?;
        let _ = tokio::time::timeout(Duration::from_secs(90), rx).await;
        self.try_with(|emu| Ok(Value::String(emu.machine.screen_text()))).await
    }

    #[tool(description = "Switch the rusty486 window to the machine display, the editor, or a         specific editor tab (code, sprites, map, sound, disks).")]
    async fn set_view(&self, Parameters(a): Parameters<ViewArgs>) -> Result<CallToolResult, McpError> {
        self.try_with(move |emu| {
            use crate::editor::Tab;
            let tab = match a.view.as_str() {
                "machine" => {
                    emu.view = View::Machine;
                    return Ok(json!({ "view": "machine" }));
                }
                "editor" => None,
                "code" => Some(Tab::Code),
                "sprites" => Some(Tab::Sprites),
                "map" => Some(Tab::Map),
                "sound" => Some(Tab::Sound),
                "disks" => Some(Tab::Disks),
                other => return Err(format!("unknown view '{other}' (machine, editor, code, sprites, map, sound, disks)")),
            };
            emu.view = View::Editor;
            if let Some(t) = tab {
                emu.editor.tab = t;
            }
            Ok(json!({ "view": a.view }))
        })
        .await
    }
}

fn build_json(b: &BuildSummary) -> Value {
    json!({
        "ok": b.ok,
        "bytes": b.bytes,
        "errors": b.errors.iter().map(|e| json!({ "line": e.line, "message": e.message })).collect::<Vec<_>>(),
    })
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for Rusty486Mcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build()).with_instructions(
            "rusty486 is a fantasy 486 PC emulator: an i486 CPU in real mode with a PC-style \
             BIOS and 80x25 colour text mode, plus a built-in DOS-style editor and NASM-syntax \
             assembler. These tools control the live machine shown in the rusty486 window. \
             Addresses are hex: 'SEG:OFF' (e.g. 'B800:0000', 'cs:0100') or linear ('B8000'). \
             Typical loop: project_info, then read_source/write_source (or edit the project's \
             files directly), build_and_run, then run and inspect with screen_text, screenshot, \
             get_registers, read_memory and breakpoints. Programs use 'org 0x100' and are loaded \
             at 1000:0100. They can log text by writing bytes to port E9h (see debug_log).",
        )
    }
}

// ------------------------------------------------------------------ helpers

fn status_json(emu: &Emu) -> Value {
    let m = &emu.machine;
    let c = &m.cpu;
    let state = if c.shutdown {
        "shutdown"
    } else if emu.paused {
        "paused"
    } else if c.halted {
        "halted"
    } else {
        "running"
    };
    json!({
        "state": state,
        "stop_reason": if emu.stop_reason.is_empty() { Value::Null } else { json!(emu.stop_reason) },
        "cs_ip": format!("{:04X}:{:04X}", c.sregs[cpu::CS], c.eip),
        "pc_linear": format!("{:05X}", m.pc()),
        "halted": c.halted,
        "interrupts_enabled": c.eflags & flags::IF != 0,
        "cycles": c.cycles,
        "clock_mhz": m.clock_hz as f64 / 1e6,
        "emulated_seconds": c.cycles as f64 / m.clock_hz as f64,
        "disks": disks_json(emu),
        "breakpoints": breakpoints_json(emu),
    })
}

fn registers_json(c: &Cpu) -> Value {
    let f = c.eflags;
    let bit = |b: u32, s: &'static str| if f & b != 0 { s } else { "-" };
    let flag_str = [
        bit(flags::OF, "O"),
        bit(flags::DF, "D"),
        bit(flags::IF, "I"),
        bit(flags::TF, "T"),
        bit(flags::SF, "S"),
        bit(flags::ZF, "Z"),
        bit(flags::AF, "A"),
        bit(flags::PF, "P"),
        bit(flags::CF, "C"),
    ]
    .concat();
    let r = |i: usize| format!("{:08X}", c.regs[i]);
    let s = |i: usize| format!("{:04X}", c.sregs[i]);
    json!({
        "eax": r(cpu::EAX), "ebx": r(cpu::EBX), "ecx": r(cpu::ECX), "edx": r(cpu::EDX),
        "esi": r(cpu::ESI), "edi": r(cpu::EDI), "ebp": r(cpu::EBP), "esp": r(cpu::ESP),
        "eip": format!("{:08X}", c.eip),
        "cs": s(cpu::CS), "ds": s(cpu::DS), "es": s(cpu::ES), "ss": s(cpu::SS),
        "fs": s(cpu::FS), "gs": s(cpu::GS),
        "eflags": format!("{f:08X}"),
        "flags": flag_str,
        "cr0": format!("{:08X}", c.cr0),
        "halted": c.halted,
    })
}

fn breakpoints_json(emu: &Emu) -> Value {
    json!(emu.machine.breakpoints.iter().map(|a| format!("{a:05X}")).collect::<Vec<_>>())
}

fn disks_json(emu: &Emu) -> Value {
    let mut out = serde_json::Map::new();
    for (i, letter) in ["A", "B", "C", "D"].iter().enumerate() {
        if let Some(d) = &emu.machine.bus.disk.drives[i] {
            out.insert(
                letter.to_string(),
                json!({
                    "file": emu.disk_paths[i].as_ref().map(|p| p.display().to_string()),
                    "size_kb": d.data.len() / 1024,
                    "chs": format!("{}/{}/{}", d.cylinders, d.heads, d.sectors),
                    "modified": d.dirty,
                    "read_only": d.read_only,
                }),
            );
        }
    }
    Value::Object(out)
}

fn parse_drive(s: &str) -> Result<Drive, String> {
    match s.trim().trim_end_matches(':').to_ascii_uppercase().as_str() {
        "A" => Ok(Drive::A),
        "B" => Ok(Drive::B),
        "C" => Ok(Drive::C),
        "D" => Ok(Drive::D),
        _ => Err(format!("unknown drive '{s}' (use A, B, C or D)")),
    }
}

fn parse_hex(s: &str) -> Result<u32, String> {
    let t = s.trim();
    let t = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")).unwrap_or(t);
    let t = t.strip_suffix(['h', 'H']).unwrap_or(t).replace('_', "");
    u32::from_str_radix(&t, 16).map_err(|_| format!("'{s}' is not a hex number"))
}

fn parse_segment(s: &str, cpu: &Cpu) -> Result<u16, String> {
    let idx = match s.trim().to_ascii_lowercase().as_str() {
        "es" => Some(cpu::ES),
        "cs" => Some(cpu::CS),
        "ss" => Some(cpu::SS),
        "ds" => Some(cpu::DS),
        "fs" => Some(cpu::FS),
        "gs" => Some(cpu::GS),
        _ => None,
    };
    match idx {
        Some(i) => Ok(cpu.sregs[i]),
        None => {
            let v = parse_hex(s)?;
            u16::try_from(v).map_err(|_| format!("segment '{s}' is larger than FFFF"))
        }
    }
}

/// "SEG:OFF" (real-mode) or a linear hex address.
fn parse_address(s: &str, cpu: &Cpu) -> Result<u32, String> {
    match s.split_once(':') {
        Some((seg, off)) => {
            let seg = parse_segment(seg, cpu)? as u32;
            Ok((seg << 4).wrapping_add(parse_hex(off)?))
        }
        None => parse_hex(s),
    }
}

fn parse_bytes(s: &str) -> Result<Vec<u8>, String> {
    let digits: String = s.chars().filter(|c| !c.is_whitespace() && *c != ',').collect();
    let digits = digits.replace("0x", "");
    if digits.len() % 2 != 0 {
        return Err("hex byte string has an odd number of digits".into());
    }
    (0..digits.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&digits[i..i + 2], 16).map_err(|_| format!("bad hex '{}'", &digits[i..i + 2])))
        .collect()
}

fn set_register(c: &mut Cpu, name: &str, v: u32) -> Result<(), String> {
    let n = name.trim().to_ascii_lowercase();
    let gpr = ["eax", "ecx", "edx", "ebx", "esp", "ebp", "esi", "edi"];
    let gpr16 = ["ax", "cx", "dx", "bx", "sp", "bp", "si", "di"];
    let r8 = ["al", "cl", "dl", "bl", "ah", "ch", "dh", "bh"];
    let segs = ["es", "cs", "ss", "ds", "fs", "gs"];
    if let Some(i) = gpr.iter().position(|r| *r == n) {
        c.regs[i] = v;
    } else if let Some(i) = gpr16.iter().position(|r| *r == n) {
        c.regs[i] = (c.regs[i] & 0xFFFF_0000) | (v & 0xFFFF);
    } else if let Some(i) = r8.iter().position(|r| *r == n) {
        let (reg, shift) = if i < 4 { (i, 0) } else { (i - 4, 8) };
        c.regs[reg] = (c.regs[reg] & !(0xFF << shift)) | (v & 0xFF) << shift;
    } else if let Some(i) = segs.iter().position(|r| *r == n) {
        c.set_segment(i, v as u16);
    } else {
        match n.as_str() {
            "eip" => c.eip = v,
            "ip" => c.eip = v & 0xFFFF,
            "eflags" | "flags" => c.eflags = (v & 0x0027_7FD5) | 2,
            "cr0" => c.cr0 = v | 0x10,
            _ => return Err(format!("unknown register '{name}'")),
        }
    }
    c.halted = false;
    Ok(())
}

fn hex_dump(start: u32, bytes: &[u8]) -> String {
    let mut out = String::new();
    for (i, chunk) in bytes.chunks(16).enumerate() {
        let addr = start.wrapping_add(i as u32 * 16);
        let hex: Vec<String> = chunk.iter().map(|b| format!("{b:02X}")).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| if (0x20..0x7F).contains(&b) { b as char } else { '.' })
            .collect();
        out.push_str(&format!("{addr:05X}  {:<47}  {ascii}\n", hex.join(" ")));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_addresses() {
        let mut cpu = Cpu::new();
        cpu.set_segment(cpu::DS, 0x1234);
        assert_eq!(parse_address("B800:0010", &cpu), Ok(0xB8010));
        assert_eq!(parse_address("ds:0004", &cpu), Ok(0x12344));
        assert_eq!(parse_address("0xB8000", &cpu), Ok(0xB8000));
        assert_eq!(parse_address("7C00h", &cpu), Ok(0x7C00));
        assert!(parse_address("xyz", &cpu).is_err());
    }

    #[test]
    fn parses_bytes() {
        assert_eq!(parse_bytes("B8 00 4C, cd21"), Ok(vec![0xB8, 0x00, 0x4C, 0xCD, 0x21]));
        assert!(parse_bytes("ABC").is_err());
    }

    #[test]
    fn sets_partial_registers() {
        let mut cpu = Cpu::new();
        cpu.regs[cpu::EAX] = 0x1122_3344;
        set_register(&mut cpu, "ah", 0xAA).unwrap();
        assert_eq!(cpu.regs[cpu::EAX], 0x1122_AA44);
        set_register(&mut cpu, "ax", 0xBEEF).unwrap();
        assert_eq!(cpu.regs[cpu::EAX], 0x1122_BEEF);
    }
}
