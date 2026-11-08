// ANTS.RS - Ant Colony 2.0, an ant farm for rusty486.
//
// A side view: sky and a scrolling surface where food lies, and the earth
// below where the colony digs its nest. 640x480 in 256 colours (VBE mode
// 101h with the linear framebuffer at E0000000h), mouse and keyboard.
//
// The world is 1920x400 pixels: a 480x100 grid of 4x4-pixel cells. Video
// memory is set up 1920 pixels wide with two pages; the screen pans over
// it with the VBE display start, so scrolling costs nothing. The static
// picture is kept in BG and drawn on both pages; moving things are drawn
// on the hidden page, which is shown at the vertical retrace.
//
// Ants find their way with three distance maps over the cells they can
// walk (tunnels, and air next to earth): to the queen's chamber, to the
// surface, and to earth marked for digging. On the surface, foragers
// follow the scent trails that food carriers leave on the way home.

const SW: i32 = 640;
const SH: i32 = 480;
const WW: i32 = 1920;
const PH: i32 = 400;
const GW: i32 = 480;
const GH: i32 = 100;
const NC: usize = 48000;
const PAGE_BYTES: usize = 921600;

const AIR: u8 = 0;
const DIRT: u8 = 1;
const TUNNEL: u8 = 2;
const ROCK: u8 = 3;

static mut BG: [u8; 768000] = [0; 768000];
static mut PANEL: [u8; 51200] = [0; 51200];
static mut TER: [u8; 48000] = [0; 48000];
static mut FOOD: [u8; 48000] = [0; 48000];
static mut TRAIL: [u8; 48000] = [0; 48000];
static mut MARK: [u8; 48000] = [0; 48000];
static mut WALK: [u8; 48000] = [0; 48000];
static mut HOMEC: [u8; 48000] = [0; 48000];
static mut CLOUD: [u8; 48000] = [0; 48000];
static mut SHOWN: [u8; 48000] = [0; 48000];
static mut HOMED: [u16; 48000] = [0; 48000];
static mut UPD: [u16; 48000] = [0; 48000];
static mut DIGD: [u16; 48000] = [0; 48000];
static mut QUEUE: [u32; 48000] = [0; 48000];
static mut GROUND: [i32; 480] = [0; 480];

static mut FONT: u32 = 0;
static mut NZ: [u8; 4096] = [0; 4096];
// 64x64 textures: earth in four depth bands, tunnel, rock.
static mut TEX: [u8; 24576] = [0; 24576];
static mut WLIST: [u32; 48000] = [0; 48000];
static mut NWALK: usize = 0;

// The nest: entrance column, ground row there, chamber centre (cells).
static mut EC: i32 = 240;
static mut EG: i32 = 25;
static mut QX: i32 = 0;
static mut QY: i32 = 0;

// ===================================================== video

static mut PAGE: usize = 1;
static mut VX: i32 = 640;
static mut PVX: [i32; 2] = [-1; 2];
static mut PDIRTY: [bool; 2] = [true; 2];

fn vram() -> *mut u8 {
    let base: u32 = 0xE0000000;
    base as *mut u8
}

// The hidden page.
fn lfb() -> *mut u8 {
    unsafe { vram().add(PAGE * PAGE_BYTES) }
}

fn set_vbe_mode() -> bool {
    let mut r = Regs::new();
    r.eax = 0x4F02;
    r.ebx = 0x4101; // 640x480x256 with the linear framebuffer
    dos_int(0x10, &mut r);
    if r.eax & 0xFFFF != 0x004F {
        return false;
    }
    // Lines 1920 pixels long: the whole world side by side.
    let mut s = Regs::new();
    s.eax = 0x4F06;
    s.ebx = 0;
    s.ecx = 1920;
    dos_int(0x10, &mut s);
    s.ecx & 0xFFFF == 1920
}

// Show the page just drawn at the current scroll position.
fn flip() {
    unsafe {
        let mut r = Regs::new();
        r.eax = 0x4F07;
        r.ebx = 0;
        r.ecx = VX as u32;
        r.edx = (PAGE * 480) as u32;
        dos_int(0x10, &mut r);
        PAGE = 1 - PAGE;
    }
}

fn wait_retrace() {
    while inb(0x3DA) & 8 != 0 {}
    while inb(0x3DA) & 8 == 0 {}
}

fn pal(i: u32, r: u32, g: u32, b: u32) {
    outb(0x3C8, i as u8);
    outb(0x3C9, r as u8);
    outb(0x3C9, g as u8);
    outb(0x3C9, b as u8);
}

const C_SKY: u32 = 16;
const C_CLOUD: u32 = 24;
const C_SCENT: u32 = 26;
const C_DIRT: u32 = 32;
const C_MARK: u32 = 48;
const C_TUNNEL: u32 = 52;
const C_ROCK: u32 = 56;
const C_GRASS: u32 = 60;
const C_FOOD: u32 = 64;
const C_ANT: u8 = 80;
const C_ANT_HI: u8 = 81;
const C_SOLDIER: u8 = 82;
const C_SOLDIER_HI: u8 = 83;
const C_CRUMB: u8 = 84;
const C_SPIDER: u8 = 85;
const C_SPIDER_LEG: u8 = 86;
const C_EYE: u8 = 87;
const C_QUEEN: u8 = 88;
const C_QUEEN_HI: u8 = 89;
const C_PANEL: u8 = 90;
const C_PANEL_HI: u8 = 91;
const C_TEXT: u8 = 92;
const C_YELLOW: u8 = 93;
const C_BUTTON: u8 = 94;
const C_BUTTON_ON: u8 = 95;
const C_RED: u8 = 96;
const C_GREEN: u8 = 97;
const C_WHITE: u8 = 98;
const C_BLACK: u8 = 99;
const C_PANEL_LO: u8 = 100;
const C_DIM: u8 = 101;
const C_EGG: u8 = 102;
const C_SOIL: u8 = 103;

fn palette() {
    pal(0, 0, 0, 0);
    for k in 0..8 {
        // Sky: deep blue at the top, pale at the horizon.
        pal(C_SKY + k, 14 + k * 4, 26 + k * 4, 50 + k);
    }
    pal(C_CLOUD, 60, 61, 63);
    pal(C_CLOUD + 1, 54, 56, 60);
    pal(C_SCENT, 50, 30, 58);
    pal(C_SCENT + 1, 58, 36, 63);
    for b in 0..4 {
        for s in 0..4 {
            // Earth, darker with depth.
            pal(C_DIRT + b * 4 + s, 38 - b * 6 + s * 2, 25 - b * 4 + s, 12 - b * 2 + s);
        }
    }
    for s in 0..4 {
        pal(C_MARK + s, 52 + s * 2, 36 + s * 2, 16 + s);
        pal(C_TUNNEL + s, 12 + s, 7 + s, 3 + s);
        pal(C_ROCK + s, 26 + s * 5, 26 + s * 5, 28 + s * 5);
        pal(C_GRASS + s, 10 + s * 3, 34 + s * 5, 8 + s * 2);
        pal(C_FOOD + s, 63 - s * 2, 20 + s * 12, 8 + s * 2);
    }
    pal(C_ANT as u32, 3, 2, 1);
    pal(C_ANT_HI as u32, 34, 18, 8);
    pal(C_SOLDIER as u32, 42, 8, 4);
    pal(C_SOLDIER_HI as u32, 63, 24, 12);
    pal(C_CRUMB as u32, 63, 56, 16);
    pal(C_SPIDER as u32, 18, 8, 24);
    pal(C_SPIDER_LEG as u32, 6, 2, 8);
    pal(C_EYE as u32, 63, 10, 10);
    pal(C_QUEEN as u32, 38, 20, 6);
    pal(C_QUEEN_HI as u32, 60, 40, 16);
    pal(C_PANEL as u32, 10, 11, 18);
    pal(C_PANEL_HI as u32, 30, 32, 44);
    pal(C_TEXT as u32, 52, 54, 58);
    pal(C_YELLOW as u32, 63, 54, 18);
    pal(C_BUTTON as u32, 18, 20, 30);
    pal(C_BUTTON_ON as u32, 12, 32, 50);
    pal(C_RED as u32, 63, 24, 20);
    pal(C_GREEN as u32, 30, 58, 26);
    pal(C_WHITE as u32, 63, 63, 63);
    pal(C_BLACK as u32, 0, 0, 0);
    pal(C_PANEL_LO as u32, 4, 4, 8);
    pal(C_DIM as u32, 30, 31, 36);
    pal(C_EGG as u32, 62, 60, 52);
    pal(C_SOIL as u32, 44, 30, 14);
}

fn make_noise() {
    for y in 0..64 {
        for x in 0..64 {
            let h = (x as u32) * 374761393 ^ (y as u32) * 668265263;
            let h2 = (h ^ (h >> 13)) * 1274126177;
            let n = (h2 >> 16) & 3;
            let o = (y * 64 + x) as usize;
            unsafe {
                NZ[o] = n as u8;
                for b in 0..4 {
                    TEX[b * 4096 + o] = (C_DIRT + b as u32 * 4 + n) as u8;
                }
                TEX[4 * 4096 + o] = (C_TUNNEL + (n & 1)) as u8;
                TEX[5 * 4096 + o] = (C_ROCK + n) as u8;
            }
        }
    }
}

// Which texture a plain cell uses (or 255: sky, or 254: paint it alone).
fn plain_kind(i: usize) -> u32 {
    unsafe {
        let gy = i as i32 / GW;
        match TER[i] {
            AIR => {
                if FOOD[i] == 0 && CLOUD[i] == 0 && TRAIL[i] < 32 { 255 } else { 254 }
            }
            DIRT => {
                if MARK[i] != 0 || grassy(i) {
                    254
                } else if gy < 30 {
                    0
                } else if gy < 50 {
                    1
                } else if gy < 72 {
                    2
                } else {
                    3
                }
            }
            TUNNEL => 4,
            _ => 5,
        }
    }
}

// Paint the whole world into BG quickly: runs of plain cells are copied
// from the textures a row at a time; the rest is painted cell by cell.
fn paint_world() {
    unsafe {
        for i in 0..NC {
            if plain_kind(i) == 254 {
                paint_cell(i, false);
            } else {
                SHOWN[i] = cell_look(i);
            }
        }
        for gy in 0..GH {
            let mut gx: i32 = 0;
            while gx < GW {
                let k = plain_kind(idx(gx, gy));
                if k == 254 {
                    gx += 1;
                    continue;
                }
                let mut end = gx + 1;
                while end < GW && plain_kind(idx(end, gy)) == k {
                    end += 1;
                }
                for py in 0..4 {
                    let y = gy * 4 + py;
                    let row = (y * WW) as usize;
                    if k == 255 {
                        let sky = C_SKY + if y < 112 { (y / 14) as u32 } else { 7 };
                        fill_bytes(BG.as_mut_ptr().add(row + (gx * 4) as usize), sky as u8, ((end - gx) * 4) as usize);
                    } else {
                        let t = (k * 4096 + ((y & 63) as u32) * 64) as usize;
                        let mut x = gx * 4;
                        while x < end * 4 {
                            let tx = x & 63;
                            let mut n = 64 - tx;
                            if x + n > end * 4 {
                                n = end * 4 - x;
                            }
                            copy_bytes(BG.as_mut_ptr().add(row + x as usize), TEX.as_ptr().add(t + tx as usize), n as usize);
                            x += n;
                        }
                    }
                }
                gx = end;
            }
        }
    }
}

// ===================================================== the panel (640x80)

fn ppset(x: i32, y: i32, c: u8) {
    if x >= 0 && x < SW && y >= 0 && y < 80 {
        unsafe { PANEL[(y * SW + x) as usize] = c; }
    }
}

fn pfill(x: i32, y: i32, w: i32, h: i32, c: u8) {
    for yy in y..y + h {
        for xx in x..x + w {
            ppset(xx, yy, c);
        }
    }
}

fn panel_changed() {
    unsafe {
        PDIRTY[0] = true;
        PDIRTY[1] = true;
    }
}

fn load_font() {
    let mut r = Regs::new();
    r.eax = 0x1130;
    r.ebx = 0x0300;
    dos_int(0x10, &mut r);
    unsafe { FONT = (r.es as u32) * 16 + (r.ebp & 0xFFFF); }
}

// Text on the panel; returns the x after it.
fn ptext(x: i32, y: i32, s: &str, fg: u8, bg: u8) -> i32 {
    let mut cx = x;
    for c in s.bytes() {
        let glyph = unsafe { (FONT + c as u32 * 8) as *const u8 };
        for row in 0..8 {
            let bits = unsafe { *glyph.add(row as usize) };
            for col in 0..8 {
                ppset(cx + col, y + row, if bits & (1 << col) != 0 { fg } else { bg });
            }
        }
        cx += 8;
    }
    cx
}

// Copy the panel to the bottom of the hidden page, under the view.
fn blit_panel() {
    unsafe {
        if !PDIRTY[PAGE] && PVX[PAGE] == VX {
            return;
        }
        for r in 0..80 {
            let o = ((PH + r) * WW + VX) as usize;
            copy_bytes(lfb().add(o), PANEL.as_ptr().add((r * SW) as usize), 640);
        }
        PDIRTY[PAGE] = false;
        PVX[PAGE] = VX;
    }
}

static mut NB: [u8; 12] = [0; 12];

fn num(v: u32) -> &'static str {
    unsafe {
        let mut d = [0u8; 12];
        let mut n = 0;
        let mut x = v;
        loop {
            d[n] = b'0' + (x % 10) as u8;
            n += 1;
            x /= 10;
            if x == 0 {
                break;
            }
        }
        for i in 0..n {
            NB[i] = d[n - 1 - i];
        }
        str_from_bytes(&NB[..n])
    }
}

// ===================================================== moving things

// A pixel on the hidden page, in world coordinates, inside the world.
fn spot(x: i32, y: i32, c: u8) {
    if x >= 0 && x < WW && y >= 0 && y < PH {
        unsafe { *lfb().add((y * WW + x) as usize) = c; }
    }
}

// Put the static picture back over a rectangle of the hidden page.
fn restore(x: i32, y: i32, w: i32, h: i32) {
    unsafe {
        let mut x0 = x;
        let mut x1 = x + w;
        if x0 < 0 {
            x0 = 0;
        }
        if x1 > WW {
            x1 = WW;
        }
        if x0 >= x1 {
            return;
        }
        let mut y0 = y;
        if y0 < 0 {
            y0 = 0;
        }
        let mut y1 = y + h;
        if y1 > SH {
            y1 = SH;
        }
        for yy in y0..y1 {
            let o = (yy * WW + x0) as usize;
            if yy < PH {
                copy_bytes(lfb().add(o), BG.as_ptr().add(o), (x1 - x0) as usize);
            } else {
                // The panel, as copied to this page.
                let mut a = x0 - PVX[PAGE];
                let mut b = x1 - PVX[PAGE];
                if a < 0 {
                    a = 0;
                }
                if b > SW {
                    b = SW;
                }
                if a < b {
                    copy_bytes(lfb().add((yy * WW + PVX[PAGE] + a) as usize), PANEL.as_ptr().add(((yy - PH) * SW + a) as usize), (b - a) as usize);
                }
            }
        }
    }
}

fn sprite_color(c: u8) -> u8 {
    match c {
        b'a' => C_ANT,
        b'A' => C_ANT_HI,
        b's' => C_SPIDER,
        b'l' => C_SPIDER_LEG,
        b'e' => C_EYE,
        b'q' => C_QUEEN,
        b'Q' => C_QUEEN_HI,
        b'w' => C_WHITE,
        b'k' => C_BLACK,
        _ => 0,
    }
}

// A sprite (rows of characters, '.' transparent) on the hidden page;
// flip mirrors it. The cursor may go over the panel (clip_play false).
fn sprite(x: i32, y: i32, w: i32, h: i32, rows: &str, flip: bool, clip_play: bool) {
    let b = rows.as_bytes();
    for r in 0..h {
        for c in 0..w {
            let sc = if flip { w - 1 - c } else { c };
            let col = sprite_color(b[(r * w + sc) as usize]);
            if col != 0 {
                let px = x + c;
                let py = y + r;
                if clip_play {
                    spot(px, py, col);
                } else if px >= 0 && px < WW && py >= 0 && py < SH {
                    unsafe { *lfb().add((py * WW + px) as usize) = col; }
                }
            }
        }
    }
}

// 11x6, facing right.
fn spider_art() -> &'static str {
    "....sss.......sssss..e.l.ssssssskl.l.sss.l...l.l.l.l.l.l...l.l...l"
}

// 12x6, facing right.
fn queen_art() -> &'static str {
    ".............qqqq....aa.qqQqqqqaaAaaqqqqqqqaaaaa.qqqq..a..a...a.a..a..a."
}

fn cursor_art() -> &'static str {
    "k.......kk......kwk.....kwwk....kwwwk...kwwwwk..kwwwwwk.kwwwwwwkkwwwkkkkkwkwk...kk.kwk..k...kk.."
}

// Text over the world (an outline, then the letters).
fn float_text(x: i32, y: i32, s: &str, fg: u8) {
    let mut cx = x;
    for c in s.bytes() {
        let glyph = unsafe { (FONT + c as u32 * 8) as *const u8 };
        for row in 0..8 {
            let bits = unsafe { *glyph.add(row as usize) };
            for col in 0..8 {
                if bits & (1 << col) != 0 {
                    spot(cx + col + 1, y + row + 1, C_BLACK);
                    spot(cx + col, y + row, fg);
                }
            }
        }
        cx += 8;
    }
}

// ===================================================== the world

fn solid(t: u8) -> bool {
    t == DIRT || t == ROCK
}

fn idx(gx: i32, gy: i32) -> usize {
    (gy * GW + gx) as usize
}

fn inside(gx: i32, gy: i32) -> bool {
    gx >= 0 && gx < GW && gy >= 0 && gy < GH
}

fn dxt(d: i32) -> i32 {
    match d & 7 {
        0 => 1,
        1 => 1,
        2 => 0,
        3 => -1,
        4 => -1,
        5 => -1,
        6 => 0,
        _ => 1,
    }
}

fn dyt(d: i32) -> i32 {
    match d & 7 {
        0 => 0,
        1 => 1,
        2 => 1,
        3 => 1,
        4 => 0,
        5 => -1,
        6 => -1,
        _ => -1,
    }
}

// Earth with air above shows grass.
fn grassy(i: usize) -> bool {
    unsafe { i >= GW as usize && TER[i - GW as usize] == AIR }
}

fn cell_look(i: usize) -> u8 {
    unsafe {
        match TER[i] {
            AIR => {
                if FOOD[i] > 0 {
                    50 + (FOOD[i] >> 6)
                } else {
                    10 + (TRAIL[i] >> 6)
                }
            }
            DIRT => {
                let mut k: u8 = 100;
                if MARK[i] != 0 {
                    k += 2;
                }
                if grassy(i) {
                    k += 1;
                }
                k
            }
            TUNNEL => 200,
            _ => 201,
        }
    }
}

// Draw a cell into BG (and both video pages when on_screen).
fn paint_cell(i: usize, on_screen: bool) {
    unsafe {
        let look = cell_look(i);
        SHOWN[i] = look;
        let gx = i as i32 % GW;
        let gy = i as i32 / GW;
        let x0 = gx * 4;
        let y0 = gy * 4;
        let t = TER[i];
        let band: u32 = if gy < 30 { 0 } else if gy < 50 { 1 } else if gy < 72 { 2 } else { 3 };
        let grass = t == DIRT && grassy(i);
        let food = FOOD[i];
        let mut o = (y0 * WW + x0) as usize;
        for py in 0..4 {
            let y = y0 + py;
            let sky: u32 = C_SKY + if y < 112 { (y / 14) as u32 } else { 7 };
            for px in 0..4 {
                let x = x0 + px;
                let n = NZ[(((y & 63) << 6) | (x & 63)) as usize] as u32;
                let c: u32 = if t == AIR {
                    if food > 0 {
                        // A pile grows up from the ground.
                        let h = (food >> 6) as i32;
                        if py >= 3 - h && (n != 0 || py == 3) { C_FOOD + n } else { sky }
                    } else if CLOUD[i] != 0 {
                        C_CLOUD + (n & 1)
                    } else if TRAIL[i] > 31 && py == 3 && n < 2 {
                        C_SCENT + (TRAIL[i] >> 7) as u32
                    } else {
                        sky
                    }
                } else if t == DIRT {
                    if MARK[i] != 0 {
                        if (x + y) & 3 == 0 { C_MARK + 3 } else { C_MARK + (n & 1) }
                    } else if grass && py < 2 {
                        C_GRASS + n
                    } else {
                        C_DIRT + band * 4 + n
                    }
                } else if t == TUNNEL {
                    C_TUNNEL + (n & 1)
                } else {
                    C_ROCK + n
                };
                BG[o + px as usize] = c as u8;
            }
            if on_screen {
                copy_bytes(vram().add(o), BG.as_ptr().add(o), 4);
                copy_bytes(vram().add(o + PAGE_BYTES), BG.as_ptr().add(o), 4);
            }
            o += WW as usize;
        }
    }
}

fn refresh_cell(i: usize) {
    unsafe {
        if SHOWN[i] != cell_look(i) {
            paint_cell(i, true);
        }
    }
}

// After terrain changes around a cell: walkability, looks, routes.
static mut NAV_DIRTY: bool = true;

fn walkable(gx: i32, gy: i32) -> bool {
    unsafe {
        if gx <= 0 || gx >= GW - 1 || gy <= 0 || gy >= GH - 1 {
            return false;
        }
        let t = TER[idx(gx, gy)];
        if t == TUNNEL {
            return true;
        }
        if t != AIR {
            return false;
        }
        for k in 0..8 {
            let nx = gx + dxt(k);
            let ny = gy + dyt(k);
            if inside(nx, ny) && solid(TER[idx(nx, ny)]) {
                return true;
            }
        }
        false
    }
}

// Offsets of the 8 neighbours in the grid (set up in main).
static mut NOFF: [i32; 8] = [0; 8];

fn init_offsets() {
    unsafe {
        for k in 0..8 {
            NOFF[k as usize] = dyt(k) * GW + dxt(k);
        }
    }
}

fn changed_around(gx: i32, gy: i32, r: i32) {
    unsafe {
        for y in gy - r - 1..gy + r + 2 {
            for x in gx - r - 1..gx + r + 2 {
                if inside(x, y) {
                    let i = idx(x, y);
                    WALK[i] = walkable(x, y) as u8;
                    refresh_cell(i);
                }
            }
        }
        NAV_DIRTY = true;
    }
}

// Breadth-first distances over walkable cells from a set of sources.
fn is_source(i: usize, kind: u32) -> bool {
    unsafe {
        match kind {
            0 => HOMEC[i] != 0,
            1 => TER[i] == AIR,
            _ => {
                for k in 0..8 {
                    let n = (i as i32 + NOFF[k]) as usize;
                    if MARK[n] != 0 && TER[n] == DIRT {
                        return true;
                    }
                }
                false
            }
        }
    }
}

fn bfs(f: *mut u16, kind: u32) {
    unsafe {
        let mut head: usize = 0;
        let mut tail: usize = 0;
        fill_bytes(f as *mut u8, 255, NC * 2);
        for w in 0..NWALK {
            let i = WLIST[w] as usize;
            if is_source(i, kind) {
                *f.add(i) = 0;
                QUEUE[tail] = i as u32;
                tail += 1;
            }
        }
        while head < tail {
            let c = QUEUE[head] as i32;
            head += 1;
            let d = *f.add(c as usize) + 1;
            for k in 0..8 {
                let n = (c + NOFF[k]) as usize;
                if WALK[n] != 0 && *f.add(n) == 65535 {
                    *f.add(n) = d;
                    QUEUE[tail] = n as u32;
                    tail += 1;
                }
            }
        }
    }
}

static mut MARKS: u32 = 0;

static mut ROUTE_STEP: u32 = 0;

// One of the three rebuilds per call (spread over ticks).
fn route_step() {
    unsafe {
        match ROUTE_STEP {
            3 => {
                list_walkable();
                bfs(HOMED.as_mut_ptr(), 0);
            }
            2 => bfs(UPD.as_mut_ptr(), 1),
            1 => bfs(DIGD.as_mut_ptr(), 2),
            _ => {}
        }
        if ROUTE_STEP > 0 {
            ROUTE_STEP -= 1;
        }
    }
}

fn list_walkable() {
    unsafe {
        NWALK = 0;
        let words = WALK.as_ptr() as *const u32;
        for w in 0..NC / 4 {
            if *words.add(w) != 0 {
                for k in 0..4 {
                    let i = w * 4 + k;
                    if WALK[i] != 0 {
                        WLIST[NWALK] = i as u32;
                        NWALK += 1;
                    }
                }
            }
        }
    }
}

// All three routes at once (a new world).
fn update_routes() {
    unsafe {
        list_walkable();
        bfs(HOMED.as_mut_ptr(), 0);
        bfs(UPD.as_mut_ptr(), 1);
        bfs(DIGD.as_mut_ptr(), 2);
        NAV_DIRTY = false;
        ROUTE_STEP = 0;
    }
}

// The top air cell of a column (just above the ground).
fn surface_y(gx: i32) -> i32 {
    unsafe {
        let mut y: i32 = 0;
        while y < GH && !solid(TER[idx(gx, y)]) {
            y += 1;
        }
        y - 1
    }
}

fn food_at(gx: i32, amount: u32) {
    unsafe {
        for x in gx - 1..gx + 2 {
            if x >= 0 && x < GW {
                let y = surface_y(x);
                if y >= 0 {
                    let i = idx(x, y);
                    if TER[i] == AIR {
                        let f = FOOD[i] as u32 + amount;
                        FOOD[i] = if f > 255 { 255 } else { f as u8 };
                        refresh_cell(i);
                    }
                }
            }
        }
    }
}

fn near_nest(gx: i32) -> bool {
    unsafe { gx > EC - 24 && gx < EC + 24 }
}

fn new_world() {
    unsafe {
        EC = 240;
        // Rolling ground: flat at the nest, wandering away from it on
        // both sides.
        for x in EC - 8..EC + 9 {
            GROUND[x as usize] = 25;
        }
        for side in 0..2 {
            let mut h: i32 = 25;
            let mut x = if side == 0 { EC - 9 } else { EC + 9 };
            while x >= 0 && x < GW {
                if random_range(0, 4) == 0 {
                    h += random_range(-1, 2);
                }
                if h < 17 {
                    h = 17;
                }
                if h > 32 {
                    h = 32;
                }
                GROUND[x as usize] = h;
                x += if side == 0 { -1 } else { 1 };
            }
        }
        for i in 0..NC {
            let gx = i as i32 % GW;
            let gy = i as i32 / GW;
            TER[i] = if gy < GROUND[gx as usize] { AIR } else { DIRT };
            FOOD[i] = 0;
            TRAIL[i] = 0;
            MARK[i] = 0;
            HOMEC[i] = 0;
            CLOUD[i] = 0;
        }
        // Clouds.
        for _ in 0..9 {
            let cx = random_range(0, GW);
            let cy = random_range(3, 13);
            let w = random_range(5, 14);
            for y in cy - 3..cy + 3 {
                for x in cx - w..cx + w + 1 {
                    let dx = (x - cx) * 3;
                    let dy = (y - cy) * w;
                    if inside(x, y) && dx * dx + dy * dy * 4 < w * w * 9 + random_range(0, w * w * 3) {
                        CLOUD[idx(x, y)] = 1;
                    }
                }
            }
        }
        // Rock: seams underground and boulders on the surface.
        for _ in 0..22 {
            let x = random_range(0, GW);
            if near_nest(x) {
                continue;
            }
            let y = if random_range(0, 3) == 0 { GROUND[x as usize] } else { random_range(36, GH - 2) };
            let r = random_range(1, 5);
            for yy in y - r..y + r + 1 {
                for xx in x - r * 2..x + r * 2 + 1 {
                    let dx = xx - x;
                    let dy = (yy - y) * 2;
                    if inside(xx, yy) && dx * dx + dy * dy <= r * r * 4 && !near_nest(xx) {
                        TER[idx(xx, yy)] = ROCK;
                    }
                }
            }
        }
        // The nest: a shaft down from the entrance to the queen's chamber.
        EC = 240;
        EG = GROUND[EC as usize];
        QX = EC;
        QY = EG + 16;
        for y in EG..QY {
            TER[idx(EC, y)] = TUNNEL;
            TER[idx(EC + 1, y)] = TUNNEL;
        }
        for y in QY - 3..QY + 3 {
            for x in QX - 9..QX + 10 {
                let dx = (x - QX) * 2;
                let dy = (y - QY) * 6;
                if dx * dx + dy * dy <= 18 * 18 {
                    let i = idx(x, y);
                    TER[i] = TUNNEL;
                    HOMEC[i] = 1;
                }
            }
        }
        // A first gallery, marked for digging.
        MARKS = 0;
        for x in QX + 9..QX + 26 {
            MARK[idx(x, QY + 1)] = 1;
            MARK[idx(x, QY + 2)] = 1;
            MARKS += 2;
        }
        for i in 0..NC {
            let gx = i as i32 % GW;
            let gy = i as i32 / GW;
            WALK[i] = walkable(gx, gy) as u8;
        }
        // Food on the surface.
        for _ in 0..8 {
            let mut x = random_range(4, GW - 4);
            while near_nest(x) {
                x = random_range(4, GW - 4);
            }
            let y = surface_y(x);
            for xx in x - 1..x + 2 {
                let yy = surface_y(xx);
                if yy >= 0 {
                    FOOD[idx(xx, yy)] = random_range(120, 255) as u8;
                }
            }
            let _ = y;
        }
        // Paint BG, then copy it to both pages.
        paint_world();
        for y in 0..PH {
            let o = (y * WW) as usize;
            copy_bytes(vram().add(o), BG.as_ptr().add(o), WW as usize);
            copy_bytes(vram().add(o + PAGE_BYTES), BG.as_ptr().add(o), WW as usize);
        }
        update_routes();
    }
}

// ===================================================== ants and spiders

const MAXA: usize = 400;
const ST_FORAGE: u8 = 0;
const ST_HOME: u8 = 1;
const ST_DIG: u8 = 2;
const ST_SOIL: u8 = 3;
const ST_GUARD: u8 = 4;

static mut AX: [i32; 400] = [0; 400];
static mut AY: [i32; 400] = [0; 400];
static mut AT: [i32; 400] = [0; 400];     // the cell it is walking to
static mut APREV: [i32; 400] = [0; 400];
static mut AFACE: [i32; 400] = [0; 400];  // -1 left, 1 right
static mut AKIND: [u8; 400] = [0; 400];   // 0 none, 1 worker, 2 soldier
static mut ASTATE: [u8; 400] = [0; 400];
static mut ACARRY: [u8; 400] = [0; 400];  // 0, 1 food, 2 soil
static mut ASTEP: [u32; 400] = [0; 400];
static mut AWORK: [u32; 400] = [0; 400];
static mut AAGE: [u32; 400] = [0; 400];
static mut ALIFE: [u32; 400] = [0; 400];
static mut DRAWN_X: [i32; 800] = [0; 800];
static mut DRAWN_Y: [i32; 800] = [0; 800];
static mut DRAWN: [bool; 800] = [false; 800];

const MAXS: usize = 8;
static mut SX: [i32; 8] = [0; 8];
static mut SY: [i32; 8] = [0; 8];
static mut ST: [i32; 8] = [0; 8];
static mut SFACE: [i32; 8] = [0; 8];
static mut SHP: [i32; 8] = [0; 8];
static mut SALIVE: [bool; 8] = [false; 8];
static mut SCOOL: [u32; 8] = [0; 8];
static mut SDRAWN_X: [i32; 16] = [0; 16];
static mut SDRAWN_Y: [i32; 16] = [0; 16];
static mut SDRAWN: [bool; 16] = [false; 16];

static mut STORE: u32 = 0;
static mut WORKERS: u32 = 0;
static mut SOLDIERS: u32 = 0;
static mut SPIDERS: u32 = 0;
static mut DIGGERS: u32 = 0;
static mut TICK: u32 = 0;
static mut BORN: u32 = 0;
static mut EATEN: u32 = 0;
static mut GATHERED: u32 = 0;
static mut DUG: u32 = 0;
static mut AUTO: bool = true;
static mut OVER: bool = false;

static mut MSG: [u8; 70] = [0; 70];
static mut MSGLEN: usize = 0;
static mut MSGTIME: u32 = 0;

fn set_msg(s: &str) {
    unsafe {
        MSGLEN = 0;
        for c in s.bytes() {
            if MSGLEN < 70 {
                MSG[MSGLEN] = c;
                MSGLEN += 1;
            }
        }
        MSGTIME = 200;
    }
}

fn cx_of(c: i32) -> i32 {
    (c % GW) * 4 + 2
}

fn cy_of(c: i32) -> i32 {
    (c / GW) * 4 + 2
}

fn sgn(v: i32) -> i32 {
    if v > 0 { 1 } else if v < 0 { -1 } else { 0 }
}

fn spawn_ant(kind: u8) -> bool {
    unsafe {
        for i in 0..MAXA {
            if AKIND[i] == 0 {
                let c = QY * GW + QX + random_range(-6, 7);
                AKIND[i] = kind;
                AT[i] = c;
                APREV[i] = c;
                AX[i] = cx_of(c);
                AY[i] = cy_of(c);
                AFACE[i] = if random_range(0, 2) == 0 { -1 } else { 1 };
                ASTATE[i] = if kind == 2 { ST_GUARD } else { ST_FORAGE };
                ACARRY[i] = 0;
                ASTEP[i] = 0;
                AWORK[i] = 0;
                AAGE[i] = 0;
                ALIFE[i] = if kind == 2 { random_range(12000, 16000) as u32 } else { random_range(9000, 13000) as u32 };
                if kind == 1 && !NAV_DIRTY {
                    choose_job(i);
                }
                if kind == 2 {
                    SOLDIERS += 1;
                } else {
                    WORKERS += 1;
                }
                BORN += 1;
                return true;
            }
        }
        false
    }
}

fn kill_ant(i: usize) {
    unsafe {
        if AKIND[i] == 2 {
            SOLDIERS -= 1;
        } else if AKIND[i] == 1 {
            WORKERS -= 1;
        }
        AKIND[i] = 0;
    }
}

fn spawn_spider(gx: i32) {
    unsafe {
        let y = surface_y(gx);
        if y < 0 {
            return;
        }
        for s in 0..MAXS {
            if !SALIVE[s] {
                let c = y * GW + gx;
                SALIVE[s] = true;
                ST[s] = c;
                SX[s] = cx_of(c);
                SY[s] = cy_of(c);
                SFACE[s] = if gx < EC { 1 } else { -1 };
                SHP[s] = 30;
                SCOOL[s] = 0;
                SPIDERS += 1;
                set_msg("A spider is coming!");
                return;
            }
        }
    }
}

fn go(i: usize, n: i32) {
    unsafe {
        APREV[i] = AT[i];
        let d = (n % GW) - (AT[i] % GW);
        if d != 0 {
            AFACE[i] = d;
        }
        AT[i] = n;
    }
}

// Step along a distance map: the walkable neighbour nearest its target.
fn step_down(i: usize, f: *const u16) -> bool {
    unsafe {
        let c = AT[i];
        let gx = c % GW;
        let gy = c / GW;
        let mut best = *f.add(c as usize);
        let mut bn: i32 = -1;
        let r = random_range(0, 8);
        for j in 0..8 {
            let k = (j + r) & 7;
            let nx = gx + dxt(k);
            let ny = gy + dyt(k);
            if inside(nx, ny) {
                let n = idx(nx, ny);
                if WALK[n] != 0 && *f.add(n) < best {
                    best = *f.add(n);
                    bn = n as i32;
                }
            }
        }
        if bn < 0 {
            return false;
        }
        go(i, bn);
        true
    }
}

// Walk along the surface: scent, food and the way it is facing.
fn wander_surface(i: usize, follow_scent: bool) {
    unsafe {
        let c = AT[i];
        let gx = c % GW;
        let gy = c / GW;
        let mut best: i32 = -1;
        let mut bn: i32 = -1;
        for k in 0..8 {
            let nx = gx + dxt(k);
            let ny = gy + dyt(k);
            if !inside(nx, ny) {
                continue;
            }
            let n = idx(nx, ny);
            if WALK[n] == 0 || TER[n] != AIR || n as i32 == APREV[i] {
                continue;
            }
            let mut score = random_range(0, 50);
            if dxt(k) == AFACE[i] {
                score += 80;
            }
            if follow_scent {
                score += TRAIL[n] as i32 * 2;
                if FOOD[n] > 0 {
                    score += 2000;
                }
            }
            if score > best {
                best = score;
                bn = n as i32;
            }
        }
        if bn < 0 {
            // A dead end: turn round.
            AFACE[i] = 0 - AFACE[i];
            let p = APREV[i];
            if p != c && WALK[p as usize] != 0 {
                go(i, p);
            }
            return;
        }
        if random_range(0, 120) == 0 {
            AFACE[i] = 0 - AFACE[i];
        }
        go(i, bn);
    }
}

// Drop carried soil beside the entrance, on the lowest of a few nearby
// columns, so the soil heaps up into a mound.
fn drop_soil(i: usize) {
    unsafe {
        let gx = AT[i] % GW;
        let side = if gx < EC { -1 } else { 1 };
        let mut best_x: i32 = -1;
        let mut best_y: i32 = -1;
        for _ in 0..4 {
            let x = gx + side * random_range(3, 14);
            if x > 0 && x < GW - 1 && (x < EC - 2 || x > EC + 3) {
                let y = surface_y(x);
                if y > best_y {
                    best_y = y;
                    best_x = x;
                }
            }
        }
        if best_x >= 0 && best_y >= 2 {
            let n = idx(best_x, best_y);
            if TER[n] == AIR && FOOD[n] == 0 {
                TER[n] = DIRT;
                changed_around(best_x, best_y, 1);
            }
        }
        ACARRY[i] = 0;
    }
}

fn choose_job(i: usize) {
    unsafe {
        ASTEP[i] = 0;
        let cap = if WORKERS / 3 > 2 { WORKERS / 3 } else { 2 };
        if MARKS > 0 && DIGGERS < cap && DIGD[AT[i] as usize] != 65535 {
            ASTATE[i] = ST_DIG;
            DIGGERS += 1;
        } else {
            ASTATE[i] = ST_FORAGE;
        }
    }
}

fn worker(i: usize) {
    unsafe {
        let c = AT[i] as usize;
        let on_surface = TER[c] == AIR;
        ASTEP[i] += 1;
        match ASTATE[i] {
            ST_FORAGE => {
                if !on_surface {
                    // Needed for digging?
                    let cap = if WORKERS / 3 > 2 { WORKERS / 3 } else { 2 };
                    if MARKS > 0 && DIGGERS < cap && ACARRY[i] == 0 && DIGD[c] != 65535 {
                        ASTATE[i] = ST_DIG;
                        DIGGERS += 1;
                        return;
                    }
                    if !step_down(i, UPD.as_ptr()) {
                        choose_job(i);
                    }
                    return;
                }
                if FOOD[c] > 0 {
                    FOOD[c] -= 1;
                    refresh_cell(c);
                    ACARRY[i] = 1;
                    ASTATE[i] = ST_HOME;
                    ASTEP[i] = 0;
                    AFACE[i] = 0 - AFACE[i];
                    return;
                }
                if ASTEP[i] > 900 {
                    ASTATE[i] = ST_HOME;
                }
                wander_surface(i, true);
            }
            ST_HOME => {
                if on_surface && ACARRY[i] == 1 {
                    // The way home, scented strongest near the food.
                    let s = 255 - (if ASTEP[i] > 230 { 230 } else { ASTEP[i] });
                    if (TRAIL[c] as u32) < s {
                        TRAIL[c] = s as u8;
                        refresh_cell(c);
                    }
                }
                if HOMED[c] == 0 {
                    if ACARRY[i] == 1 {
                        STORE += 1;
                        GATHERED += 1;
                    }
                    ACARRY[i] = 0;
                    choose_job(i);
                    return;
                }
                if !step_down(i, HOMED.as_ptr()) {
                    wander_surface(i, false);
                }
            }
            ST_DIG => {
                let gx = c as i32 % GW;
                let gy = c as i32 / GW;
                for k in 0..8 {
                    let nx = gx + dxt(k);
                    let ny = gy + dyt(k);
                    if !inside(nx, ny) {
                        continue;
                    }
                    let n = idx(nx, ny);
                    if MARK[n] != 0 && TER[n] == DIRT {
                        AWORK[i] += 1;
                        if dxt(k) != 0 {
                            AFACE[i] = dxt(k);
                        }
                        if AWORK[i] >= 30 {
                            TER[n] = TUNNEL;
                            MARK[n] = 0;
                            MARKS -= 1;
                            AWORK[i] = 0;
                            DUG += 1;
                            changed_around(nx, ny, 1);
                            ACARRY[i] = 2;
                            ASTATE[i] = ST_SOIL;
                        }
                        return;
                    }
                }
                AWORK[i] = 0;
                if DIGD[c] == 65535 || !step_down(i, DIGD.as_ptr()) {
                    ASTATE[i] = ST_FORAGE;
                }
            }
            _ => {
                // Carrying soil up and out.
                if on_surface {
                    drop_soil(i);
                    choose_job(i);
                    return;
                }
                if !step_down(i, UPD.as_ptr()) {
                    ACARRY[i] = 0;
                    choose_job(i);
                }
            }
        }
    }
}

fn nearest_spider(x: i32, y: i32, range: i32) -> i32 {
    unsafe {
        let mut best: i32 = -1;
        let mut bd = range * range;
        for s in 0..MAXS {
            if SALIVE[s] {
                let dx = SX[s] - x;
                let dy = SY[s] - y;
                let d = dx * dx + dy * dy;
                if d < bd {
                    bd = d;
                    best = s as i32;
                }
            }
        }
        best
    }
}

// Move to the walkable neighbour nearest a point.
fn step_toward(i: usize, tx: i32, ty: i32) {
    unsafe {
        let c = AT[i];
        let gx = c % GW;
        let gy = c / GW;
        let dx = tx - cx_of(c);
        let dy = ty - cy_of(c);
        let mut best = dx * dx + dy * dy;
        let mut bn: i32 = -1;
        for k in 0..8 {
            let nx = gx + dxt(k);
            let ny = gy + dyt(k);
            if inside(nx, ny) {
                let n = idx(nx, ny);
                if WALK[n] != 0 {
                    let ex = tx - cx_of(n as i32);
                    let ey = ty - cy_of(n as i32);
                    let d = ex * ex + ey * ey;
                    if d < best {
                        best = d;
                        bn = n as i32;
                    }
                }
            }
        }
        if bn >= 0 {
            go(i, bn);
        }
    }
}

fn soldier(i: usize) {
    unsafe {
        let s = nearest_spider(AX[i], AY[i], 160);
        if s >= 0 {
            let s = s as usize;
            let dx = SX[s] - AX[i];
            let dy = SY[s] - AY[i];
            if dx * dx + dy * dy < 49 {
                if (TICK + i as u32) % 5 == 0 {
                    SHP[s] -= 1;
                    if SHP[s] <= 0 {
                        SALIVE[s] = false;
                        SPIDERS -= 1;
                        set_msg("The soldiers killed a spider.");
                    }
                }
                return;
            }
            step_toward(i, SX[s], SY[s]);
            return;
        }
        let c = AT[i] as usize;
        if TER[c] != AIR {
            // Up to the surface to stand guard.
            if !step_down(i, UPD.as_ptr()) {
                step_toward(i, EC * 4, EG * 4);
            }
            return;
        }
        // Patrol the surface near the entrance.
        let gx = c as i32 % GW;
        if gx < EC - 30 {
            AFACE[i] = 1;
        }
        if gx > EC + 30 {
            AFACE[i] = -1;
        }
        if (TICK + i as u32) % 3 == 0 {
            wander_surface(i, false);
        }
    }
}

// Each tick an ant moves a pixel towards its cell; there, it decides.
fn ant(i: usize) {
    unsafe {
        let tx = cx_of(AT[i]);
        let ty = cy_of(AT[i]);
        if AX[i] != tx || AY[i] != ty {
            AX[i] += sgn(tx - AX[i]);
            AY[i] += sgn(ty - AY[i]);
            return;
        }
        let c = AT[i] as usize;
        if solid(TER[c]) {
            // Buried by Fill: back to the chamber.
            let q = QY * GW + QX;
            AT[i] = q;
            AX[i] = cx_of(q);
            AY[i] = cy_of(q);
            return;
        }
        if WALK[c] == 0 {
            // Nothing to hold on to: fall.
            go(i, c as i32 + GW);
            return;
        }
        if AKIND[i] == 2 {
            soldier(i);
        } else {
            worker(i);
        }
    }
}

fn spider(s: usize) {
    unsafe {
        if SCOOL[s] > 0 {
            SCOOL[s] -= 1;
        }
        // Hunt the nearest ant on the surface.
        let mut target: i32 = -1;
        let mut bd: i32 = 120 * 120;
        for i in 0..MAXA {
            if AKIND[i] != 0 && TER[AT[i] as usize] == AIR {
                let dx = AX[i] - SX[s];
                let dy = AY[i] - SY[s];
                let d = dx * dx + dy * dy;
                if d < bd {
                    bd = d;
                    target = i as i32;
                }
            }
        }
        if target >= 0 && bd < 25 && SCOOL[s] == 0 {
            kill_ant(target as usize);
            EATEN += 1;
            SCOOL[s] = 50;
        }
        if TICK % 2 != 0 {
            return;
        }
        let tx = cx_of(ST[s]);
        let ty = cy_of(ST[s]);
        if SX[s] != tx || SY[s] != ty {
            SX[s] += sgn(tx - SX[s]);
            SY[s] += sgn(ty - SY[s]);
            return;
        }
        // Next surface cell: towards the prey, or onwards.
        let c = ST[s];
        let gx = c % GW;
        let gy = c / GW;
        let mut best: i32 = -1000000;
        let mut bn: i32 = -1;
        for k in 0..8 {
            let nx = gx + dxt(k);
            let ny = gy + dyt(k);
            if !inside(nx, ny) {
                continue;
            }
            let n = idx(nx, ny);
            if WALK[n] == 0 || TER[n] != AIR {
                continue;
            }
            let mut score = random_range(0, 30);
            if target >= 0 {
                let t = target as usize;
                let ex = AX[t] - cx_of(n as i32);
                let ey = AY[t] - cy_of(n as i32);
                score -= (ex * ex + ey * ey) / 8;
            } else if dxt(k) == SFACE[s] {
                score += 60;
            }
            if score > best {
                best = score;
                bn = n as i32;
            }
        }
        if bn < 0 || random_range(0, 200) == 0 {
            SFACE[s] = 0 - SFACE[s];
        }
        if bn >= 0 {
            let d = (bn % GW) - gx;
            if d != 0 {
                SFACE[s] = d;
            }
            ST[s] = bn;
        }
    }
}

fn tick() {
    unsafe {
        TICK += 1;
        if NAV_DIRTY && ROUTE_STEP == 0 && TICK % 15 == 0 {
            NAV_DIRTY = false;
            ROUTE_STEP = 3;
        }
        route_step();
        DIGGERS = 0;
        for i in 0..MAXA {
            if AKIND[i] != 0 && ASTATE[i] == ST_DIG {
                DIGGERS += 1;
            }
        }
        for i in 0..MAXA {
            if AKIND[i] != 0 {
                ant(i);
                AAGE[i] += 1;
                if AKIND[i] != 0 && AAGE[i] > ALIFE[i] {
                    kill_ant(i);
                }
            }
        }
        for s in 0..MAXS {
            if SALIVE[s] {
                spider(s);
            }
        }
        // Scent fades: a sixteenth of the sky each tick.
        let mut i = (TICK % 16) as usize;
        while i < (GW * 40) as usize {
            if TRAIL[i] > 0 {
                TRAIL[i] = if TRAIL[i] > 2 { TRAIL[i] - 2 } else { 0 };
                refresh_cell(i);
            }
            i += 16;
        }
        // The queen lays eggs while there is food to spare.
        if AUTO && TICK % 120 == 0 && STORE >= 20 {
            if SOLDIERS * 6 < WORKERS && STORE >= 40 {
                if spawn_ant(2) {
                    STORE -= 25;
                }
            } else if spawn_ant(1) {
                STORE -= 10;
            }
        }
        if TICK % 500 == 0 {
            let need = (WORKERS + SOLDIERS + 9) / 10;
            if STORE >= need {
                STORE -= need;
            } else {
                STORE = 0;
                let mut old: i32 = -1;
                let mut age: u32 = 0;
                for i in 0..MAXA {
                    if AKIND[i] != 0 && AAGE[i] >= age {
                        age = AAGE[i];
                        old = i as i32;
                    }
                }
                if old >= 0 {
                    kill_ant(old as usize);
                    set_msg("The colony is starving!");
                }
            }
        }
        if TICK % 1500 == 0 {
            let mut x = random_range(4, GW - 4);
            while near_nest(x) {
                x = random_range(4, GW - 4);
            }
            food_at(x, random_range(100, 220) as u32);
        }
        let day = TICK / 2400;
        if TICK % 2400 == 1200 && day >= 1 && random_range(0, 3) < 2 {
            spawn_spider(if random_range(0, 2) == 0 { 1 } else { GW - 2 });
            if day >= 4 && random_range(0, 2) == 0 {
                spawn_spider(if random_range(0, 2) == 0 { 1 } else { GW - 2 });
            }
        }
        if WORKERS + SOLDIERS == 0 && STORE < 10 {
            OVER = true;
        }
    }
}

// ===================================================== panel and input

const B_DIG: usize = 0;
const B_FOOD: usize = 1;
const B_FILL: usize = 2;
const B_SPIDER: usize = 3;
const B_WORKER: usize = 4;
const B_SOLDIER: usize = 5;
const B_AUTO: usize = 6;
const B_PAUSE: usize = 7;
const B_SPEED: usize = 8;
const B_NEW: usize = 9;
const B_QUIT: usize = 10;
const NBTN: usize = 11;

static mut TOOL: usize = 0;
static mut PAUSED: bool = false;
static mut SPEED: u32 = 1;
static mut BX: [i32; 11] = [0; 11];
static mut BY: [i32; 11] = [0; 11];
static mut BW: [i32; 11] = [0; 11];

const MM_X: i32 = 392;
const MM_Y: i32 = 4;

fn button_label(b: usize) -> &'static str {
    unsafe {
        match b {
            0 => "1 Dig",
            1 => "2 Food",
            2 => "3 Fill",
            3 => "4 Spider",
            4 => "W +Worker 10",
            5 => "S +Soldier 25",
            6 => if AUTO { "A Breed: on " } else { "A Breed: off" },
            7 => if PAUSED { "P Resume" } else { "P Pause " },
            8 => match SPEED {
                1 => "F Speed 1x",
                2 => "F Speed 2x",
                _ => "F Speed 4x",
            },
            9 => "N New",
            _ => "Esc Quit",
        }
    }
}

fn layout_buttons() {
    unsafe {
        let mut x: i32 = 6;
        for b in 0..NBTN {
            if b == B_WORKER || b == B_PAUSE {
                x = 6;
            }
            BX[b] = x;
            BY[b] = if b < B_WORKER { 4 } else if b < B_PAUSE { 21 } else { 38 };
            BW[b] = button_label(b).len() as i32 * 8 + 10;
            x += BW[b] + 4;
        }
    }
}

fn draw_button(b: usize) {
    unsafe {
        let on = (b <= B_SPIDER && TOOL == b) || (b == B_PAUSE && PAUSED) || (b == B_AUTO && AUTO);
        let bg = if on { C_BUTTON_ON } else { C_BUTTON };
        pfill(BX[b], BY[b], BW[b], 14, bg);
        for x in BX[b]..BX[b] + BW[b] {
            ppset(x, BY[b], C_PANEL_HI);
            ppset(x, BY[b] + 13, C_PANEL_LO);
        }
        for y in BY[b]..BY[b] + 14 {
            ppset(BX[b], y, C_PANEL_HI);
            ppset(BX[b] + BW[b] - 1, y, C_PANEL_LO);
        }
        let k = button_label(b).as_bytes();
        let kl: usize = if k[0] == b'E' { 3 } else { 1 };
        let x = ptext(BX[b] + 5, BY[b] + 3, str_from_bytes(&k[..kl]), C_YELLOW, bg);
        ptext(x, BY[b] + 3, str_from_bytes(&k[kl..]), C_TEXT, bg);
    }
}

fn draw_panel() {
    pfill(0, 0, SW, 80, C_PANEL);
    for x in 0..SW {
        ppset(x, 0, C_PANEL_HI);
    }
    for b in 0..NBTN {
        draw_button(b);
    }
    draw_stats();
    draw_minimap();
    panel_changed();
}

static mut STATS_END: i32 = 0;

fn draw_stats() {
    unsafe {
        let y = 62;
        let mut x = ptext(8, y, "Day ", C_DIM, C_PANEL);
        x = ptext(x, y, num(TICK / 2400 + 1), C_TEXT, C_PANEL);
        x = ptext(x, y, "  Food ", C_DIM, C_PANEL);
        x = ptext(x, y, num(STORE), if STORE < 10 { C_RED } else { C_YELLOW }, C_PANEL);
        x = ptext(x, y, "  Ants ", C_DIM, C_PANEL);
        x = ptext(x, y, num(WORKERS), C_GREEN, C_PANEL);
        x = ptext(x, y, "+", C_DIM, C_PANEL);
        x = ptext(x, y, num(SOLDIERS), C_RED, C_PANEL);
        x = ptext(x, y, "  Digging ", C_DIM, C_PANEL);
        x = ptext(x, y, num(DIGGERS), C_TEXT, C_PANEL);
        x = ptext(x, y, "  Spiders ", C_DIM, C_PANEL);
        x = ptext(x, y, num(SPIDERS), if SPIDERS > 0 { C_RED } else { C_TEXT }, C_PANEL);
        if STATS_END > x {
            pfill(x, y, STATS_END - x, 8, C_PANEL);
        }
        STATS_END = x;
        panel_changed();
    }
}

// The whole world at 1/8 scale (one pixel per 2x2 cells) with the view
// outlined; ants and spiders as dots.
fn draw_minimap() {
    unsafe {
        for my in 0..50 {
            for mx in 0..240 {
                let i = idx(mx * 2, my * 2);
                let c: u8 = match TER[i] {
                    AIR => {
                        if FOOD[i] > 0 || FOOD[i + 1] > 0 || FOOD[i + GW as usize] > 0 {
                            C_FOOD as u8
                        } else if my < 14 {
                            (C_SKY + (my / 2) as u32) as u8
                        } else {
                            (C_SKY + 7) as u8
                        }
                    }
                    DIRT => {
                        if MARK[i] != 0 {
                            C_MARK as u8
                        } else if grassy(i) {
                            C_GRASS as u8 + 1
                        } else {
                            C_DIRT as u8 + 1
                        }
                    }
                    TUNNEL => C_TUNNEL as u8,
                    _ => C_ROCK as u8 + 1,
                };
                ppset(MM_X + mx, MM_Y + my, c);
            }
        }
        for i in 0..MAXA {
            if AKIND[i] != 0 {
                ppset(MM_X + AX[i] / 8, MM_Y + AY[i] / 8, if AKIND[i] == 2 { C_SOLDIER_HI } else { C_BLACK });
            }
        }
        for s in 0..MAXS {
            if SALIVE[s] {
                ppset(MM_X + SX[s] / 8, MM_Y + SY[s] / 8, C_RED);
                ppset(MM_X + SX[s] / 8 + 1, MM_Y + SY[s] / 8, C_RED);
            }
        }
        let vx = MM_X + VX / 8;
        for x in vx..vx + 80 {
            ppset(x, MM_Y, C_WHITE);
            ppset(x, MM_Y + 49, C_WHITE);
        }
        for y in MM_Y..MM_Y + 50 {
            ppset(vx, y, C_WHITE);
            ppset(vx + 79, y, C_WHITE);
        }
        panel_changed();
    }
}

fn scroll_to(x: i32) {
    unsafe {
        VX = x;
        if VX < 0 {
            VX = 0;
        }
        if VX > WW - SW {
            VX = WW - SW;
        }
    }
}

fn press_button(b: usize) -> bool {
    unsafe {
        match b {
            B_WORKER => {
                if STORE >= 10 && spawn_ant(1) {
                    STORE -= 10;
                } else {
                    set_msg("Not enough food (a worker costs 10).");
                }
            }
            B_SOLDIER => {
                if STORE >= 25 && spawn_ant(2) {
                    STORE -= 25;
                } else {
                    set_msg("Not enough food (a soldier costs 25).");
                }
            }
            B_AUTO => AUTO = !AUTO,
            B_PAUSE => PAUSED = !PAUSED,
            B_SPEED => SPEED = if SPEED >= 4 { 1 } else { SPEED * 2 },
            B_NEW => new_game(),
            B_QUIT => return true,
            _ => {
                TOOL = b;
            }
        }
        draw_panel();
        false
    }
}

static mut FOOD_WAIT: u32 = 0;
static mut LAST_WX: i32 = -1;
static mut LAST_WY: i32 = -1;

// A brush tool along the line from the last mouse position (no gaps when
// the mouse moves fast).
fn use_tool_line(wx: i32, wy: i32, first: bool, cancel: bool) {
    unsafe {
        if first || LAST_WX < 0 || !(cancel || TOOL == B_DIG || TOOL == B_FILL) {
            use_tool(wx, wy, first, cancel);
        } else {
            let dx = wx - LAST_WX;
            let dy = wy - LAST_WY;
            let ax = if dx < 0 { 0 - dx } else { dx };
            let ay = if dy < 0 { 0 - dy } else { dy };
            let steps = (if ax > ay { ax } else { ay }) / 4 + 1;
            for k in 1..steps + 1 {
                use_tool(LAST_WX + dx * k / steps, LAST_WY + dy * k / steps, false, cancel);
            }
        }
        LAST_WX = wx;
        LAST_WY = wy;
    }
}

// Tools at a world pixel; cancel (right button) removes dig marks.
fn use_tool(wx: i32, wy: i32, first: bool, cancel: bool) {
    unsafe {
        let gx = wx >> 2;
        let gy = wy >> 2;
        if cancel || TOOL == B_DIG || TOOL == B_FILL {
            for y in gy - 1..gy + 2 {
                for x in gx - 1..gx + 2 {
                    if !inside(x, y) || HOMEC[idx(x, y)] != 0 {
                        continue;
                    }
                    let i = idx(x, y);
                    if cancel {
                        if MARK[i] != 0 {
                            MARK[i] = 0;
                            MARKS -= 1;
                        }
                    } else if TOOL == B_DIG {
                        if TER[i] == DIRT && MARK[i] == 0 {
                            MARK[i] = 1;
                            MARKS += 1;
                        }
                    } else if TER[i] == TUNNEL && !(x >= EC && x <= EC + 1 && y >= EG && y < QY) {
                        // Fill a tunnel back in (not the main shaft).
                        TER[i] = DIRT;
                        MARK[i] = 0;
                    }
                }
            }
            changed_around(gx, gy, 1);
            return;
        }
        if TOOL == B_FOOD {
            if FOOD_WAIT == 0 {
                food_at(gx, 40);
                FOOD_WAIT = 4;
            }
        } else if first {
            spawn_spider(gx);
        }
    }
}

static mut MX: i32 = 320;
static mut MY: i32 = 240;
static mut MB: u8 = 0;
static mut MB_OLD: u8 = 0;

// The rusty486 mouse: X 0..639 and Y 0..399 over the whole screen.
fn read_mouse() {
    unsafe {
        MB_OLD = MB;
        let x = inb(0x32A) as i32 | (inb(0x32B) as i32) << 8;
        let y = inb(0x32C) as i32 | (inb(0x32D) as i32) << 8;
        MB = inb(0x32E);
        MX = if x > SW - 1 { SW - 1 } else { x };
        MY = y * SH / 400;
        if MY > SH - 1 {
            MY = SH - 1;
        }
    }
}

// ===================================================== the frame

static mut CUR_X: [i32; 2] = [0; 2];
static mut CUR_Y: [i32; 2] = [0; 2];
static mut MSG_X: [i32; 2] = [-1; 2];

fn view_has(x: i32) -> bool {
    unsafe { x > VX - 16 && x < VX + SW + 16 }
}

fn erase_all() {
    unsafe {
        for a in 0..MAXA {
            let i = PAGE * MAXA + a;
            if DRAWN[i] {
                let x = DRAWN_X[i];
                let y = DRAWN_Y[i];
                if x >= 3 && x < WW - 4 && y >= 3 && y < PH - 3 {
                    let mut o = ((y - 2) * WW + x - 3) as usize;
                    for _ in 0..5 {
                        copy_bytes(lfb().add(o), BG.as_ptr().add(o), 7);
                        o += WW as usize;
                    }
                } else {
                    restore(x - 3, y - 2, 7, 5);
                }
                DRAWN[i] = false;
            }
        }
        for k in 0..MAXS {
            let s = PAGE * MAXS + k;
            if SDRAWN[s] {
                restore(SDRAWN_X[s] - 6, SDRAWN_Y[s] - 4, 13, 8);
                SDRAWN[s] = false;
            }
        }
        // The chamber (queen, eggs, stores), the cursor and messages.
        restore(QX * 4 - 40, QY * 4 - 12, 84, 26);
        restore(CUR_X[PAGE], CUR_Y[PAGE], 8, 12);
        if MSG_X[PAGE] >= 0 {
            restore(MSG_X[PAGE], 0, SW, 10);
            restore(MSG_X[PAGE] + 170, 190, 310, 28);
            MSG_X[PAGE] = -1;
        }
    }
}

fn draw_ant(i: usize) {
    unsafe {
        let x = AX[i];
        let y = AY[i];
        let f = if AFACE[i] < 0 { -1 } else { 1 };
        let soldier = AKIND[i] == 2;
        let body = if soldier { C_SOLDIER } else { C_ANT };
        let hi = if soldier { C_SOLDIER_HI } else { C_ANT_HI };
        // Side view: abdomen, thorax, head; legs below, antenna up.
        spot(x - f * 2, y, body);
        spot(x - f, y, body);
        spot(x, y, hi);
        spot(x + f, y, body);
        spot(x + f * 2, y - 1, body);
        spot(x - f, y + 1, C_ANT);
        spot(x + f, y + 1, C_ANT);
        if soldier {
            spot(x + f, y - 1, hi);
        }
        if ACARRY[i] == 1 {
            spot(x + f, y - 1, C_CRUMB);
            spot(x + f * 2, y - 2, C_CRUMB);
        } else if ACARRY[i] == 2 {
            spot(x + f, y - 1, C_SOIL);
            spot(x + f * 2, y - 2, C_SOIL);
            spot(x, y - 1, C_SOIL);
        }
        let r = PAGE * MAXA + i;
        DRAWN_X[r] = x;
        DRAWN_Y[r] = y;
        DRAWN[r] = true;
    }
}

fn draw_all() {
    unsafe {
        // The chamber: stores on the left, eggs on the right, the queen.
        let fx = QX * 4 - 34;
        let fy = QY * 4 + 9;
        let store = if STORE > 150 { 150 } else { STORE };
        let mut k: u32 = 0;
        while k < store {
            let col = ((k / 3) % 20) as i32;
            let row = (k / 60) as i32;
            spot(fx + col, fy - row, (C_FOOD + (k & 3)) as u8);
            k += 3;
        }
        let eggs = if STORE / 15 > 8 { 8 } else { STORE / 15 };
        for e in 0..eggs as i32 {
            spot(QX * 4 + 14 + e * 3, fy, C_EGG);
            spot(QX * 4 + 15 + e * 3, fy, C_EGG);
            spot(QX * 4 + 14 + e * 3, fy - 1, C_EGG);
        }
        sprite(QX * 4 - 6, QY * 4 + 4, 12, 6, queen_art(), false, true);
        for i in 0..MAXA {
            if AKIND[i] != 0 && view_has(AX[i]) {
                draw_ant(i);
            }
        }
        for s in 0..MAXS {
            if SALIVE[s] && view_has(SX[s]) {
                let x = SX[s];
                let y = SY[s];
                sprite(x - 5, y - 3, 11, 6, spider_art(), SFACE[s] < 0, true);
                let r = PAGE * MAXS + s;
                SDRAWN_X[r] = x;
                SDRAWN_Y[r] = y;
                SDRAWN[r] = true;
            }
        }
        if MSGTIME > 0 || PAUSED || OVER {
            MSG_X[PAGE] = VX;
        }
        if MSGTIME > 0 {
            float_text(VX + 8, 1, str_from_bytes(&MSG[..MSGLEN]), C_YELLOW);
            MSGTIME -= 1;
        }
        if PAUSED {
            float_text(VX + SW - 8 * 8, 1, "PAUSED", C_WHITE);
        }
        if OVER {
            float_text(VX + 200, 192, "The colony has died out.", C_RED);
            float_text(VX + 176, 208, "Press N or click New to start again.", C_WHITE);
        }
        CUR_X[PAGE] = VX + MX;
        CUR_Y[PAGE] = MY;
        sprite(VX + MX, MY, 8, 12, cursor_art(), false, false);
    }
}

fn new_game() {
    unsafe {
        for i in 0..MAXA {
            AKIND[i] = 0;
            DRAWN[i] = false;
            DRAWN[MAXA + i] = false;
        }
        for s in 0..MAXS {
            SALIVE[s] = false;
            SDRAWN[s] = false;
            SDRAWN[MAXS + s] = false;
        }
        WORKERS = 0;
        SOLDIERS = 0;
        SPIDERS = 0;
        DIGGERS = 0;
        TICK = 0;
        STORE = 40;
        OVER = false;
        PAUSED = false;
        MSG_X[0] = -1;
        MSG_X[1] = -1;
        new_world();
        scroll_to(EC * 4 - SW / 2);
        for _ in 0..24 {
            spawn_ant(1);
        }
        spawn_ant(2);
        spawn_ant(2);
        draw_panel();
        set_msg("Mark earth with Dig (1) and the workers dig it out.");
    }
}

fn main() {
    if !set_vbe_mode() {
        set_video_mode(3);
        println!("Ant Colony needs a VBE video card with 640x480 in 256 colours.");
        return;
    }
    seed_random(ticks());
    make_noise();
    init_offsets();
    load_font();
    palette();
    layout_buttons();
    new_game();
    let mut quit = false;
    let mut frame: u32 = 0;
    while !quit {
        read_mouse();
        unsafe {
            while key_pressed() {
                let k = read_key();
                let a = (k & 0xFF) as u8;
                let scan = (k >> 8) & 0xFF;
                match scan {
                    0x01 => quit = true,
                    0x4B => scroll_to(VX - 64),
                    0x4D => scroll_to(VX + 64),
                    0x47 => scroll_to(EC * 4 - SW / 2),
                    _ => {}
                }
                let b: usize = match a.to_ascii_lowercase() {
                    b'1' => 0,
                    b'2' => 1,
                    b'3' => 2,
                    b'4' => 3,
                    b'w' => 4,
                    b's' => 5,
                    b'a' => 6,
                    b'p' => 7,
                    b' ' => 7,
                    b'f' => 8,
                    b'n' => 9,
                    _ => 99,
                };
                if b < NBTN && press_button(b) {
                    quit = true;
                }
            }
            let click = MB & 1 != 0 && MB_OLD & 1 == 0;
            if FOOD_WAIT > 0 {
                FOOD_WAIT -= 1;
            }
            if MY >= PH {
                let px = MX;
                let py = MY - PH;
                if MB & 1 != 0 && px >= MM_X && px < MM_X + 240 && py >= MM_Y && py < MM_Y + 50 {
                    // The minimap: jump there.
                    scroll_to((px - MM_X) * 8 - SW / 2);
                } else if click {
                    for b in 0..NBTN {
                        if px >= BX[b] && px < BX[b] + BW[b] && py >= BY[b] && py < BY[b] + 14 && press_button(b) {
                            quit = true;
                        }
                    }
                }
            } else {
                // Scroll at the screen edges.
                if MX < 4 {
                    scroll_to(VX - 12);
                } else if MX > SW - 5 {
                    scroll_to(VX + 12);
                }
                if MB & 1 != 0 {
                    use_tool_line(VX + MX, MY, click, false);
                } else if MB & 2 != 0 {
                    use_tool_line(VX + MX, MY, MB_OLD & 2 == 0, true);
                } else {
                    LAST_WX = -1;
                }
            }
            if !PAUSED && !OVER {
                for _ in 0..SPEED {
                    tick();
                }
            }
            if frame % 8 == 0 {
                draw_stats();
            }
            if frame % 48 == 0 {
                draw_minimap();
            }
        }
        // Draw on the hidden page and show it.
        erase_all();
        blit_panel();
        draw_all();
        wait_retrace();
        flip();
        frame += 1;
    }
    set_video_mode(3);
    unsafe {
        print!("Ant Colony: {} ants were born, ", BORN);
        print!("{} crumbs of food were gathered, ", GATHERED);
        print!("{} cells were dug ", DUG);
        println!("and spiders ate {}.", EATEN);
    }
}
