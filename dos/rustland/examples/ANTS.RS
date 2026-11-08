// ANTS.RS - Ant Colony 1.0, a small colony simulator for rusty486.
//
// 640x480 in 256 colours (VBE mode 101h with the linear framebuffer at
// E0000000h), mouse and keyboard.
//
// Workers wander out of the nest. When one finds food it carries a crumb
// home and leaves a scent trail that is strongest near the food; other
// workers climb that trail. The queen turns stored food into new ants,
// soldiers guard the nest against spiders, and everyone has to eat.
//
// The world is a 160x107 grid of 4x4-pixel cells. A copy of the static
// picture (terrain, scent, food, panel) is kept in BG and drawn on both
// video pages. Each frame the moving things are erased (from BG) and drawn
// again on the hidden page, which is then shown during the vertical
// retrace, so nothing is ever seen half drawn.

const SW: i32 = 640;
const SH: i32 = 480;
const PH: i32 = 428;
const GW: i32 = 160;
const GH: i32 = 107;
const NCELLS: usize = 17120;

const T_GRASS: u8 = 0;
const T_ROCK: u8 = 1;
const T_WATER: u8 = 2;
const T_NEST: u8 = 3;

const NEST_X: i32 = 320;
const NEST_Y: i32 = 220;

static mut BG: [u8; 307200] = [0; 307200];
static mut TER: [u8; 17600] = [0; 17600];
static mut FOOD: [u8; 17600] = [0; 17600];
static mut TRAIL: [u8; 17600] = [0; 17600];
static mut SHOWN: [u8; 17600] = [0; 17600];

static mut FONT: u32 = 0;

// ===================================================== drawing

// The two video pages in the linear framebuffer.
const PAGE_BYTES: usize = 307200;
static mut PAGE: usize = 1;

fn vram() -> *mut u8 {
    let base: u32 = 0xE0000000;
    base as *mut u8
}

// The hidden page, where moving things are drawn.
fn lfb() -> *mut u8 {
    unsafe { vram().add(PAGE * PAGE_BYTES) }
}

// Show the page just drawn (VBE display start) and draw on the other.
fn flip() {
    unsafe {
        let mut r = Regs::new();
        r.eax = 0x4F07;
        r.ebx = 0;
        r.ecx = 0;
        r.edx = (PAGE * 480) as u32;
        dos_int(0x10, &mut r);
        PAGE = 1 - PAGE;
    }
}

fn pal(i: u32, r: u32, g: u32, b: u32) {
    outb(0x3C8, i as u8);
    outb(0x3C9, r as u8);
    outb(0x3C9, g as u8);
    outb(0x3C9, b as u8);
}

// Colours.
const C_GRASS: u32 = 16;
const C_ROCK: u32 = 48;
const C_WATER: u32 = 52;
const C_NEST: u32 = 56;
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

fn mix(a: u32, b: u32, k: u32) -> u32 {
    (a * (8 - k) + b * k) / 8
}

fn palette() {
    pal(0, 0, 0, 0);
    // Grass, blended towards violet scent in 8 steps, 4 shades each.
    for k in 0..8 {
        for s in 0..4 {
            let r = mix(9 + s * 2, 44, k);
            let g = mix(26 + s * 3, 22, k);
            let b = mix(7 + s, 50, k);
            pal(C_GRASS + k * 4 + s, r, g, b);
        }
    }
    for s in 0..4 {
        pal(C_ROCK + s, 26 + s * 5, 25 + s * 5, 24 + s * 5);
        pal(C_WATER + s, 6 + s * 2, 16 + s * 4, 36 + s * 6);
        pal(C_NEST + s, 24 + s * 4, 15 + s * 3, 7 + s * 2);
        pal(C_FOOD + s, 50 + s * 4, 38 + s * 6, 6 + s * 2);
    }
    pal(C_ANT as u32, 4, 2, 1);
    pal(C_ANT_HI as u32, 40, 22, 10);
    pal(C_SOLDIER as u32, 40, 8, 4);
    pal(C_SOLDIER_HI as u32, 60, 22, 12);
    pal(C_CRUMB as u32, 63, 60, 20);
    pal(C_SPIDER as u32, 16, 6, 22);
    pal(C_SPIDER_LEG as u32, 6, 2, 8);
    pal(C_EYE as u32, 63, 10, 10);
    pal(C_QUEEN as u32, 34, 18, 6);
    pal(C_QUEEN_HI as u32, 56, 36, 14);
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
}

// A pixel of the static picture (BG and the screen).
fn pset(x: i32, y: i32, c: u8) {
    let o = (y * SW + x) as usize;
    unsafe {
        BG[o] = c;
        *vram().add(o) = c;
        *vram().add(o + PAGE_BYTES) = c;
    }
}

fn fill_rect(x: i32, y: i32, w: i32, h: i32, c: u8) {
    for yy in y..y + h {
        for xx in x..x + w {
            pset(xx, yy, c);
        }
    }
}

// A pixel of a moving thing (screen only), clipped to the play area.
fn spot(x: i32, y: i32, c: u8) {
    if x >= 0 && x < SW && y >= 0 && y < PH {
        unsafe { *lfb().add((y * SW + x) as usize) = c; }
    }
}

// Put the static picture back over a rectangle.
fn restore(x: i32, y: i32, w: i32, h: i32) {
    let mut x0 = x;
    let mut y0 = y;
    let mut x1 = x + w;
    let mut y1 = y + h;
    if x0 < 0 {
        x0 = 0;
    }
    if y0 < 0 {
        y0 = 0;
    }
    if x1 > SW {
        x1 = SW;
    }
    if y1 > SH {
        y1 = SH;
    }
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    for yy in y0..y1 {
        let o = (yy * SW + x0) as usize;
        unsafe { copy_bytes(lfb().add(o), BG.as_ptr().add(o), (x1 - x0) as usize); }
    }
}

// The BIOS 8x8 font (bit 0 = leftmost pixel).
fn load_font() {
    let mut r = Regs::new();
    r.eax = 0x1130;
    r.ebx = 0x0300;
    dos_int(0x10, &mut r);
    unsafe { FONT = (r.es as u32) * 16 + (r.ebp & 0xFFFF); }
}

// Text into the static picture; returns the x after it.
fn text(x: i32, y: i32, s: &str, fg: u8, bg: u8) -> i32 {
    let mut cx = x;
    for c in s.bytes() {
        let glyph = unsafe { (FONT + c as u32 * 8) as *const u8 };
        for row in 0..8 {
            let bits = unsafe { *glyph.add(row as usize) };
            for col in 0..8 {
                if bits & (1 << col) != 0 {
                    pset(cx + col, y + row, fg);
                } else {
                    pset(cx + col, y + row, bg);
                }
            }
        }
        cx += 8;
    }
    cx
}

// Text drawn as a moving thing (over the play area, erased by restore).
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

// Numbers as text.
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

// Sprites: strings of rows; '.' is transparent.
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

fn sprite(x: i32, y: i32, w: i32, h: i32, rows: &str, clip_play: bool) {
    let b = rows.as_bytes();
    for r in 0..h {
        for c in 0..w {
            let col = sprite_color(b[(r * w + c) as usize]);
            if col != 0 {
                let px = x + c;
                let py = y + r;
                if clip_play {
                    spot(px, py, col);
                } else if px >= 0 && px < SW && py >= 0 && py < SH {
                    unsafe { *lfb().add((py * SW + px) as usize) = col; }
                }
            }
        }
    }
}

fn spider_art() -> &'static str {
    "..l.....l.....l...l...l...sss...l.llsesesll....sssss....llsssssll.l..sssss..l...lsssl.....l.....l.."
}

fn queen_art() -> &'static str {
    "...a..a.......aa.......aQQa.......aa.......qqqq.....qqQQqq....qqqqqq.....qqqq.......qq...."
}

fn cursor_art() -> &'static str {
    "k.......kk......kwk.....kwwk....kwwwk...kwwwwk..kwwwwwk.kwwwwwwkkwwwkkkkkwkwk...kk.kwk..k...kk.."
}

// ===================================================== the world

fn cell_of(x: i32, y: i32) -> usize {
    ((y >> 2) * GW + (x >> 2)) as usize
}

static mut NZ: [u8; 4096] = [0; 4096];

// Grass and rock texture: a 64x64 tile of shades 0..3.
fn make_noise() {
    for y in 0..64 {
        for x in 0..64 {
            let h = (x as u32) * 374761393 ^ (y as u32) * 668265263;
            let h2 = (h ^ (h >> 13)) * 1274126177;
            unsafe { NZ[(y * 64 + x) as usize] = ((h2 >> 16) & 3) as u8; }
        }
    }
}

fn noise(x: i32, y: i32) -> u32 {
    unsafe { NZ[(((y & 63) << 6) | (x & 63)) as usize] as u32 }
}

// What a cell looks like, as a number that changes when it should be redrawn.
fn cell_look(i: usize) -> u8 {
    unsafe {
        match TER[i] {
            T_ROCK => 200,
            T_WATER => 201,
            T_NEST => 202,
            _ => {
                if FOOD[i] > 0 {
                    210 + (FOOD[i] >> 6)
                } else {
                    TRAIL[i] >> 5
                }
            }
        }
    }
}

fn paint_cell(i: usize) {
    unsafe {
        let look = cell_look(i);
        SHOWN[i] = look;
        let cx = (i as i32 % GW) * 4;
        let cy = (i as i32 / GW) * 4;
        let mut o = (cy * SW + cx) as usize;
        for py in 0..4 {
            for px in 0..4 {
                let x = cx + px;
                let y = cy + py;
                let n = noise(x, y);
                let c = match look {
                    200 => C_ROCK + n,
                    201 => C_WATER + ((n + (x as u32 >> 3)) & 3),
                    202 => C_NEST + n,
                    _ => {
                        if look >= 210 {
                            // Thin piles show grass between the crumbs.
                            let amount = (look - 210) as u32;
                            if n <= amount || (px + py) & 1 == 0 { C_FOOD + n } else { C_GRASS + n }
                        } else {
                            C_GRASS + look as u32 * 4 + n
                        }
                    }
                };
                BG[o + px as usize] = c as u8;
            }
            copy_bytes(vram().add(o), BG.as_ptr().add(o), 4);
            copy_bytes(vram().add(o + PAGE_BYTES), BG.as_ptr().add(o), 4);
            o += SW as usize;
        }
    }
}

fn refresh_cell(i: usize) {
    unsafe {
        if SHOWN[i] != cell_look(i) {
            paint_cell(i);
        }
    }
}

fn in_world(x: i32, y: i32) -> bool {
    x >= 0 && x < SW && y >= 0 && y < PH
}

fn blocked(x: i32, y: i32) -> bool {
    if !in_world(x, y) {
        return true;
    }
    let t = unsafe { TER[cell_of(x, y)] };
    t == T_ROCK || t == T_WATER
}

fn near_nest_cell(gx: i32, gy: i32, r: i32) -> bool {
    let dx = gx * 4 + 2 - NEST_X;
    let dy = gy * 4 + 2 - NEST_Y;
    dx * dx + dy * dy < r * r
}

// Paint terrain in a round brush (grid coordinates), away from the nest.
fn blob(gx: i32, gy: i32, r: i32, t: u8) {
    for y in gy - r..gy + r + 1 {
        for x in gx - r..gx + r + 1 {
            if x >= 0 && x < GW && y >= 0 && y < GH && (x - gx) * (x - gx) + (y - gy) * (y - gy) <= r * r {
                if near_nest_cell(x, y, 40) {
                    continue;
                }
                let i = (y * GW + x) as usize;
                unsafe {
                    if t == T_GRASS {
                        TER[i] = T_GRASS;
                        FOOD[i] = 0;
                    } else if TER[i] != T_NEST {
                        TER[i] = t;
                        FOOD[i] = 0;
                        TRAIL[i] = 0;
                    }
                }
            }
        }
    }
}

fn food_pile(gx: i32, gy: i32, r: i32, amount: u32) {
    for y in gy - r..gy + r + 1 {
        for x in gx - r..gx + r + 1 {
            if x >= 0 && x < GW && y >= 0 && y < GH && (x - gx) * (x - gx) + (y - gy) * (y - gy) <= r * r {
                let i = (y * GW + x) as usize;
                unsafe {
                    if TER[i] == T_GRASS {
                        let f = FOOD[i] as u32 + amount;
                        FOOD[i] = if f > 255 { 255 } else { f as u8 };
                    }
                }
            }
        }
    }
}

fn refresh_area(gx: i32, gy: i32, r: i32) {
    for y in gy - r..gy + r + 1 {
        for x in gx - r..gx + r + 1 {
            if x >= 0 && x < GW && y >= 0 && y < GH {
                refresh_cell((y * GW + x) as usize);
            }
        }
    }
}

fn random_far_cell_x() -> i32 {
    random_range(4, GW - 4)
}

fn random_far_cell_y() -> i32 {
    random_range(4, GH - 4)
}

fn new_world() {
    unsafe {
        for i in 0..NCELLS {
            TER[i] = T_GRASS;
            FOOD[i] = 0;
            TRAIL[i] = 0;
        }
        for y in 0..GH {
            for x in 0..GW {
                if near_nest_cell(x, y, 13) {
                    TER[(y * GW + x) as usize] = T_NEST;
                }
            }
        }
    }
    for _ in 0..7 {
        let x = random_far_cell_x();
        let y = random_far_cell_y();
        blob(x, y, random_range(2, 6), T_ROCK);
        blob(x + random_range(-4, 5), y + random_range(-4, 5), random_range(2, 4), T_ROCK);
    }
    for _ in 0..2 {
        let x = random_far_cell_x();
        let y = random_far_cell_y();
        blob(x, y, random_range(4, 8), T_WATER);
        blob(x + random_range(-6, 7), y + random_range(-3, 4), random_range(3, 6), T_WATER);
    }
    for _ in 0..5 {
        food_pile(random_far_cell_x(), random_far_cell_y(), random_range(2, 4), random_range(120, 255) as u32);
    }
    for i in 0..NCELLS {
        paint_cell(i);
    }
}

// ===================================================== ants and spiders

const MAXA: usize = 400;
static mut AX: [i32; 400] = [0; 400];
static mut AY: [i32; 400] = [0; 400];
static mut ADIR: [i32; 400] = [0; 400];
static mut AKIND: [u8; 400] = [0; 400];   // 0 none, 1 worker, 2 soldier
static mut ACARRY: [u8; 400] = [0; 400];  // 0 searching, 1 food, 2 going home empty
static mut ASTEP: [u32; 400] = [0; 400];
static mut AAGE: [u32; 400] = [0; 400];
static mut ALIFE: [u32; 400] = [0; 400];
static mut ADET: [u32; 400] = [0; 400];   // detour ticks after hitting a wall
// Where each ant was drawn (to erase it).
// (one record per page: index page * MAXA + ant)
static mut DRAWN_X: [i32; 800] = [0; 800];
static mut DRAWN_Y: [i32; 800] = [0; 800];
static mut DRAWN: [bool; 800] = [false; 800];

const MAXS: usize = 8;
static mut SX: [i32; 8] = [0; 8];
static mut SY: [i32; 8] = [0; 8];
static mut SDIR: [i32; 8] = [0; 8];
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
static mut TICK: u32 = 0;
static mut BORN: u32 = 0;
static mut EATEN: u32 = 0;
static mut GATHERED: u32 = 0;
static mut AUTO: bool = true;
static mut OVER: bool = false;

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

fn iabs(v: i32) -> i32 {
    if v < 0 { 0 - v } else { v }
}

// The nearest of the 8 directions to a vector.
fn dir_to(dx: i32, dy: i32) -> i32 {
    let ax = iabs(dx);
    let ay = iabs(dy);
    if ax > ay * 2 {
        return if dx > 0 { 0 } else { 4 };
    }
    if ay > ax * 2 {
        return if dy > 0 { 2 } else { 6 };
    }
    if dx > 0 {
        if dy > 0 { 1 } else { 7 }
    } else {
        if dy > 0 { 3 } else { 5 }
    }
}

fn spawn_ant(kind: u8) -> bool {
    unsafe {
        for i in 0..MAXA {
            if AKIND[i] == 0 {
                AKIND[i] = kind;
                AX[i] = NEST_X + random_range(-6, 7);
                AY[i] = NEST_Y + random_range(-6, 7);
                ADIR[i] = random_range(0, 8);
                ACARRY[i] = 0;
                ASTEP[i] = 0;
                AAGE[i] = 0;
                ALIFE[i] = if kind == 2 { random_range(9000, 13000) as u32 } else { random_range(6000, 10000) as u32 };
                ADET[i] = 0;
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
        // A worker drops what it carried.
        if ACARRY[i] == 1 {
            let c = cell_of(AX[i], AY[i]);
            if TER[c] == T_GRASS && FOOD[c] < 255 {
                FOOD[c] += 1;
                refresh_cell(c);
            }
        }
        AKIND[i] = 0;
    }
}

fn spawn_spider(x: i32, y: i32) {
    unsafe {
        for i in 0..MAXS {
            if !SALIVE[i] {
                SALIVE[i] = true;
                SX[i] = x;
                SY[i] = y;
                SDIR[i] = random_range(0, 8);
                SHP[i] = 30;
                SCOOL[i] = 0;
                SPIDERS += 1;
                set_msg("A spider is coming!");
                return;
            }
        }
    }
}

// A spider from a random edge of the world.
fn spider_from_edge() {
    match random_range(0, 4) {
        0 => spawn_spider(4, random_range(10, PH - 10)),
        1 => spawn_spider(SW - 5, random_range(10, PH - 10)),
        2 => spawn_spider(random_range(10, SW - 10), 4),
        _ => spawn_spider(random_range(10, SW - 10), PH - 5),
    }
}

fn try_move(i: usize) {
    unsafe {
        let nx = AX[i] + dxt(ADIR[i]);
        let ny = AY[i] + dyt(ADIR[i]);
        if blocked(nx, ny) {
            // Turn away and keep that heading for a while.
            ADIR[i] = (ADIR[i] + random_range(2, 7)) & 7;
            ADET[i] = random_range(10, 40) as u32;
        } else {
            AX[i] = nx;
            AY[i] = ny;
        }
    }
}

// Scent and food ahead (left, straight, right); picks the strongest.
fn sense(i: usize) {
    unsafe {
        let mut best: i32 = 0;
        let mut best_d = ADIR[i];
        for k in 0..3 {
            let d = (ADIR[i] + k + 7) & 7;
            let px = AX[i] + dxt(d) * 10;
            let py = AY[i] + dyt(d) * 10;
            if !in_world(px, py) {
                continue;
            }
            let c = cell_of(px, py);
            let mut v = TRAIL[c] as i32;
            if FOOD[c] > 0 {
                v += 400;
            }
            if v > best {
                best = v;
                best_d = d;
            }
        }
        if best > 8 && random_range(0, 5) != 0 {
            ADIR[i] = best_d;
        } else if random_range(0, 6) == 0 {
            ADIR[i] = (ADIR[i] + random_range(0, 3) + 7) & 7;
        }
    }
}

fn worker(i: usize) {
    unsafe {
        let c = cell_of(AX[i], AY[i]);
        if ACARRY[i] == 0 {
            if FOOD[c] > 0 {
                FOOD[c] -= 1;
                refresh_cell(c);
                ACARRY[i] = 1;
                ASTEP[i] = 0;
                ADIR[i] = (ADIR[i] + 4) & 7;
                return;
            }
            ASTEP[i] += 1;
            if ASTEP[i] > 2500 {
                // Lost: head home and start over.
                ACARRY[i] = 2;
            }
            if ADET[i] > 0 {
                ADET[i] -= 1;
            } else if (TICK + i as u32) & 3 == 0 {
                sense(i);
            }
            try_move(i);
            return;
        }
        // Carrying food (or lost): home. Food carriers mark the way, the
        // scent strongest nearest the food.
        if ACARRY[i] == 1 && TER[c] == T_GRASS {
            let s = 255 - (if ASTEP[i] / 2 > 235 { 235 } else { ASTEP[i] / 2 });
            if (TRAIL[c] as u32) < s {
                TRAIL[c] = s as u8;
                refresh_cell(c);
            }
        }
        ASTEP[i] += 1;
        let dx = NEST_X - AX[i];
        let dy = NEST_Y - AY[i];
        if dx * dx + dy * dy < 64 {
            if ACARRY[i] == 1 {
                STORE += 1;
                GATHERED += 1;
            }
            ACARRY[i] = 0;
            ASTEP[i] = 0;
            ADIR[i] = random_range(0, 8);
            return;
        }
        if ADET[i] > 0 {
            ADET[i] -= 1;
        } else {
            ADIR[i] = dir_to(dx, dy);
            if random_range(0, 3) == 0 {
                ADIR[i] = (ADIR[i] + random_range(0, 3) + 7) & 7;
            }
        }
        try_move(i);
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

fn soldier(i: usize) {
    unsafe {
        let s = nearest_spider(AX[i], AY[i], 140);
        if s >= 0 {
            let s = s as usize;
            let dx = SX[s] - AX[i];
            let dy = SY[s] - AY[i];
            if dx * dx + dy * dy < 36 {
                // Bite.
                if (TICK + i as u32) % 6 == 0 {
                    SHP[s] -= 1;
                    if SHP[s] <= 0 {
                        SALIVE[s] = false;
                        SPIDERS -= 1;
                        set_msg("The soldiers killed a spider.");
                    }
                }
                return;
            }
            if ADET[i] > 0 {
                ADET[i] -= 1;
            } else {
                ADIR[i] = dir_to(dx, dy);
            }
            try_move(i);
            return;
        }
        // Patrol around the nest.
        let dx = NEST_X - AX[i];
        let dy = NEST_Y - AY[i];
        if ADET[i] > 0 {
            ADET[i] -= 1;
        } else if dx * dx + dy * dy > 90 * 90 {
            ADIR[i] = dir_to(dx, dy);
        } else if random_range(0, 8) == 0 {
            ADIR[i] = (ADIR[i] + random_range(0, 3) + 7) & 7;
        }
        if (TICK + i as u32) & 1 == 0 {
            try_move(i);
        }
    }
}

fn spider(s: usize) {
    unsafe {
        if SCOOL[s] > 0 {
            SCOOL[s] -= 1;
        }
        // Hunt the nearest ant in sight.
        let mut target: i32 = -1;
        let mut bd: i32 = 90 * 90;
        for i in 0..MAXA {
            if AKIND[i] != 0 {
                let dx = AX[i] - SX[s];
                let dy = AY[i] - SY[s];
                let d = dx * dx + dy * dy;
                if d < bd {
                    bd = d;
                    target = i as i32;
                }
            }
        }
        if target >= 0 {
            let t = target as usize;
            if bd < 25 && SCOOL[s] == 0 {
                kill_ant(t);
                EATEN += 1;
                SCOOL[s] = 40;
                return;
            }
            SDIR[s] = dir_to(AX[t] - SX[s], AY[t] - SY[s]);
        } else if random_range(0, 20) == 0 {
            SDIR[s] = (SDIR[s] + random_range(0, 3) + 7) & 7;
        }
        if TICK % 3 == 0 {
            let nx = SX[s] + dxt(SDIR[s]);
            let ny = SY[s] + dyt(SDIR[s]);
            if blocked(nx, ny) {
                SDIR[s] = random_range(0, 8);
            } else {
                SX[s] = nx;
                SY[s] = ny;
            }
        }
    }
}

// One step of the colony.
fn tick() {
    unsafe {
        TICK += 1;
        for i in 0..MAXA {
            match AKIND[i] {
                1 => worker(i),
                2 => soldier(i),
                _ => {}
            }
            if AKIND[i] != 0 {
                AAGE[i] += 1;
                if AAGE[i] > ALIFE[i] {
                    kill_ant(i);
                }
            }
        }
        for s in 0..MAXS {
            if SALIVE[s] {
                spider(s);
            }
        }
        // Scent fades: a sixteenth of the grid each tick.
        let part = (TICK % 16) as usize;
        let mut i = part;
        while i < NCELLS {
            if TRAIL[i] > 0 {
                TRAIL[i] = if TRAIL[i] > 3 { TRAIL[i] - 3 } else { 0 };
                refresh_cell(i);
            }
            i += 16;
        }
        // The queen lays an egg when there is food to spare.
        if AUTO && TICK % 90 == 0 && STORE >= 20 {
            if SOLDIERS * 6 < WORKERS && STORE >= 40 {
                if spawn_ant(2) {
                    STORE -= 25;
                }
            } else if spawn_ant(1) {
                STORE -= 10;
            }
        }
        // Everyone eats.
        if TICK % 400 == 0 {
            let need = (WORKERS + SOLDIERS + 9) / 10;
            if STORE >= need {
                STORE -= need;
            } else {
                STORE = 0;
                // Hunger: the oldest ant dies.
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
        // Food grows back; spiders come more often as days pass.
        if TICK % 1500 == 0 {
            let x = random_far_cell_x();
            let y = random_far_cell_y();
            food_pile(x, y, random_range(2, 4), random_range(100, 220) as u32);
            refresh_area(x, y, 4);
        }
        let day = TICK / 1800;
        if TICK % 1800 == 900 && day >= 1 && random_range(0, 3) < 2 {
            spider_from_edge();
            if day >= 4 && random_range(0, 2) == 0 {
                spider_from_edge();
            }
        }
        if WORKERS + SOLDIERS == 0 && STORE < 10 && !OVER {
            OVER = true;
        }
    }
}

// ===================================================== panel and input

static mut MSG: [u8; 60] = [0; 60];
static mut MSGLEN: usize = 0;
static mut MSGTIME: u32 = 0;

fn set_msg(s: &str) {
    unsafe {
        MSGLEN = 0;
        for c in s.bytes() {
            if MSGLEN < 60 {
                MSG[MSGLEN] = c;
                MSGLEN += 1;
            }
        }
        MSGTIME = 150;
    }
}

// Tools and buttons.
const B_FOOD: usize = 0;
const B_ROCK: usize = 1;
const B_WATER: usize = 2;
const B_ERASE: usize = 3;
const B_SPIDER: usize = 4;
const B_WORKER: usize = 5;
const B_SOLDIER: usize = 6;
const B_AUTO: usize = 7;
const B_PAUSE: usize = 8;
const B_SPEED: usize = 9;
const B_NEW: usize = 10;
const B_QUIT: usize = 11;
const NB_BUTTONS: usize = 12;

static mut TOOL: usize = 0;
static mut PAUSED: bool = false;
static mut SPEED: u32 = 1;
static mut BX: [i32; 12] = [0; 12];
static mut BY: [i32; 12] = [0; 12];
static mut BW: [i32; 12] = [0; 12];

fn button_label(b: usize) -> &'static str {
    unsafe {
        match b {
            0 => "1 Food",
            1 => "2 Rock",
            2 => "3 Water",
            3 => "4 Erase",
            4 => "5 Spider",
            5 => "W +Worker 10",
            6 => "S +Soldier 25",
            7 => if AUTO { "A Breed: on " } else { "A Breed: off" },
            8 => if PAUSED { "P Resume" } else { "P Pause " },
            9 => match SPEED {
                1 => "F Speed 1x",
                2 => "F Speed 2x",
                _ => "F Speed 4x",
            },
            10 => "N New",
            _ => "Esc Quit",
        }
    }
}

fn layout_buttons() {
    unsafe {
        let mut x: i32 = 6;
        for b in 0..NB_BUTTONS {
            if b == B_WORKER {
                x += 10;
            }
            if b == B_AUTO {
                // The second row.
                x = 6;
            }
            BX[b] = x;
            BY[b] = if b < B_AUTO { 432 } else { 449 };
            BW[b] = button_label(b).len() as i32 * 8 + 10;
            x += BW[b] + 4;
        }
    }
}

fn draw_button(b: usize) {
    unsafe {
        let on = (b <= B_SPIDER && TOOL == b) || (b == B_PAUSE && PAUSED) || (b == B_AUTO && AUTO);
        let bg = if on { C_BUTTON_ON } else { C_BUTTON };
        fill_rect(BX[b], BY[b], BW[b], 14, bg);
        for x in BX[b]..BX[b] + BW[b] {
            pset(x, BY[b], C_PANEL_HI);
            pset(x, BY[b] + 13, C_PANEL_LO);
        }
        for y in BY[b]..BY[b] + 14 {
            pset(BX[b], y, C_PANEL_HI);
            pset(BX[b] + BW[b] - 1, y, C_PANEL_LO);
        }
        let label = button_label(b);
        // The key letter in yellow.
        let k = label.as_bytes();
        let mut kl: usize = 1;
        if k[0] == b'E' {
            kl = 3;
        }
        let x = text(BX[b] + 5, BY[b] + 3, str_from_bytes(&k[..kl]), C_YELLOW, bg);
        text(x, BY[b] + 3, str_from_bytes(&k[kl..]), C_TEXT, bg);
    }
}

fn draw_panel() {
    fill_rect(0, PH, SW, SH - PH, C_PANEL);
    for x in 0..SW {
        pset(x, PH, C_PANEL_HI);
    }
    for b in 0..NB_BUTTONS {
        draw_button(b);
    }
    draw_stats();
}

static mut STATS_END: i32 = 0;

// The stats line. Text paints its own background, so only what is left
// of a longer previous line is cleared (no flicker).
fn draw_stats() {
    unsafe {
        let y = 468;
        let mut x = text(8, y, "Day ", C_DIM, C_PANEL);
        x = text(x, y, num(TICK / 1800 + 1), C_TEXT, C_PANEL);
        x = text(x, y, "  Food ", C_DIM, C_PANEL);
        x = text(x, y, num(STORE), if STORE < 10 { C_RED } else { C_YELLOW }, C_PANEL);
        x = text(x, y, "  Ants ", C_DIM, C_PANEL);
        x = text(x, y, num(WORKERS + SOLDIERS), C_TEXT, C_PANEL);
        x = text(x, y, " (", C_DIM, C_PANEL);
        x = text(x, y, num(WORKERS), C_GREEN, C_PANEL);
        x = text(x, y, " workers, ", C_DIM, C_PANEL);
        x = text(x, y, num(SOLDIERS), C_RED, C_PANEL);
        x = text(x, y, " soldiers)  Spiders ", C_DIM, C_PANEL);
        x = text(x, y, num(SPIDERS), if SPIDERS > 0 { C_RED } else { C_TEXT }, C_PANEL);
        x = text(x, y, "  Gathered ", C_DIM, C_PANEL);
        x = text(x, y, num(GATHERED), C_TEXT, C_PANEL);
        if STATS_END > x {
            fill_rect(x, y, STATS_END - x, 8, C_PANEL);
        }
        STATS_END = x;
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

// The tool under the mouse: food, rock, water, erase (all with a brush),
// or a spider (once per click).
fn use_tool(mx: i32, my: i32, first: bool) {
    unsafe {
        let gx = mx >> 2;
        let gy = my >> 2;
        match TOOL {
            B_FOOD => food_pile(gx, gy, 2, 40),
            B_ROCK => blob(gx, gy, 2, T_ROCK),
            B_WATER => blob(gx, gy, 2, T_WATER),
            B_ERASE => blob(gx, gy, 3, T_GRASS),
            _ => {
                if first {
                    spawn_spider(mx, my);
                }
            }
        }
        refresh_area(gx, gy, 4);
        // Ants caught under new rock or water are moved out of it.
        for i in 0..MAXA {
            if AKIND[i] != 0 && blocked(AX[i], AY[i]) {
                AX[i] = NEST_X;
                AY[i] = NEST_Y;
            }
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

fn erase_all() {
    unsafe {
        for a in 0..MAXA {
            let i = PAGE * MAXA + a;
            if DRAWN[i] {
                let x = DRAWN_X[i];
                let y = DRAWN_Y[i];
                if x >= 2 && x < SW - 2 && y >= 2 && y < PH - 2 {
                    let mut o = ((y - 2) * SW + x - 2) as usize;
                    for _ in 0..5 {
                        copy_bytes(lfb().add(o), BG.as_ptr().add(o), 5);
                        o += SW as usize;
                    }
                } else {
                    restore(x - 2, y - 2, 5, 5);
                }
                DRAWN[i] = false;
            }
        }
        for k in 0..MAXS {
            let s = PAGE * MAXS + k;
            if SDRAWN[s] {
                restore(SDRAWN_X[s] - 5, SDRAWN_Y[s] - 5, 11, 11);
                SDRAWN[s] = false;
            }
        }
        restore(CUR_X[PAGE], CUR_Y[PAGE], 8, 12);
        restore(0, 0, SW, 10);
        restore(170, 200, 300, 40);
    }
}

fn draw_ant(i: usize) {
    unsafe {
        let x = AX[i];
        let y = AY[i];
        let d = ADIR[i];
        let fx = dxt(d);
        let fy = dyt(d);
        let soldier = AKIND[i] == 2;
        let body = if soldier { C_SOLDIER } else { C_ANT };
        let hi = if soldier { C_SOLDIER_HI } else { C_ANT_HI };
        let r = PAGE * MAXA + i;
        DRAWN_X[r] = x;
        DRAWN_Y[r] = y;
        DRAWN[r] = true;
        if x >= 2 && x < SW - 2 && y >= 2 && y < PH - 2 {
            let v = lfb().add((y * SW + x) as usize);
            let f: i32 = fy * SW + fx;
            let side: i32 = fx * SW - fy;
            let back: i32 = 0 - f;
            let back2: i32 = back * 2;
            let other: i32 = 0 - side;
            *v.offset(back) = body;
            *v.offset(back2) = body;
            *v = hi;
            *v.offset(f) = body;
            *v.offset(side) = C_ANT;
            *v.offset(other) = C_ANT;
            if soldier {
                *v.offset(f * 2) = hi;
            }
            if ACARRY[i] == 1 {
                *v.offset(f * 2) = C_CRUMB;
            }
            return;
        }
        // Abdomen, thorax, head along the heading; legs to the sides.
        spot(x - fx, y - fy, body);
        spot(x - fx * 2, y - fy * 2, body);
        spot(x, y, hi);
        spot(x + fx, y + fy, body);
        spot(x - fy, y + fx, C_ANT);
        spot(x + fy, y - fx, C_ANT);
        if soldier {
            spot(x + fx * 2, y + fy * 2, hi);
        }
        if ACARRY[i] == 1 {
            spot(x + fx * 2, y + fy * 2, C_CRUMB);
        }
    }
}

fn draw_all() {
    unsafe {
        sprite(NEST_X - 5, NEST_Y - 5, 10, 9, queen_art(), true);
        for i in 0..MAXA {
            if AKIND[i] != 0 {
                draw_ant(i);
            }
        }
        for s in 0..MAXS {
            if SALIVE[s] {
                // Legs move as it walks.
                let x = SX[s];
                let y = SY[s];
                sprite(x - 5, y - 4, 11, 9, spider_art(), true);
                if (x + y) & 2 == 0 {
                    spot(x - 5, y, C_SPIDER_LEG);
                    spot(x + 5, y, C_SPIDER_LEG);
                }
                let r = PAGE * MAXS + s;
                SDRAWN_X[r] = x;
                SDRAWN_Y[r] = y;
                SDRAWN[r] = true;
            }
        }
        if MSGTIME > 0 {
            float_text(8, 1, str_from_bytes(&MSG[..MSGLEN]), C_YELLOW);
            MSGTIME -= 1;
        }
        if PAUSED {
            float_text(SW - 8 * 8, 1, "PAUSED", C_WHITE);
        }
        if OVER {
            float_text(200, 206, "The colony has died out.", C_RED);
            float_text(176, 222, "Press N or click New to start again.", C_WHITE);
        }
        CUR_X[PAGE] = MX;
        CUR_Y[PAGE] = MY;
        sprite(MX, MY, 8, 12, cursor_art(), false);
    }
}

fn wait_retrace() {
    while inb(0x3DA) & 8 != 0 {}
    while inb(0x3DA) & 8 == 0 {}
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
        TICK = 0;
        STORE = 40;
        OVER = false;
        PAUSED = false;
        new_world();
        for _ in 0..24 {
            spawn_ant(1);
        }
        spawn_ant(2);
        spawn_ant(2);
        draw_panel();
        set_msg("Welcome! Guide your colony: place food, build walls, breed ants.");
    }
}

fn set_vbe_mode() -> bool {
    let mut r = Regs::new();
    r.eax = 0x4F02;
    r.ebx = 0x4101; // 640x480x256 with the linear framebuffer
    dos_int(0x10, &mut r);
    r.eax & 0xFFFF == 0x004F
}

fn main() {
    if !set_vbe_mode() {
        println!("Ant Colony needs a VBE video card with 640x480 in 256 colours.");
        return;
    }
    seed_random(ticks());
    make_noise();
    load_font();
    palette();
    layout_buttons();
    new_game();
    let mut quit = false;
    let mut frame: u32 = 0;
    while !quit {
        read_mouse();
        unsafe {
            // Keys.
            while key_pressed() {
                let k = read_key();
                let a = (k & 0xFF) as u8;
                let scan = (k >> 8) & 0xFF;
                if scan == 1 {
                    quit = true;
                }
                let b: usize = match a.to_ascii_lowercase() {
                    b'1' => 0,
                    b'2' => 1,
                    b'3' => 2,
                    b'4' => 3,
                    b'5' => 4,
                    b'w' => 5,
                    b's' => 6,
                    b'a' => 7,
                    b'p' => 8,
                    b' ' => 8,
                    b'f' => 9,
                    b'n' => 10,
                    _ => 99,
                };
                if b < NB_BUTTONS && press_button(b) {
                    quit = true;
                }
            }
            // Mouse: buttons on a click, tools while the button is held.
            let click = MB & 1 != 0 && MB_OLD & 1 == 0;
            if MY >= PH {
                if click {
                    for b in 0..NB_BUTTONS {
                        if MX >= BX[b] && MX < BX[b] + BW[b] && MY >= BY[b] && MY < BY[b] + 14 {
                            if press_button(b) {
                                quit = true;
                            }
                        }
                    }
                }
            } else if MB & 1 != 0 && (click || frame % 3 == 0) {
                use_tool(MX, MY, click);
            } else if MB & 2 != 0 && frame % 3 == 0 {
                // Right button: erase.
                let t = TOOL;
                TOOL = B_ERASE;
                use_tool(MX, MY, false);
                TOOL = t;
            }
            if !PAUSED && !OVER {
                for _ in 0..SPEED {
                    tick();
                }
            }
            if frame % 8 == 0 {
                draw_stats();
            }
        }
        // Redraw the moving things on the hidden page, then show it.
        erase_all();
        draw_all();
        wait_retrace();
        flip();
        frame += 1;
    }
    set_video_mode(3);
    unsafe {
        print!("Ant Colony: ");
        print!("{} ants were born, ", BORN);
        print!("{} crumbs of food were gathered ", GATHERED);
        println!("and spiders ate {}.", EATEN);
    }
}
