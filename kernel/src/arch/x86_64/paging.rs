use bitflags::bitflags;
use core::{arch::asm, ptr::null_mut};

use crate::memory::{
    GIB, KIB, MIB, PAGE_SIZE, PhysAddr, VirtAddr, VmCache, VmProtection,
    buddy::{self, get_page},
    hhdm_offset,
};

type Pte = u64;

const PTE_ADDR_MASK: u64 = 0x000F_FFFF_FFFF_F000;

const PAGE_4KIB: usize = 4 * KIB;
const PAGE_2MIB: usize = 2 * MIB;
const PAGE_1GIB: usize = 1 * GIB;

bitflags! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
    struct PteFlags: u64 {
        const PRESENT = 1 << 0;
        const WRITE = 1 << 1;
        const USER = 1 << 2;
        const ACCESSED = 1 << 5;
        const DIRTY = 1 << 6;
        const HUGE = 1 << 7;
        const GLOBAL = 1 << 8;
        const NX = 1 << 63;
    }
}

impl PteFlags {
    pub fn dir_from(vaddr: VirtAddr, prot: VmProtection, cache: VmCache) -> Self {
        let mut flags = PteFlags::PRESENT;

        if !prot.contains(VmProtection::READ) {
            panic!("No-read mapping is not supported on x86_64");
        }
        if prot.contains(VmProtection::WRITE) {
            flags |= PteFlags::WRITE;
        }
        if !prot.contains(VmProtection::EXECUTE) {
            flags |= PteFlags::NX;
        }
        if vaddr.as_usize() < hhdm_offset() {
            flags |= PteFlags::USER;
        }

        flags
    }

    pub fn leaf_from(vaddr: VirtAddr, prot: VmProtection, cache: VmCache, huge: bool) -> Self {
        let mut flags = PteFlags::PRESENT;

        if !prot.contains(VmProtection::READ) {
            panic!("No-read mapping is not supported on x86_64");
        }
        if prot.contains(VmProtection::WRITE) {
            flags |= PteFlags::WRITE;
        }
        if !prot.contains(VmProtection::EXECUTE) {
            flags |= PteFlags::NX;
        }
        if vaddr.as_usize() < hhdm_offset() {
            flags |= PteFlags::USER;
        } else {
            flags |= PteFlags::GLOBAL;
        }
        if huge {
            flags |= PteFlags::HUGE;
        }

        flags
    }
}

pub struct Map {
    pml4: *mut Pte,
}

static mut HIGHER_HALF_ENTRIES: [Pte; 256] = [0; 256];

impl Map {
    pub fn new() -> Option<Self> {
        let table = unsafe { buddy::alloc(0)?.as_ref() };
        let table_phys = table.addr.as_usize();
        let table_virt = table_phys + hhdm_offset();

        unsafe {
            core::ptr::write_bytes(table_virt as *mut u8, 0, PAGE_SIZE);

            // Copy shared higher-half entries into PML4[256..512].
            let pml4 = table_virt as *mut Pte;
            for i in 0..256 {
                *pml4.add(256 + i) = HIGHER_HALF_ENTRIES[i];
            }

            Some(Self { pml4 })
        }
    }

    pub fn map(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        size: usize,
        prot: VmProtection,
        cache: VmCache,
    ) -> Option<()> {
        debug_assert!(vaddr.as_usize() % size == 0);
        debug_assert!(paddr.as_usize() % size == 0);

        let dir_flags = PteFlags::dir_from(vaddr, prot, cache);
        let leaf_flags: PteFlags;

        let target_level;
        match size {
            PAGE_4KIB => {
                // 4 KiB
                target_level = 0;
                leaf_flags = PteFlags::leaf_from(vaddr, prot, cache, false);
            }
            PAGE_2MIB => {
                // 2 MiB
                target_level = 1;
                leaf_flags = PteFlags::leaf_from(vaddr, prot, cache, true);
            }
            PAGE_1GIB => {
                // 1 GiB
                target_level = 2;
                leaf_flags = PteFlags::leaf_from(vaddr, prot, cache, true);
            }
            _ => panic!("Invalid page size: {:#x}", size),
        }

        let table_entries: [usize; 4] = [
            (vaddr.as_usize() >> 12) & 0x1FF, // PML1 entry
            (vaddr.as_usize() >> 21) & 0x1FF, // PML2 entry
            (vaddr.as_usize() >> 30) & 0x1FF, // PML3 entry
            (vaddr.as_usize() >> 39) & 0x1FF, // PML4 entry
        ];

        let mut allocated_tables = [null_mut(); 4];

        let mut current_table = self.pml4;
        for i in (target_level + 1..=3).rev() {
            match get_next_level(current_table, table_entries[i], i, true, dir_flags) {
                Some((next_table, allocated)) => {
                    current_table = next_table;
                    if allocated {
                        allocated_tables[i] = next_table;
                    }
                }
                None => {
                    // Rollback.
                    for j in i + 1..=3 {
                        if allocated_tables[j] == null_mut() {
                            continue;
                        }

                        buddy::free(
                            buddy::get_page(PhysAddr(allocated_tables[j] as usize - hhdm_offset()))
                                .unwrap(),
                        );
                    }

                    return None;
                }
            }
        }

        unsafe {
            let entry_ptr = current_table.add(table_entries[target_level]);

            let old_flags = PteFlags::from_bits_truncate(*entry_ptr);
            let is_present = old_flags.contains(PteFlags::PRESENT);
            let is_huge = old_flags.contains(PteFlags::HUGE);

            if is_present && !is_huge && target_level != 0 {
                // Destroy subtree.
            }

            *entry_ptr = (paddr.as_usize() as u64) | leaf_flags.bits();

            if !is_present {
                let page = get_page(PhysAddr(current_table as usize - hhdm_offset()))
                    .unwrap()
                    .as_ref();
                page.inc_children();
            }
        }

        Some(())
    }

    pub fn map_range(
        &mut self,
        vaddr: VirtAddr,
        paddr: PhysAddr,
        length: usize,
        prot: VmProtection,
        cache: VmCache,
    ) -> Option<()> {
        debug_assert!(
            vaddr.as_usize() % PAGE_SIZE == 0
                && paddr.as_usize() % PAGE_SIZE == 0
                && length % PAGE_SIZE == 0
        );

        let mut offset: usize = 0;
        while offset < length {
            let curr_vaddr = vaddr + offset;
            let curr_paddr = paddr + offset;
            let remaining = length - offset;

            let size = [PAGE_1GIB, PAGE_2MIB, PAGE_4KIB]
                .into_iter()
                .find(|&page_size| {
                    remaining >= page_size
                        && curr_vaddr.as_usize() % page_size == 0
                        && curr_paddr.as_usize() % page_size == 0
                })
                .expect("No valid page size found for mapping");

            self.map(curr_vaddr, curr_paddr, size, prot, cache)?;

            offset += size;
        }

        Some(())
    }

    pub fn unmap(&mut self, vaddr: VirtAddr) {}

    pub fn unmap_range(&mut self, vaddr: VirtAddr, pages: usize) {
        for i in 0..pages {
            self.unmap(vaddr + i * PAGE_SIZE);
        }
    }

    pub fn load_map(&mut self) {
        let cr3_value = self.pml4 as usize - hhdm_offset();

        unsafe {
            asm!("mov cr3, {}", in(reg) cr3_value, options(nostack, preserves_flags));
        }
    }
}

fn get_next_level(
    table: *mut Pte,
    index: usize,
    level: usize,
    alloc: bool,
    flags: PteFlags,
) -> Option<(*mut Pte, bool)> {
    let entry = unsafe { *table.add(index) };

    let entry_flags = PteFlags::from_bits_truncate(entry);

    if entry_flags.contains(PteFlags::PRESENT) {
        if entry_flags.contains(PteFlags::HUGE) {
            let new_entry = split_huge_pte(entry, level)?;

            unsafe {
                *table.add(index) = new_entry;
            }

            let next = ((new_entry & PTE_ADDR_MASK) as usize + hhdm_offset()) as *mut Pte;
            return Some((next, false));
        }

        let next = ((entry & PTE_ADDR_MASK) as usize + hhdm_offset()) as *mut Pte;
        return Some((next, false));
    }

    if !alloc {
        return None;
    }

    let page = unsafe { buddy::alloc(0)?.as_ref() };
    let page_phys = page.addr.as_usize();
    let page_virt = page_phys + hhdm_offset();

    unsafe {
        core::ptr::write_bytes(page_virt as *mut u8, 0, PAGE_SIZE);
        *table.add(index) = (page_phys as u64) | flags.bits();
    }

    let parent_block = unsafe {
        buddy::get_page(PhysAddr(table as usize - hhdm_offset()))
            .unwrap()
            .as_ref()
    };
    parent_block.inc_children();

    Some((page_virt as *mut Pte, true))
}

fn split_huge_pte(entry: Pte, level: usize) -> Option<Pte> {
    let flags = PteFlags::from_bits_truncate(entry);

    debug_assert!(flags.contains(PteFlags::PRESENT) && flags.contains(PteFlags::HUGE));

    let child_size = match level {
        2 => PAGE_2MIB,
        1 => PAGE_4KIB,
        _ => panic!("Invalid huge PTE level!"),
    };

    let page = unsafe { buddy::alloc(0)?.as_ref() };
    let page_phys = page.addr.as_usize();
    let page_virt = page_phys + hhdm_offset();

    unsafe {
        core::ptr::write_bytes(page_virt as *mut u8, 0, PAGE_SIZE);
    }

    let base_phys = (entry & PTE_ADDR_MASK) as usize;

    let mut child_flags = flags;
    if level == 1 {
        child_flags.remove(PteFlags::HUGE);
    }

    for i in 0..512 {
        let child_phys = base_phys + i * child_size;
        unsafe { *((page_virt as *mut Pte).add(i)) = (child_phys as u64) | child_flags.bits() }
    }

    page.set_children(512);

    let mut table_flags = flags;
    table_flags.remove(PteFlags::HUGE);
    table_flags.remove(PteFlags::GLOBAL);
    table_flags.remove(PteFlags::DIRTY);

    Some((page_phys as u64) | table_flags.bits())
}

pub fn init() {
    for i in 0..256 {
        // Pre-allocate PML3 tables.
        let table = unsafe {
            buddy::alloc(0)
                .expect("Could not preallocate PML3 tables for higher half entries!")
                .as_ref()
        };
        let table_phys = table.addr.as_usize();
        let table_virt = table_phys + hhdm_offset();

        unsafe {
            core::ptr::write_bytes(table_virt as *mut u8, 0, PAGE_SIZE);
        }

        let flags = PteFlags::PRESENT | PteFlags::WRITE;
        unsafe {
            HIGHER_HALF_ENTRIES[i] = table_phys as u64 | flags.bits();
        }
    }
}
