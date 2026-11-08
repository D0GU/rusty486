//! 8237A DMA controller (channels 0-3, 8-bit), as used by the Sound Blaster.
//!
//! Ports: address/count 00h-07h, status/command 08h, request 09h, single
//! mask 0Ah, mode 0Bh, clear flip-flop 0Ch, master clear 0Dh, clear mask 0Eh,
//! write all masks 0Fh; page registers 87h, 83h, 81h, 82h.

#[derive(Clone, Debug, Default)]
pub struct DmaChannel {
    pub base_addr: u16,
    pub base_count: u16,
    pub addr: u16,
    pub count: u16,
    pub page: u8,
    pub mode: u8,
    pub masked: bool,
    /// Terminal count reached (status bit).
    pub tc: bool,
}

#[derive(Clone, Debug)]
pub struct Dma {
    pub ch: [DmaChannel; 4],
    flip_flop: bool,
}

impl Default for Dma {
    fn default() -> Self {
        let mut d = Dma { ch: Default::default(), flip_flop: false };
        for c in &mut d.ch {
            c.masked = true;
        }
        d
    }
}

impl Dma {
    pub fn handles(port: u16) -> bool {
        port <= 0x0F || matches!(port, 0x81 | 0x82 | 0x83 | 0x87)
    }

    fn page_channel(port: u16) -> usize {
        match port {
            0x87 => 0,
            0x83 => 1,
            0x81 => 2,
            _ => 3,
        }
    }

    pub fn read(&mut self, port: u16) -> u8 {
        match port {
            0x00..=0x07 => {
                let c = &self.ch[(port / 2) as usize];
                let v = if port & 1 == 0 { c.addr } else { c.count };
                let b = if self.flip_flop { (v >> 8) as u8 } else { v as u8 };
                self.flip_flop = !self.flip_flop;
                b
            }
            0x08 => {
                let mut s = 0;
                for (i, c) in self.ch.iter_mut().enumerate() {
                    if c.tc {
                        s |= 1 << i;
                        c.tc = false;
                    }
                }
                s
            }
            0x81 | 0x82 | 0x83 | 0x87 => self.ch[Self::page_channel(port)].page,
            _ => 0xFF,
        }
    }

    pub fn write(&mut self, port: u16, v: u8) {
        match port {
            0x00..=0x07 => {
                let c = &mut self.ch[(port / 2) as usize];
                let (base, cur) = if port & 1 == 0 { (&mut c.base_addr, &mut c.addr) } else { (&mut c.base_count, &mut c.count) };
                *base = if self.flip_flop { (*base & 0xFF) | (v as u16) << 8 } else { (*base & 0xFF00) | v as u16 };
                *cur = *base;
                self.flip_flop = !self.flip_flop;
            }
            0x0A => self.ch[(v & 3) as usize].masked = v & 4 != 0,
            0x0B => self.ch[(v & 3) as usize].mode = v,
            0x0C => self.flip_flop = false,
            0x0D => {
                self.flip_flop = false;
                for c in &mut self.ch {
                    c.masked = true;
                }
            }
            0x0E => {
                for c in &mut self.ch {
                    c.masked = false;
                }
            }
            0x0F => {
                for (i, c) in self.ch.iter_mut().enumerate() {
                    c.masked = v & (1 << i) != 0;
                }
            }
            0x81 | 0x82 | 0x83 | 0x87 => self.ch[Self::page_channel(port)].page = v,
            _ => {}
        }
    }

    /// Device reads one byte from memory on channel `n` (memory -> device).
    /// Returns None if the channel is masked. The bool is "terminal count".
    pub fn read_byte(&mut self, n: usize, ram: &[u8]) -> Option<(u8, bool)> {
        let c = &mut self.ch[n];
        if c.masked {
            return None;
        }
        let phys = (c.page as usize) << 16 | c.addr as usize;
        let b = ram.get(phys).copied().unwrap_or(0x80);
        if c.mode & 0x20 != 0 {
            c.addr = c.addr.wrapping_sub(1);
        } else {
            c.addr = c.addr.wrapping_add(1);
        }
        let (count, wrapped) = c.count.overflowing_sub(1);
        c.count = count;
        if wrapped {
            c.tc = true;
            if c.mode & 0x10 != 0 {
                // Auto-initialize.
                c.addr = c.base_addr;
                c.count = c.base_count;
            } else {
                c.masked = true;
            }
        }
        Some((b, wrapped))
    }
}
