//! FAT12/FAT16 filesystem on an in-memory disk image (8.3 names, no long
//! file names). Used by the frontend to build TinyDOS disks and by the drive
//! manager; it does no host I/O.
//!
//! Disks have no partition table: sector 0 is the boot sector with the BPB
//! (a "superfloppy" layout, also used for hard disk images).

use std::fmt;

pub const SECTOR: usize = 512;
const DIRENT: usize = 32;

pub const ATTR_READONLY: u8 = 0x01;
pub const ATTR_HIDDEN: u8 = 0x02;
pub const ATTR_SYSTEM: u8 = 0x04;
pub const ATTR_VOLUME: u8 = 0x08;
pub const ATTR_DIR: u8 = 0x10;
pub const ATTR_ARCHIVE: u8 = 0x20;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FatError {
    NotFat,
    NotFound(String),
    Exists(String),
    NotADirectory(String),
    IsADirectory(String),
    DirectoryNotEmpty(String),
    BadName(String),
    DiskFull,
    RootFull,
}

impl fmt::Display for FatError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            FatError::NotFat => write!(f, "not a FAT disk"),
            FatError::NotFound(p) => write!(f, "{p}: not found"),
            FatError::Exists(p) => write!(f, "{p}: already exists"),
            FatError::NotADirectory(p) => write!(f, "{p}: not a directory"),
            FatError::IsADirectory(p) => write!(f, "{p}: is a directory"),
            FatError::DirectoryNotEmpty(p) => write!(f, "{p}: directory not empty"),
            FatError::BadName(p) => write!(f, "{p}: not a valid 8.3 name"),
            FatError::DiskFull => write!(f, "disk full"),
            FatError::RootFull => write!(f, "root directory full"),
        }
    }
}

impl std::error::Error for FatError {}

type R<T> = Result<T, FatError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirEntry {
    /// "NAME.EXT"
    pub name: String,
    pub attr: u8,
    pub size: u32,
    pub cluster: u16,
    /// DOS packed date and time.
    pub date: u16,
    pub time: u16,
}

impl DirEntry {
    pub fn is_dir(&self) -> bool {
        self.attr & ATTR_DIR != 0
    }
}

/// Layout parameters for formatting.
#[derive(Debug, Clone, Copy)]
pub struct Geometry {
    pub total_sectors: u32,
    pub sectors_per_cluster: u8,
    pub reserved_sectors: u16,
    pub root_entries: u16,
    pub fat_sectors: u16,
    pub media: u8,
    pub sectors_per_track: u16,
    pub heads: u16,
    pub fat16: bool,
}

impl Geometry {
    /// Pick a layout for a disk of `bytes` with `reserved` reserved sectors.
    pub fn for_size(bytes: usize, reserved: u16) -> Geometry {
        let total = (bytes / SECTOR) as u32;
        let floppy = total <= 5760;
        let (spt, heads) = match total {
            2880 => (18, 2),
            1440 => (9, 2),
            2400 => (15, 2),
            5760 => (36, 2),
            _ => (63, 16),
        };
        let root_entries: u16 = if floppy { 224 } else { 512 };
        let root_sectors = (root_entries as u32 * 32).div_ceil(SECTOR as u32);
        let mut spc: u32 = 1;
        loop {
            let data = total.saturating_sub(reserved as u32 + root_sectors);
            let clusters = data / spc;
            if clusters < 65_525 || spc >= 128 {
                let fat16 = clusters >= 4085;
                let bytes_per_fat = if fat16 { (clusters + 2) * 2 } else { (clusters + 2) * 3 / 2 + 1 };
                let fat_sectors = bytes_per_fat.div_ceil(SECTOR as u32) as u16;
                return Geometry {
                    total_sectors: total,
                    sectors_per_cluster: spc as u8,
                    reserved_sectors: reserved,
                    root_entries,
                    fat_sectors,
                    media: if floppy { 0xF0 } else { 0xF8 },
                    sectors_per_track: spt,
                    heads,
                    fat16,
                };
            }
            spc *= 2;
        }
    }
}

/// Write a fresh FAT filesystem. `boot` supplies the boot sector code (its
/// BPB area, bytes 3..62, is overwritten). The two FAT copies are cleared.
pub fn format(disk: &mut [u8], boot: &[u8], g: &Geometry, label: &str) {
    disk.fill(0);
    let b = &mut disk[..SECTOR];
    b[..boot.len().min(SECTOR)].copy_from_slice(&boot[..boot.len().min(SECTOR)]);
    if boot.is_empty() {
        b[0] = 0xEB;
        b[1] = 0x3C;
        b[2] = 0x90;
        b[510] = 0x55;
        b[511] = 0xAA;
    }
    b[3..11].copy_from_slice(b"TINYDOS ");
    b[11..13].copy_from_slice(&(SECTOR as u16).to_le_bytes());
    b[13] = g.sectors_per_cluster;
    b[14..16].copy_from_slice(&g.reserved_sectors.to_le_bytes());
    b[16] = 2;
    b[17..19].copy_from_slice(&g.root_entries.to_le_bytes());
    let (t16, t32) = if g.total_sectors < 65536 { (g.total_sectors as u16, 0) } else { (0, g.total_sectors) };
    b[19..21].copy_from_slice(&t16.to_le_bytes());
    b[21] = g.media;
    b[22..24].copy_from_slice(&g.fat_sectors.to_le_bytes());
    b[24..26].copy_from_slice(&g.sectors_per_track.to_le_bytes());
    b[26..28].copy_from_slice(&g.heads.to_le_bytes());
    b[28..32].copy_from_slice(&0u32.to_le_bytes());
    b[32..36].copy_from_slice(&t32.to_le_bytes());
    b[36] = if g.media == 0xF0 { 0x00 } else { 0x80 };
    b[38] = 0x29;
    b[39..43].copy_from_slice(&0x1986_0486u32.to_le_bytes());
    let mut lab = [b' '; 11];
    for (i, c) in label.bytes().take(11).enumerate() {
        lab[i] = c.to_ascii_uppercase();
    }
    b[43..54].copy_from_slice(&lab);
    b[54..62].copy_from_slice(if g.fat16 { b"FAT16   " } else { b"FAT12   " });
    if disk[510] != 0x55 {
        disk[510] = 0x55;
        disk[511] = 0xAA;
    }
    let mut fs = Fs::open(disk).expect("just formatted");
    fs.set_fat(0, 0xFF00 | g.media as u16);
    fs.set_fat(1, 0xFFFF);
    if !label.is_empty() {
        let mut e = [0u8; DIRENT];
        e[..11].copy_from_slice(&lab);
        e[11] = ATTR_VOLUME;
        let off = fs.root_start * SECTOR;
        fs.d[off..off + DIRENT].copy_from_slice(&e);
    }
}

/// A mounted FAT filesystem over a disk image.
pub struct Fs<'a> {
    d: &'a mut [u8],
    spc: usize,
    reserved: usize,
    nfats: usize,
    root_entries: usize,
    fat_sectors: usize,
    fat16: bool,
    root_start: usize,
    data_start: usize,
    clusters: usize,
    /// DOS date/time stamped on new and modified entries.
    pub stamp: (u16, u16),
}

/// A directory: the fixed root or a cluster chain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Dir {
    Root,
    Chain(u16),
}

/// Location of a 32-byte directory entry in the image.
type Slot = usize;

fn rd16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}

fn rd32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Convert "name.ext" to the 11-byte directory form.
pub fn to_83(name: &str) -> R<[u8; 11]> {
    let bad = || FatError::BadName(name.to_string());
    if name == "." || name == ".." {
        let mut out = [b' '; 11];
        out[..name.len()].copy_from_slice(name.as_bytes());
        return Ok(out);
    }
    let (base, ext) = match name.rsplit_once('.') {
        Some((b, e)) => (b, e),
        None => (name, ""),
    };
    if base.is_empty() || base.len() > 8 || ext.len() > 3 {
        return Err(bad());
    }
    let mut out = [b' '; 11];
    for (i, c) in base.bytes().enumerate() {
        if !valid_char(c) {
            return Err(bad());
        }
        out[i] = c.to_ascii_uppercase();
    }
    for (i, c) in ext.bytes().enumerate() {
        if !valid_char(c) {
            return Err(bad());
        }
        out[8 + i] = c.to_ascii_uppercase();
    }
    Ok(out)
}

fn valid_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"!#$%&'()-@^_`{}~".contains(&c)
}

fn from_83(raw: &[u8]) -> String {
    let base = String::from_utf8_lossy(&raw[..8]).trim_end().to_string();
    let ext = String::from_utf8_lossy(&raw[8..11]).trim_end().to_string();
    if ext.is_empty() {
        base
    } else {
        format!("{base}.{ext}")
    }
}

fn split_path(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|p| !p.is_empty())
        .map(|p| p.to_ascii_uppercase())
        .collect()
}

impl<'a> Fs<'a> {
    pub fn open(d: &'a mut [u8]) -> R<Fs<'a>> {
        if d.len() < SECTOR {
            return Err(FatError::NotFat);
        }
        let bps = rd16(d, 11) as usize;
        let spc = d[13] as usize;
        let reserved = rd16(d, 14) as usize;
        let nfats = d[16] as usize;
        let root_entries = rd16(d, 17) as usize;
        let total16 = rd16(d, 19) as usize;
        let fat_sectors = rd16(d, 22) as usize;
        let total = if total16 != 0 { total16 } else { rd32(d, 32) as usize };
        if bps != SECTOR || spc == 0 || !spc.is_power_of_two() || nfats == 0 || fat_sectors == 0 || reserved == 0 {
            return Err(FatError::NotFat);
        }
        let root_start = reserved + nfats * fat_sectors;
        let data_start = root_start + (root_entries * DIRENT).div_ceil(SECTOR);
        if total > d.len() / SECTOR || data_start >= total {
            return Err(FatError::NotFat);
        }
        let clusters = (total - data_start) / spc;
        Ok(Fs {
            d,
            spc,
            reserved,
            nfats,
            root_entries,
            fat_sectors,
            fat16: clusters >= 4085,
            root_start,
            data_start,
            clusters,
            stamp: (0x5D67, 0x6000), // 2026-11-07 12:00
        })
    }

    pub fn cluster_bytes(&self) -> usize {
        self.spc * SECTOR
    }

    pub fn free_bytes(&self) -> usize {
        (2..self.clusters + 2).filter(|&c| self.fat(c as u16) == 0).count() * self.cluster_bytes()
    }

    // -------------------------------------------------------------- FAT

    fn fat(&self, c: u16) -> u16 {
        let base = self.reserved * SECTOR;
        let c = c as usize;
        if self.fat16 {
            rd16(self.d, base + c * 2)
        } else {
            let v = rd16(self.d, base + c * 3 / 2);
            if c & 1 == 0 {
                v & 0xFFF
            } else {
                v >> 4
            }
        }
    }

    fn set_fat(&mut self, c: u16, v: u16) {
        for copy in 0..self.nfats {
            let base = (self.reserved + copy * self.fat_sectors) * SECTOR;
            let c = c as usize;
            if self.fat16 {
                self.d[base + c * 2..base + c * 2 + 2].copy_from_slice(&v.to_le_bytes());
            } else {
                let o = base + c * 3 / 2;
                let old = rd16(self.d, o);
                let new = if c & 1 == 0 { (old & 0xF000) | (v & 0xFFF) } else { (old & 0x000F) | (v << 4) };
                self.d[o..o + 2].copy_from_slice(&new.to_le_bytes());
            }
        }
    }

    fn is_eoc(&self, v: u16) -> bool {
        if self.fat16 {
            v >= 0xFFF8
        } else {
            v >= 0xFF8
        }
    }

    fn eoc(&self) -> u16 {
        if self.fat16 {
            0xFFFF
        } else {
            0xFFF
        }
    }

    fn chain(&self, start: u16) -> Vec<u16> {
        let mut out = Vec::new();
        let mut c = start;
        while c >= 2 && (c as usize) < self.clusters + 2 && out.len() <= self.clusters {
            out.push(c);
            let n = self.fat(c);
            if self.is_eoc(n) || n < 2 {
                break;
            }
            c = n;
        }
        out
    }

    fn alloc(&mut self, prev: Option<u16>) -> R<u16> {
        let c = (2..self.clusters + 2).find(|&c| self.fat(c as u16) == 0).ok_or(FatError::DiskFull)? as u16;
        let eoc = self.eoc();
        self.set_fat(c, eoc);
        if let Some(p) = prev {
            self.set_fat(p, c);
        }
        let off = self.cluster_off(c);
        let n = self.cluster_bytes();
        self.d[off..off + n].fill(0);
        Ok(c)
    }

    fn free_chain(&mut self, start: u16) {
        for c in self.chain(start) {
            self.set_fat(c, 0);
        }
    }

    fn cluster_off(&self, c: u16) -> usize {
        (self.data_start + (c as usize - 2) * self.spc) * SECTOR
    }

    // -------------------------------------------------------------- directories

    fn dir_slots(&self, dir: Dir) -> Vec<Slot> {
        match dir {
            Dir::Root => (0..self.root_entries).map(|i| self.root_start * SECTOR + i * DIRENT).collect(),
            Dir::Chain(c) => self
                .chain(c)
                .into_iter()
                .flat_map(|cl| {
                    let off = self.cluster_off(cl);
                    (0..self.cluster_bytes() / DIRENT).map(move |i| off + i * DIRENT)
                })
                .collect(),
        }
    }

    fn entry_at(&self, s: Slot) -> DirEntry {
        let e = &self.d[s..s + DIRENT];
        DirEntry {
            name: from_83(&e[..11]),
            attr: e[11],
            time: rd16(e, 22),
            date: rd16(e, 24),
            cluster: rd16(e, 26),
            size: rd32(e, 28),
        }
    }

    fn entries(&self, dir: Dir) -> Vec<(Slot, DirEntry)> {
        let mut out = Vec::new();
        for s in self.dir_slots(dir) {
            match self.d[s] {
                0x00 => break,
                0xE5 => continue,
                _ => {}
            }
            let e = self.entry_at(s);
            if e.attr & ATTR_VOLUME != 0 || e.attr == 0x0F {
                continue;
            }
            out.push((s, e));
        }
        out
    }

    fn find(&self, dir: Dir, name: &str) -> R<Option<(Slot, DirEntry)>> {
        let want = to_83(name)?;
        Ok(self.entries(dir).into_iter().find(|(s, _)| self.d[*s..*s + 11] == want))
    }

    /// Resolve a directory path.
    fn dir(&self, path: &str) -> R<Dir> {
        let mut d = Dir::Root;
        for part in split_path(path) {
            match self.find(d, &part)? {
                Some((_, e)) if e.is_dir() => d = if e.cluster == 0 { Dir::Root } else { Dir::Chain(e.cluster) },
                Some(_) => return Err(FatError::NotADirectory(path.into())),
                None => return Err(FatError::NotFound(path.into())),
            }
        }
        Ok(d)
    }

    /// Split "A/B/FILE.TXT" into (parent dir, "FILE.TXT").
    fn parent(&self, path: &str) -> R<(Dir, String)> {
        let mut parts = split_path(path);
        let name = parts.pop().ok_or_else(|| FatError::BadName(path.into()))?;
        Ok((self.dir(&parts.join("/"))?, name))
    }

    fn free_slot(&mut self, dir: Dir) -> R<Slot> {
        if let Some(s) = self.dir_slots(dir).into_iter().find(|&s| self.d[s] == 0 || self.d[s] == 0xE5) {
            return Ok(s);
        }
        match dir {
            Dir::Root => Err(FatError::RootFull),
            Dir::Chain(c) => {
                let last = *self.chain(c).last().unwrap();
                let n = self.alloc(Some(last))?;
                Ok(self.cluster_off(n))
            }
        }
    }

    fn write_entry(&mut self, s: Slot, name: &[u8; 11], attr: u8, cluster: u16, size: u32) {
        let (date, time) = self.stamp;
        let e = &mut self.d[s..s + DIRENT];
        e.fill(0);
        e[..11].copy_from_slice(name);
        e[11] = attr;
        e[22..24].copy_from_slice(&time.to_le_bytes());
        e[24..26].copy_from_slice(&date.to_le_bytes());
        e[26..28].copy_from_slice(&cluster.to_le_bytes());
        e[28..32].copy_from_slice(&size.to_le_bytes());
    }

    // -------------------------------------------------------------- public API

    /// List a directory ("" or "/" for the root).
    pub fn list(&self, path: &str) -> R<Vec<DirEntry>> {
        let d = self.dir(path)?;
        Ok(self.entries(d).into_iter().map(|(_, e)| e).collect())
    }

    pub fn stat(&self, path: &str) -> R<DirEntry> {
        let (d, name) = self.parent(path)?;
        self.find(d, &name)?.map(|(_, e)| e).ok_or_else(|| FatError::NotFound(path.into()))
    }

    pub fn exists(&self, path: &str) -> bool {
        self.stat(path).is_ok()
    }

    pub fn read(&self, path: &str) -> R<Vec<u8>> {
        let e = self.stat(path)?;
        if e.is_dir() {
            return Err(FatError::IsADirectory(path.into()));
        }
        let mut out = Vec::with_capacity(e.size as usize);
        for c in self.chain(e.cluster) {
            let off = self.cluster_off(c);
            out.extend_from_slice(&self.d[off..off + self.cluster_bytes()]);
            if out.len() >= e.size as usize {
                break;
            }
        }
        out.truncate(e.size as usize);
        Ok(out)
    }

    /// Create or replace a file.
    pub fn write(&mut self, path: &str, data: &[u8]) -> R<()> {
        self.write_attr(path, data, ATTR_ARCHIVE)
    }

    pub fn write_attr(&mut self, path: &str, data: &[u8], attr: u8) -> R<()> {
        let (d, name) = self.parent(path)?;
        let raw = to_83(&name)?;
        let slot = match self.find(d, &name)? {
            Some((s, e)) => {
                if e.is_dir() {
                    return Err(FatError::IsADirectory(path.into()));
                }
                if e.cluster >= 2 {
                    self.free_chain(e.cluster);
                }
                s
            }
            None => self.free_slot(d)?,
        };
        // Check space before allocating.
        let need = data.len().div_ceil(self.cluster_bytes());
        if need * self.cluster_bytes() > self.free_bytes() {
            self.d[slot] = 0xE5;
            return Err(FatError::DiskFull);
        }
        let mut first = 0u16;
        let mut prev = None;
        for chunk in data.chunks(self.cluster_bytes()) {
            let c = self.alloc(prev)?;
            if first == 0 {
                first = c;
            }
            let off = self.cluster_off(c);
            self.d[off..off + chunk.len()].copy_from_slice(chunk);
            prev = Some(c);
        }
        self.write_entry(slot, &raw, attr, first, data.len() as u32);
        Ok(())
    }

    pub fn mkdir(&mut self, path: &str) -> R<()> {
        let (d, name) = self.parent(path)?;
        if self.find(d, &name)?.is_some() {
            return Err(FatError::Exists(path.into()));
        }
        let raw = to_83(&name)?;
        let slot = self.free_slot(d)?;
        let c = self.alloc(None)?;
        let parent_cluster = match d {
            Dir::Root => 0,
            Dir::Chain(p) => p,
        };
        let off = self.cluster_off(c);
        self.write_entry(off, &to_83(".")?, ATTR_DIR, c, 0);
        self.write_entry(off + DIRENT, &to_83("..")?, ATTR_DIR, parent_cluster, 0);
        self.write_entry(slot, &raw, ATTR_DIR, c, 0);
        Ok(())
    }

    /// Delete a file or an empty directory.
    pub fn remove(&mut self, path: &str) -> R<()> {
        let (d, name) = self.parent(path)?;
        let (slot, e) = self.find(d, &name)?.ok_or_else(|| FatError::NotFound(path.into()))?;
        if e.is_dir() && self.entries(Dir::Chain(e.cluster)).iter().any(|(_, x)| x.name != "." && x.name != "..") {
            return Err(FatError::DirectoryNotEmpty(path.into()));
        }
        if e.cluster >= 2 {
            self.free_chain(e.cluster);
        }
        self.d[slot] = 0xE5;
        Ok(())
    }

    /// Write `data` to `path`, creating parent directories as needed.
    pub fn write_creating_dirs(&mut self, path: &str, data: &[u8]) -> R<()> {
        let parts = split_path(path);
        for i in 1..parts.len() {
            let dir = parts[..i].join("/");
            if !self.exists(&dir) {
                self.mkdir(&dir)?;
            }
        }
        self.write(path, data)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn floppy() -> Vec<u8> {
        let mut d = vec![0u8; 1_474_560];
        let g = Geometry::for_size(d.len(), 1);
        format(&mut d, &[], &g, "TEST");
        d
    }

    #[test]
    fn geometry() {
        let g = Geometry::for_size(1_474_560, 1);
        assert_eq!((g.sectors_per_cluster, g.fat_sectors, g.root_entries, g.fat16), (1, 9, 224, false));
        let h = Geometry::for_size(64 << 20, 1);
        assert!(h.fat16);
    }

    #[test]
    fn files_and_dirs() {
        let mut d = floppy();
        let mut fs = Fs::open(&mut d).unwrap();
        let big: Vec<u8> = (0..5000u32).map(|i| i as u8).collect();
        fs.write("HELLO.TXT", b"hi there").unwrap();
        fs.mkdir("GAMES").unwrap();
        fs.write("games/big.dat", &big).unwrap();
        assert_eq!(fs.read("hello.txt").unwrap(), b"hi there");
        assert_eq!(fs.read("GAMES\\BIG.DAT").unwrap(), big);
        let root: Vec<String> = fs.list("/").unwrap().into_iter().map(|e| e.name).collect();
        assert_eq!(root, vec!["HELLO.TXT", "GAMES"]);
        let free = fs.free_bytes();
        fs.write("GAMES/BIG.DAT", b"small").unwrap();
        assert_eq!(fs.free_bytes(), free + 9 * 512);
        assert!(fs.remove("GAMES").is_err());
        fs.remove("GAMES/BIG.DAT").unwrap();
        fs.remove("GAMES").unwrap();
        assert!(!fs.exists("GAMES"));
        assert!(matches!(fs.write("toolongname.txt", b""), Err(FatError::BadName(_))));
    }

    #[test]
    fn fat16_and_dir_growth() {
        let mut d = vec![0u8; 32 << 20];
        let g = Geometry::for_size(d.len(), 64);
        format(&mut d, &[], &g, "HD");
        let mut fs = Fs::open(&mut d).unwrap();
        assert!(fs.fat16);
        fs.mkdir("SUB").unwrap();
        for i in 0..100 {
            fs.write(&format!("SUB/F{i}.TXT"), format!("file {i}").as_bytes()).unwrap();
        }
        assert_eq!(fs.list("SUB").unwrap().len(), 102);
        assert_eq!(fs.read("SUB/F77.TXT").unwrap(), b"file 77");
    }
}
