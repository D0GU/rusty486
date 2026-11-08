// Snake in text mode. Arrow keys steer, Esc quits.

const W: i32 = 40;
const H: i32 = 20;
const MAX: usize = 400;

#[derive(Clone, Copy, PartialEq)]
enum Dir {
    Up,
    Down,
    Left,
    Right,
}

struct Snake {
    xs: [i32; MAX],
    ys: [i32; MAX],
    len: usize,
    dir: Dir,
}

impl Snake {
    fn hits(&self, x: i32, y: i32) -> bool {
        for i in 0..self.len {
            if self.xs[i] == x && self.ys[i] == y {
                return true;
            }
        }
        false
    }
}

fn draw_cell(x: i32, y: i32, c: u8, attr: u8) {
    put_char_at((x + 20) as u32, (y + 2) as u32, c, attr);
}

fn place_food(s: &Snake) -> i32 {
    loop {
        let x = random_range(0, W);
        let y = random_range(0, H);
        if !s.hits(x, y) {
            return y * W + x;
        }
    }
}

fn main() {
    clear_screen();
    gotoxy(0, 25);
    for y in -1..=H {
        for x in -1..=W {
            if x < 0 || y < 0 || x == W || y == H {
                draw_cell(x, y, 0xB1, 0x07);
            }
        }
    }
    put_str_at(20, 0, "R++ SNAKE   arrows steer, Esc quits", 0x0E);
    let mut s = Snake { xs: [0; MAX], ys: [0; MAX], len: 3, dir: Dir::Right };
    for i in 0..3 {
        s.xs[i] = 10 - i as i32;
        s.ys[i] = 10;
    }
    let mut food = place_food(&s);
    let mut score = 0;
    loop {
        draw_cell(food % W, food / W, b'*', 0x0C);
        for i in 0..s.len {
            draw_cell(s.xs[i], s.ys[i], if i == 0 { b'@' } else { b'o' }, 0x0A);
        }
        sleep_ms(110);
        while key_pressed() {
            let k = read_key() >> 8;
            match k {
                0x48 if s.dir != Dir::Down => s.dir = Dir::Up,
                0x50 if s.dir != Dir::Up => s.dir = Dir::Down,
                0x4B if s.dir != Dir::Right => s.dir = Dir::Left,
                0x4D if s.dir != Dir::Left => s.dir = Dir::Right,
                0x01 => {
                    clear_screen();
                    println!("Bye! Score: {}", score);
                    return;
                }
                _ => {}
            }
        }
        let mut x = s.xs[0];
        let mut y = s.ys[0];
        match s.dir {
            Dir::Up => y -= 1,
            Dir::Down => y += 1,
            Dir::Left => x -= 1,
            Dir::Right => x += 1,
        }
        if x < 0 || y < 0 || x >= W || y >= H || s.hits(x, y) {
            break;
        }
        let ate = y * W + x == food;
        if ate && s.len < MAX {
            s.len += 1;
            score += 10;
            beep(660, 20);
        } else {
            draw_cell(s.xs[s.len - 1], s.ys[s.len - 1], b' ', 0x07);
        }
        let mut i = s.len - 1;
        while i > 0 {
            s.xs[i] = s.xs[i - 1];
            s.ys[i] = s.ys[i - 1];
            i -= 1;
        }
        s.xs[0] = x;
        s.ys[0] = y;
        if ate {
            food = place_food(&s);
        }
    }
    beep(220, 300);
    gotoxy(0, 23);
    println!("Game over! Score: {}", score);
}
