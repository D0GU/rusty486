//! Runs the SingleStepTests 80386 real-mode suite against the CPU core.
//!
//! Fetch the data first with `scripts/fetch-tests.ps1`, then:
//!
//! ```text
//! cargo test -p rusty486-core --release --test singlestep -- --nocapture
//! ```
//!
//! Environment variables:
//! - `RUSTY486_TESTDATA`: test data directory (default `<repo>/testdata/386`)
//! - `RUSTY486_TEST_FILTER`: only run files whose name starts with this (e.g. `0F`)
//! - `RUSTY486_TEST_VERBOSE`: print details for up to N failures per file

use std::collections::{HashMap, HashSet};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use rusty486_core::cpu::{Cpu, CS, DS, ES, FS, GS, SS};
use rusty486_core::Bus;

// ------------------------------------------------------------------ MOO format

/// RG32 register order.
const REG_NAMES: [&str; 20] = [
    "cr0", "cr3", "eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp", "cs", "ds", "es", "fs",
    "gs", "ss", "eip", "eflags", "dr6", "dr7",
];
const R_EFLAGS: usize = 17;

#[derive(Default, Clone)]
struct State {
    /// Register values by RG32 index; None = not present.
    regs: [Option<u32>; 20],
    /// RM32 masks, if any.
    masks: [Option<u32>; 20],
    ram: Vec<(u32, u8)>,
}

#[derive(Default)]
struct Test {
    idx: u32,
    name: String,
    bytes: Vec<u8>,
    init: State,
    fina: State,
    exception: Option<(u8, u32)>,
    hash: String,
}

struct Reader<'a> {
    d: &'a [u8],
    p: usize,
}

impl<'a> Reader<'a> {
    fn u8(&mut self) -> u8 {
        self.p += 1;
        self.d[self.p - 1]
    }
    fn u32(&mut self) -> u32 {
        let v = u32::from_le_bytes(self.d[self.p..self.p + 4].try_into().unwrap());
        self.p += 4;
        v
    }
    fn bytes(&mut self, n: usize) -> &'a [u8] {
        self.p += n;
        &self.d[self.p - n..self.p]
    }
    /// Iterate (id, payload) chunks until the end of the slice.
    fn chunks(d: &'a [u8]) -> Vec<([u8; 4], &'a [u8])> {
        let mut r = Reader { d, p: 0 };
        let mut out = Vec::new();
        while r.p + 8 <= d.len() {
            let id: [u8; 4] = r.bytes(4).try_into().unwrap();
            let len = r.u32() as usize;
            out.push((id, r.bytes(len)));
        }
        out
    }
}

fn parse_regs32(d: &[u8], out: &mut [Option<u32>; 20]) {
    let mut r = Reader { d, p: 0 };
    let mask = r.u32();
    for (i, slot) in out.iter_mut().enumerate() {
        if mask & (1 << i) != 0 {
            *slot = Some(r.u32());
        }
    }
}

fn parse_state(d: &[u8]) -> State {
    let mut s = State::default();
    for (id, payload) in Reader::chunks(d) {
        match &id {
            b"RG32" => parse_regs32(payload, &mut s.regs),
            b"RM32" => parse_regs32(payload, &mut s.masks),
            b"RAM " => {
                let mut r = Reader { d: payload, p: 0 };
                let n = r.u32();
                for _ in 0..n {
                    let a = r.u32();
                    s.ram.push((a, r.u8()));
                }
            }
            _ => {}
        }
    }
    s
}

fn parse_moo(data: &[u8]) -> Vec<Test> {
    let mut tests = Vec::new();
    for (id, payload) in Reader::chunks(data) {
        if &id != b"TEST" {
            continue;
        }
        let mut t = Test {
            idx: u32::from_le_bytes(payload[..4].try_into().unwrap()),
            ..Default::default()
        };
        for (id, p) in Reader::chunks(&payload[4..]) {
            match &id {
                b"NAME" => t.name = String::from_utf8_lossy(&p[4..]).into_owned(),
                b"BYTS" => t.bytes = p[4..].to_vec(),
                b"INIT" => t.init = parse_state(p),
                b"FINA" => t.fina = parse_state(p),
                b"EXCP" => {
                    t.exception = Some((p[0], u32::from_le_bytes(p[1..5].try_into().unwrap())))
                }
                b"HASH" => t.hash = p.iter().map(|b| format!("{b:02x}")).collect(),
                _ => {}
            }
        }
        tests.push(t);
    }
    tests
}

// ------------------------------------------------------------------ metadata

/// Undefined-flag masks from 80386.csv, keyed by (opcode, group extension).
fn load_flag_masks(dir: &Path) -> HashMap<(String, Option<u8>), u16> {
    let text = std::fs::read_to_string(dir.join("80386.csv")).expect("80386.csv");
    let mut lines = text.lines();
    let header: Vec<&str> = lines.next().unwrap().split(',').collect();
    let col = |name: &str| header.iter().position(|h| *h == name).unwrap();
    let (c_op, c_ex, c_mask) = (col("op"), col("ex"), col("f_umask"));
    let mut map = HashMap::new();
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        if f.len() <= c_mask {
            continue;
        }
        let ex = f[c_ex].trim().parse::<u8>().ok();
        let m = f[c_mask].trim();
        if let Some(hex) = m.strip_prefix("0x") {
            if let Ok(v) = u16::from_str_radix(hex, 16) {
                map.insert((f[c_op].trim().to_uppercase(), ex), v);
            }
        }
    }
    map
}

/// "6667F7.6.MOO.gz" -> ("F7", Some(6))
fn opcode_key(file: &str) -> (String, Option<u8>) {
    let stem = file.split(".MOO").next().unwrap();
    let mut parts = stem.split('.');
    let mut op = parts.next().unwrap().to_uppercase();
    let ex = parts.next().and_then(|e| e.parse().ok());
    while op.len() > 2 && (op.starts_with("66") || op.starts_with("67")) {
        op = op[2..].to_string();
    }
    (op, ex)
}

// ------------------------------------------------------------------ test bus

struct TestBus {
    ram: Vec<u8>,
    touched: Vec<u32>,
    writes: HashSet<u32>,
}

const ADDR_MASK: u32 = 0xFF_FFFF;

impl Bus for TestBus {
    fn read8(&mut self, addr: u32) -> u8 {
        self.ram[(addr & ADDR_MASK) as usize]
    }
    fn write8(&mut self, addr: u32, value: u8) {
        let a = addr & ADDR_MASK;
        self.ram[a as usize] = value;
        self.touched.push(a);
        self.writes.insert(a);
    }
    fn io_read8(&mut self, port: u16) -> u8 {
        // The 386EX's on-chip peripheral at 22h reads back 7Fh in the test rig.
        if port == 0x22 {
            0x7F
        } else {
            0xFF
        }
    }
    fn io_write8(&mut self, _port: u16, _value: u8) {}
}

impl TestBus {
    fn clear(&mut self) {
        for a in self.touched.drain(..) {
            self.ram[a as usize] = 0;
        }
        self.writes.clear();
    }
}

fn cpu_regs(cpu: &Cpu) -> [u32; 20] {
    let s = |i: usize| cpu.sregs[i] as u32;
    [
        cpu.cr0, cpu.cr3, cpu.regs[0], cpu.regs[3], cpu.regs[1], cpu.regs[2], cpu.regs[6],
        cpu.regs[7], cpu.regs[5], cpu.regs[4], s(CS), s(DS), s(ES), s(FS), s(GS), s(SS), cpu.eip,
        cpu.eflags, cpu.dr[6], cpu.dr[7],
    ]
}

/// Run one test. Returns a description of every mismatch.
fn run_test(t: &Test, bus: &mut TestBus, flag_mask: u16) -> Vec<String> {
    let init: Vec<u32> = t.init.regs.iter().map(|r| r.unwrap_or(0)).collect();

    let mut cpu = Cpu::new();
    // Behave like the 386EX the tests were captured on.
    cpu.popf_mask32 = 0x7FD5;
    cpu.has_cpuid = false;
    cpu.quirks_386 = true;
    cpu.cr0 = init[0];
    cpu.cr3 = init[1];
    for (slot, i) in [(0usize, 2usize), (3, 3), (1, 4), (2, 5), (6, 6), (7, 7), (5, 8), (4, 9)] {
        cpu.regs[slot] = init[i];
    }
    for (seg, i) in [(CS, 10), (DS, 11), (ES, 12), (FS, 13), (GS, 14), (SS, 15)] {
        cpu.sregs[seg] = init[i] as u16;
        cpu.seg_base[seg] = (init[i] & 0xFFFF) << 4;
    }
    cpu.eip = init[16];
    // The upper EFLAGS bits in the dumps are SMM garbage; keep the real ones.
    cpu.eflags = init[R_EFLAGS] & 0x0003_7FD7 | 2;
    cpu.dr[6] = init[18];
    cpu.dr[7] = init[19];

    let mut init_ram = HashMap::new();
    for &(a, v) in &t.init.ram {
        bus.ram[(a & ADDR_MASK) as usize] = v;
        bus.touched.push(a & ADDR_MASK);
        init_ram.insert(a & ADDR_MASK, v);
    }
    bus.writes.clear();

    let mut steps = 0;
    while !cpu.halted && !cpu.shutdown && steps < 100_000 {
        cpu.step(bus);
        steps += 1;
    }

    let mut errs = Vec::new();
    if !cpu.halted {
        errs.push(format!("did not halt (shutdown={})", cpu.shutdown));
    }

    let got = cpu_regs(&cpu);
    for i in 0..20 {
        let want = t.fina.regs[i].unwrap_or(init[i]);
        let mut m = t.fina.masks[i].unwrap_or(0xFFFF_FFFF);
        if (10..16).contains(&i) {
            m &= 0xFFFF;
        }
        if i == R_EFLAGS {
            m &= flag_mask as u32 & 0x7FD7;
        }
        if (got[i] & m) != (want & m) {
            errs.push(format!(
                "{}: got {:08X} want {:08X}",
                REG_NAMES[i],
                got[i] & m,
                want & m
            ));
        }
    }

    // Final memory, masking undefined flags in a pushed FLAGS image.
    let flags_at = t.exception.map(|(_, a)| a & ADDR_MASK);
    let mut expected: HashMap<u32, u8> = HashMap::new();
    for &(a, v) in &t.fina.ram {
        expected.insert(a & ADDR_MASK, v);
    }
    for (&a, &want) in &expected {
        let got = bus.ram[a as usize];
        let m = match flags_at {
            Some(f) if a == f => flag_mask as u8,
            Some(f) if a == f + 1 => (flag_mask >> 8) as u8,
            _ => 0xFF,
        };
        if got & m != want & m {
            errs.push(format!("mem[{a:06X}]: got {got:02X} want {want:02X}"));
        }
    }
    // Writes the hardware didn't make must not change known memory.
    for &a in &bus.writes {
        if !expected.contains_key(&a) {
            if let Some(&orig) = init_ram.get(&a) {
                if bus.ram[a as usize] != orig {
                    errs.push(format!(
                        "mem[{a:06X}]: unexpected write {:02X} (was {orig:02X})",
                        bus.ram[a as usize]
                    ));
                }
            }
        }
    }
    bus.clear();
    errs
}

// ------------------------------------------------------------------ driver

fn testdata_dir() -> PathBuf {
    std::env::var_os("RUSTY486_TESTDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| Path::new(env!("CARGO_MANIFEST_DIR")).join("../../testdata/386"))
}

#[test]
fn singlestep_386_real_mode() {
    let dir = testdata_dir();
    let test_dir = dir.join("real_mode");
    if !test_dir.exists() {
        eprintln!("SKIPPED: no test data at {} (run scripts/fetch-tests.ps1)", test_dir.display());
        return;
    }
    let masks = load_flag_masks(&dir);
    let revoked: HashSet<String> = std::fs::read_to_string(dir.join("revocation_list.txt"))
        .unwrap_or_default()
        .lines()
        .map(|l| l.trim().to_lowercase())
        .collect();
    let filter = std::env::var("RUSTY486_TEST_FILTER").unwrap_or_default().to_uppercase();
    // Tests that exercise 386EX-specific behaviour we deliberately don't emulate.
    let known: HashSet<String> = include_str!("singlestep_known.txt")
        .lines()
        .filter_map(|l| l.split('#').next())
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_uppercase)
        .collect();
    let verbose: usize = std::env::var("RUSTY486_TEST_VERBOSE")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(1);

    let mut files: Vec<PathBuf> = std::fs::read_dir(&test_dir)
        .unwrap()
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| {
            let n = p.file_name().unwrap().to_string_lossy().to_uppercase();
            n.ends_with(".MOO.GZ") && n.starts_with(&filter)
        })
        .collect();
    files.sort();

    let next = AtomicUsize::new(0);
    let report = Mutex::new(Vec::<(String, usize, usize, Vec<String>)>::new());
    let threads = std::thread::available_parallelism().map_or(4, |n| n.get());

    std::thread::scope(|s| {
        for _ in 0..threads {
            s.spawn(|| {
                let mut bus = TestBus {
                    ram: vec![0; 1 << 24],
                    touched: Vec::new(),
                    writes: HashSet::new(),
                };
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(path) = files.get(i) else { break };
                    let name = path.file_name().unwrap().to_string_lossy().into_owned();
                    let mut data = Vec::new();
                    flate2::read::GzDecoder::new(std::fs::File::open(path).unwrap())
                        .read_to_end(&mut data)
                        .unwrap();
                    let tests = parse_moo(&data);
                    let mask = masks.get(&opcode_key(&name)).copied().unwrap_or(0xFFFF);
                    let (mut failed, mut details) = (0, Vec::new());
                    for t in &tests {
                        let base = name.split(".MOO").next().unwrap().to_uppercase();
                        if revoked.contains(&t.hash) || known.contains(&format!("{base} {}", t.idx))
                        {
                            continue;
                        }
                        let errs = run_test(t, &mut bus, mask);
                        if !errs.is_empty() {
                            failed += 1;
                            if details.len() < verbose {
                                let bytes: Vec<String> =
                                    t.bytes.iter().map(|b| format!("{b:02X}")).collect();
                                let init: Vec<String> = [2, 3, 4, 5, 6, 7, 8, 9, 17]
                                    .iter()
                                    .map(|&i| {
                                        format!(
                                            "{}={:08X}",
                                            REG_NAMES[i],
                                            t.init.regs[i].unwrap_or(0)
                                        )
                                    })
                                    .collect();
                                details.push(format!(
                                    "  #{} {} [{}]{}\n    init: {}\n    {}",
                                    t.idx,
                                    t.name,
                                    bytes.join(" "),
                                    t.exception.map_or(String::new(), |(n, _)| format!(
                                        " (exception {n})"
                                    )),
                                    init.join(" "),
                                    errs.join("\n    ")
                                ));
                            }
                        }
                    }
                    report.lock().unwrap().push((name, tests.len(), failed, details));
                }
            });
        }
    });

    let mut report = report.into_inner().unwrap();
    report.sort_by(|a, b| a.0.cmp(&b.0));
    let total: usize = report.iter().map(|r| r.1).sum();
    let failed: usize = report.iter().map(|r| r.2).sum();
    let bad_files: Vec<_> = report.iter().filter(|r| r.2 > 0).collect();
    for (name, n, f, details) in &bad_files {
        println!("{name}: {f}/{n} failed");
        for d in details {
            println!("{d}");
        }
    }
    println!(
        "\n{} files, {} tests, {} failed ({:.3}% pass), {} files with failures",
        report.len(),
        total,
        failed,
        100.0 * (total - failed) as f64 / total.max(1) as f64,
        bad_files.len()
    );
    assert_eq!(failed, 0, "{failed} SingleStepTests failed");
}
