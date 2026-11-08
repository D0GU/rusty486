//! The interface between the CPU and the rest of the machine.

/// Everything the CPU can talk to: physical memory, I/O ports and the
/// interrupt controller. Addresses are physical (after segmentation/paging).
pub trait Bus {
    fn read8(&mut self, addr: u32) -> u8;
    fn write8(&mut self, addr: u32, value: u8);

    fn read16(&mut self, addr: u32) -> u16 {
        u16::from_le_bytes([self.read8(addr), self.read8(addr.wrapping_add(1))])
    }
    fn read32(&mut self, addr: u32) -> u32 {
        self.read16(addr) as u32 | (self.read16(addr.wrapping_add(2)) as u32) << 16
    }
    fn write16(&mut self, addr: u32, value: u16) {
        let [a, b] = value.to_le_bytes();
        self.write8(addr, a);
        self.write8(addr.wrapping_add(1), b);
    }
    fn write32(&mut self, addr: u32, value: u32) {
        self.write16(addr, value as u16);
        self.write16(addr.wrapping_add(2), (value >> 16) as u16);
    }

    fn io_read8(&mut self, port: u16) -> u8;
    fn io_write8(&mut self, port: u16, value: u8);

    fn io_read16(&mut self, port: u16) -> u16 {
        u16::from_le_bytes([self.io_read8(port), self.io_read8(port.wrapping_add(1))])
    }
    fn io_read32(&mut self, port: u16) -> u32 {
        self.io_read16(port) as u32 | (self.io_read16(port.wrapping_add(2)) as u32) << 16
    }
    fn io_write16(&mut self, port: u16, value: u16) {
        let [a, b] = value.to_le_bytes();
        self.io_write8(port, a);
        self.io_write8(port.wrapping_add(1), b);
    }
    fn io_write32(&mut self, port: u16, value: u32) {
        self.io_write16(port, value as u16);
        self.io_write16(port.wrapping_add(2), (value >> 16) as u16);
    }

    /// Copy `len` bytes of physical memory from `src` to `dst` in one go, if
    /// both ranges are plain memory (RAM or the linear framebuffer) and don't
    /// overlap. Returns false (having done nothing) otherwise. Used to speed
    /// up REP MOVS.
    fn copy_block(&mut self, _dst: u32, _src: u32, _len: usize) -> bool {
        false
    }

    /// Fill `len` bytes at `dst` with the little-endian `pattern` of `step`
    /// bytes (1, 2 or 4), if the range is plain memory. For REP STOS.
    fn fill_block(&mut self, _dst: u32, _pattern: u32, _step: usize, _len: usize) -> bool {
        false
    }

    /// Bring time-based devices up to CPU cycle `now`. The CPU calls this
    /// before every I/O port access, so devices that are only ticked
    /// lazily still read back exact counter and status values.
    fn sync(&mut self, _now: u64) {}

    /// Called when the CPU is ready to take a maskable interrupt (IF=1).
    /// Returns the vector number if an IRQ is pending, acknowledging it.
    fn irq_ack(&mut self) -> Option<u8> {
        None
    }
}
