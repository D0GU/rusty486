//! 486 paging: two-level page tables, accessed/dirty bits, user/supervisor
//! and read/write protection (with CR0.WP), page faults and a TLB.

use super::*;

pub(crate) const TLB_SIZE: usize = 1024;

// TLB data bits (alongside the physical page frame).
const T_WRITE: u32 = 1 << 1;
const T_USER: u32 = 1 << 2;
const T_DIRTY: u32 = 1 << 6;

const P_PRESENT: u32 = 1;
const P_WRITE: u32 = 2;
const P_USER: u32 = 4;
const P_ACCESSED: u32 = 0x20;
const P_DIRTY: u32 = 0x40;

impl Cpu {
    pub fn tlb_flush(&mut self) {
        self.tlb_tag.iter_mut().for_each(|t| *t = 0);
    }

    pub(crate) fn tlb_flush_page(&mut self, lin: u32) {
        let i = ((lin >> 12) as usize) % TLB_SIZE;
        self.tlb_tag[i] = 0;
    }

    /// Translate a linear address to physical, checking permissions.
    pub(crate) fn translate<B: Bus>(&mut self, bus: &mut B, lin: u32, write: bool, user: bool) -> R<u32> {
        let page = lin >> 12;
        let i = (page as usize) % TLB_SIZE;
        if self.tlb_tag[i] == (page << 1 | 1) {
            let d = self.tlb_data[i];
            let ok = (!user || d & T_USER != 0)
                && (!write || ((d & T_WRITE != 0 || (!user && self.cr0 & CR0_WP == 0)) && d & T_DIRTY != 0));
            if ok {
                return Ok((d & !0xFFF) | (lin & 0xFFF));
            }
        }
        self.walk(bus, lin, write, user, i)
    }

    fn walk<B: Bus>(&mut self, bus: &mut B, lin: u32, write: bool, user: bool, slot: usize) -> R<u32> {
        let err = |present: bool| {
            Fault(14, present as u32 | (write as u32) << 1 | (user as u32) << 2)
        };
        let pde_addr = (self.cr3 & !0xFFF) + (lin >> 22) * 4;
        let pde = bus.read32(pde_addr);
        if pde & P_PRESENT == 0 {
            self.cr2 = lin;
            return Err(err(false));
        }
        let pte_addr = (pde & !0xFFF) + ((lin >> 12) & 0x3FF) * 4;
        let pte = bus.read32(pte_addr);
        if pte & P_PRESENT == 0 {
            self.cr2 = lin;
            return Err(err(false));
        }
        let user_ok = pde & pte & P_USER != 0;
        let write_ok = pde & pte & P_WRITE != 0;
        if (user && !user_ok) || (write && !write_ok && (user || self.cr0 & CR0_WP != 0)) {
            self.cr2 = lin;
            return Err(err(true));
        }
        if pde & P_ACCESSED == 0 {
            bus.write32(pde_addr, pde | P_ACCESSED);
        }
        let mut new_pte = pte | P_ACCESSED;
        if write {
            new_pte |= P_DIRTY;
        }
        if new_pte != pte {
            bus.write32(pte_addr, new_pte);
        }
        let mut data = pte & !0xFFF;
        if write_ok {
            data |= T_WRITE;
        }
        if user_ok {
            data |= T_USER;
        }
        if new_pte & P_DIRTY != 0 {
            data |= T_DIRTY;
        }
        self.tlb_tag[slot] = (lin >> 12) << 1 | 1;
        self.tlb_data[slot] = data;
        Ok((pte & !0xFFF) | (lin & 0xFFF))
    }

    /// Read `sz` bits at a linear address.
    pub(crate) fn lin_read<B: Bus>(&mut self, bus: &mut B, lin: u32, sz: u32, user: bool) -> R<u32> {
        if self.cr0 & CR0_PG == 0 {
            return Ok(match sz {
                8 => bus.read8(lin) as u32,
                16 => bus.read16(lin) as u32,
                _ => bus.read32(lin),
            });
        }
        let n = sz / 8;
        if (lin & 0xFFF) + n <= 0x1000 {
            let p = self.translate(bus, lin, false, user)?;
            return Ok(match sz {
                8 => bus.read8(p) as u32,
                16 => bus.read16(p) as u32,
                _ => bus.read32(p),
            });
        }
        // Crosses a page boundary: translate both pages first, then read.
        let p0 = self.translate(bus, lin, false, user)?;
        let p1 = self.translate(bus, (lin | 0xFFF).wrapping_add(1), false, user)?;
        let mut v = 0u32;
        for k in 0..n {
            let l = lin.wrapping_add(k);
            let p = if (l ^ lin) & !0xFFF == 0 { p0 + k } else { p1 + (l & 0xFFF) };
            v |= (bus.read8(p) as u32) << (8 * k);
        }
        Ok(v)
    }

    /// Write `sz` bits at a linear address.
    pub(crate) fn lin_write<B: Bus>(&mut self, bus: &mut B, lin: u32, sz: u32, v: u32, user: bool) -> R<()> {
        if self.cr0 & CR0_PG == 0 {
            match sz {
                8 => bus.write8(lin, v as u8),
                16 => bus.write16(lin, v as u16),
                _ => bus.write32(lin, v),
            }
            return Ok(());
        }
        let n = sz / 8;
        if (lin & 0xFFF) + n <= 0x1000 {
            let p = self.translate(bus, lin, true, user)?;
            match sz {
                8 => bus.write8(p, v as u8),
                16 => bus.write16(p, v as u16),
                _ => bus.write32(p, v),
            }
            return Ok(());
        }
        let p0 = self.translate(bus, lin, true, user)?;
        let p1 = self.translate(bus, (lin | 0xFFF).wrapping_add(1), true, user)?;
        for k in 0..n {
            let l = lin.wrapping_add(k);
            let p = if (l ^ lin) & !0xFFF == 0 { p0 + k } else { p1 + (l & 0xFFF) };
            bus.write8(p, (v >> (8 * k)) as u8);
        }
        Ok(())
    }
}
