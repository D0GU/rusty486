// 320x200 graphics in mode 13h: a moving pattern until a key is pressed.

fn main() {
    set_video_mode(0x13);
    let screen = 0xA0000 as *mut u8;
    let mut t: u32 = 0;
    while !key_pressed() {
        for y in 0..200u32 {
            for x in 0..320u32 {
                let c = ((x ^ y) + t) & 0xFF;
                unsafe {
                    *screen.add((y * 320 + x) as usize) = c as u8;
                }
            }
        }
        t += 3;
    }
    read_key();
    set_video_mode(3);
    beep(880, 100);
    println!("Back in text mode after {} frames.", t / 3);
}
