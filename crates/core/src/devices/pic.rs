//! Dual 8259A programmable interrupt controllers (master at 20h/21h, slave at
//! A0h/A1h cascaded on IRQ2), edge-triggered as on the AT.

#[derive(Clone, Debug, Default)]
struct Chip {
    irr: u8,
    isr: u8,
    imr: u8,
    base: u8,
    /// Which ICW the data port expects next (0 = normal operation).
    icw_step: u8,
    single: bool,
    need_icw4: bool,
    auto_eoi: bool,
    read_isr: bool,
}

impl Chip {
    fn new(base: u8) -> Self {
        Chip { base, imr: 0xFF, ..Default::default() }
    }

    fn write_cmd(&mut self, v: u8) {
        if v & 0x10 != 0 {
            // ICW1
            self.icw_step = 2;
            self.single = v & 0x02 != 0;
            self.need_icw4 = v & 0x01 != 0;
            self.imr = 0;
            self.isr = 0;
            self.irr = 0;
            self.read_isr = false;
        } else if v & 0x08 != 0 {
            // OCW3
            if v & 0x02 != 0 {
                self.read_isr = v & 0x01 != 0;
            }
        } else {
            // OCW2
            match v >> 5 {
                0b001 | 0b101 => {
                    // Non-specific EOI: clear the highest-priority in-service bit.
                    if self.isr != 0 {
                        self.isr &= self.isr - 1;
                    }
                }
                0b011 | 0b111 => self.isr &= !(1 << (v & 7)),
                _ => {}
            }
        }
    }

    fn write_data(&mut self, v: u8) {
        match self.icw_step {
            2 => {
                self.base = v & 0xF8;
                self.icw_step = if !self.single { 3 } else if self.need_icw4 { 4 } else { 0 };
            }
            3 => self.icw_step = if self.need_icw4 { 4 } else { 0 },
            4 => {
                self.auto_eoi = v & 0x02 != 0;
                self.icw_step = 0;
            }
            _ => self.imr = v,
        }
    }

    fn read_cmd(&self) -> u8 {
        if self.read_isr {
            self.isr
        } else {
            self.irr
        }
    }

    /// Highest-priority unmasked request that outranks everything in service.
    fn pending(&self, extra_irr: u8) -> Option<u8> {
        let req = (self.irr | extra_irr) & !self.imr;
        if req == 0 {
            return None;
        }
        let irq = req.trailing_zeros() as u8;
        let in_service = if self.isr == 0 { 8 } else { self.isr.trailing_zeros() as u8 };
        (irq < in_service).then_some(irq)
    }

    fn accept(&mut self, irq: u8) {
        self.irr &= !(1 << irq);
        if !self.auto_eoi {
            self.isr |= 1 << irq;
        }
    }
}

#[derive(Clone, Debug)]
pub struct Pic {
    master: Chip,
    slave: Chip,
}

impl Default for Pic {
    fn default() -> Self {
        Pic { master: Chip::new(0x08), slave: Chip::new(0x70) }
    }
}

impl Pic {
    pub fn handles(port: u16) -> bool {
        matches!(port, 0x20 | 0x21 | 0xA0 | 0xA1)
    }

    /// Signal an interrupt request on line 0-15 (edge).
    pub fn raise(&mut self, irq: u8) {
        if irq < 8 {
            self.master.irr |= 1 << irq;
        } else {
            self.slave.irr |= 1 << (irq - 8);
        }
    }

    fn slave_line(&self) -> u8 {
        if self.slave.pending(0).is_some() {
            1 << 2
        } else {
            0
        }
    }

    /// Whether an interrupt would be delivered if the CPU accepted one.
    pub fn has_pending(&self) -> bool {
        self.master.pending(self.slave_line()).is_some()
    }

    /// Interrupt acknowledge: returns the vector number.
    pub fn ack(&mut self) -> Option<u8> {
        let irq = self.master.pending(self.slave_line())?;
        if irq == 2 && self.master.irr & 4 == 0 {
            let s = self.slave.pending(0)?;
            self.master.accept(2);
            self.slave.accept(s);
            return Some(self.slave.base + s);
        }
        self.master.accept(irq);
        Some(self.master.base + irq)
    }

    pub fn read(&self, port: u16) -> u8 {
        match port {
            0x20 => self.master.read_cmd(),
            0x21 => self.master.imr,
            0xA0 => self.slave.read_cmd(),
            _ => self.slave.imr,
        }
    }

    pub fn write(&mut self, port: u16, v: u8) {
        match port {
            0x20 => self.master.write_cmd(v),
            0x21 => self.master.write_data(v),
            0xA0 => self.slave.write_cmd(v),
            _ => self.slave.write_data(v),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn init(p: &mut Pic) {
        for (port, v) in [(0x20, 0x11), (0x21, 0x08), (0x21, 0x04), (0x21, 0x01), (0xA0, 0x11), (0xA1, 0x70), (0xA1, 0x02), (0xA1, 0x01), (0x21, 0x00), (0xA1, 0x00)] {
            p.write(port, v);
        }
    }

    #[test]
    fn priority_and_eoi() {
        let mut p = Pic::default();
        init(&mut p);
        p.raise(1);
        p.raise(0);
        assert_eq!(p.ack(), Some(0x08));
        // IRQ1 is lower priority than IRQ0 in service.
        assert_eq!(p.ack(), None);
        p.write(0x20, 0x20);
        assert_eq!(p.ack(), Some(0x09));
    }

    #[test]
    fn cascade() {
        let mut p = Pic::default();
        init(&mut p);
        p.raise(12);
        assert_eq!(p.ack(), Some(0x74));
        p.write(0xA0, 0x20);
        p.write(0x20, 0x20);
        assert_eq!(p.ack(), None);
    }

    #[test]
    fn masking() {
        let mut p = Pic::default();
        init(&mut p);
        p.write(0x21, 0x01);
        p.raise(0);
        assert_eq!(p.ack(), None);
        p.write(0x21, 0x00);
        assert_eq!(p.ack(), Some(0x08));
    }
}
