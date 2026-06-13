use core::ptr::null_mut;
use core::sync::atomic::{AtomicU16, Ordering};

use intrusive_collections::{LinkedList, LinkedListAtomicLink, intrusive_adapter};
use limine::memory_map::EntryType;

use crate::boot::MEMORYMAP_REQUEST;
use crate::memory::{PAGE_SIZE, PhysAddr, hhdm_offset};
use crate::println;
use crate::sync::spinlock::SpinLock;

pub const MAX_BLOCK_ORDER: usize = 10;

pub struct Block {
    pub addr: PhysAddr,

    pub mapcount: AtomicU16,
    pub children: AtomicU16,

    order: usize,
    free: bool,
    link: LinkedListAtomicLink,
}

impl Block {
    pub fn inc_children(&self) {
        self.children.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_children(&self) -> bool {
        let old = self.children.fetch_sub(1, Ordering::Relaxed);

        old == 1
    }

    pub fn set_children(&self, count: u16) {
        self.children.store(count, Ordering::Relaxed);
    }
}

intrusive_adapter!(BlockAdapter = &'static Block: Block { link => LinkedListAtomicLink });

pub struct BuddyAllocator {
    blocks: &'static mut [Block],
    block_count: usize,
    levels: [LinkedList<BlockAdapter>; MAX_BLOCK_ORDER + 1],
}

impl BuddyAllocator {
    pub fn new(blocks: &'static mut [Block]) -> Self {
        let block_count = blocks.len();
        let levels = core::array::from_fn(|_| LinkedList::new(BlockAdapter::new()));

        Self {
            blocks,
            block_count,
            levels,
        }
    }
}

static BUDDY_ALLOCATOR: SpinLock<Option<BuddyAllocator>> = SpinLock::new(None);

pub fn init() {
    let memory_map_response = MEMORYMAP_REQUEST
        .get_response()
        .expect("Memory map request failed");

    if memory_map_response.entries().is_empty() {
        panic!("Invalid memory map provided by the bootloader");
    }

    print_memory_map();

    let mut max_phys = 0;
    for entry in memory_map_response.entries() {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        let end = entry.base + entry.length;
        if end > max_phys {
            max_phys = end;
        }
    }
    assert!(
        max_phys != 0,
        "Bootloader memory map contains no usable memory regions"
    );

    let total_pages = (max_phys as usize) / PAGE_SIZE;

    // Number of bytes reserverd for the page database.
    let page_db_bytes = total_pages * size_of::<Block>();
    let page_db_bytes_aligned = (page_db_bytes + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let mut blocks_base_phys = 0;
    let mut blocks_ptr: *mut Block = null_mut();
    // Find a usable region, large enough to hold the page database.
    for entry in memory_map_response.entries() {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        if (entry.length as usize) >= page_db_bytes_aligned {
            blocks_base_phys = entry.base as usize;
            blocks_ptr = (blocks_base_phys + hhdm_offset()) as *mut Block;
            break;
        }
    }
    assert!(
        !blocks_ptr.is_null(),
        "No usable memory regeion large enough for the page database"
    );

    let blocks = unsafe { core::slice::from_raw_parts_mut(blocks_ptr, total_pages) };

    // Set each block's address and mark them as used for now.
    for i in 0..total_pages {
        blocks[i] = Block {
            addr: PhysAddr(i * PAGE_SIZE),
            order: 0,
            free: false,
            mapcount: AtomicU16::new(0),
            children: AtomicU16::new(0),
            link: LinkedListAtomicLink::new(),
        }
    }

    let mut allocator = BuddyAllocator::new(blocks);

    /*
     * Iterate through each entry and set the blocks corresponding to a usable memory entry as free
     * using greedy.
     */
    for entry in memory_map_response.entries() {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        let mut addr = entry.base as usize;
        let end = (entry.base + entry.length) as usize;
        if entry.base as usize == blocks_base_phys {
            addr += page_db_bytes_aligned;
        }

        let mut order = MAX_BLOCK_ORDER;

        while addr < end {
            let span = PAGE_SIZE << order;

            if addr + span > end || addr % span != 0 {
                order -= 1;
                continue;
            }

            let idx = addr / PAGE_SIZE;
            let block = &mut allocator.blocks[idx];

            block.order = order;
            block.free = true;

            let static_block = unsafe { &mut *(block as *mut Block) };
            allocator.levels[order].push_back(static_block);

            addr += span;
            order = MAX_BLOCK_ORDER;
        }
    }

    *BUDDY_ALLOCATOR.lock() = Some(allocator);

    println!("Buddy allocator initialized.");
}

pub fn alloc(order: usize) -> Option<&'static Block> {
    debug_assert!(order <= MAX_BLOCK_ORDER);

    let mut guard = BUDDY_ALLOCATOR.lock();
    let allocator = guard.as_mut()?;

    let mut i = order;
    while allocator.levels[i].is_empty() {
        i += 1;
        if i > MAX_BLOCK_ORDER {
            return None;
        }
    }

    let block = allocator.levels[i].pop_front()?;
    let idx = block.addr.as_usize() / PAGE_SIZE;
    // Split block if needed.
    while i > order {
        // When splitting we modify the metadata of the block on the right.
        let r_idx = idx ^ (1 << (i - 1));
        let r_block = &mut allocator.blocks[r_idx];
        r_block.order = i - 1;
        r_block.free = true;

        let static_r_block = unsafe { &mut *(r_block as *mut Block) };
        allocator.levels[i - 1].push_back(static_r_block);

        i -= 1;
    }

    let ret = &mut allocator.blocks[idx];
    ret.order = order;
    ret.free = false;
    ret.mapcount.store(0, Ordering::Relaxed);
    ret.children.store(0, Ordering::Relaxed);

    Some(unsafe { &mut *(ret as *mut Block) })
}

pub fn free(block: &'static Block) {
    let mut guard = BUDDY_ALLOCATOR.lock();
    let Some(allocator) = guard.as_mut() else {
        return;
    };

    let mut idx = block.addr.as_usize() / PAGE_SIZE;
    let mut i = block.order;

    // Merge blocks if needed.
    while i < MAX_BLOCK_ORDER {
        let b_idx = idx ^ (1 << i);
        if b_idx >= allocator.block_count {
            break;
        }

        let buddy = &allocator.blocks[b_idx];
        if buddy.free && buddy.order == i {
            unsafe {
                let mut cursor = allocator.levels[i].cursor_mut_from_ptr(buddy as *const Block);
                cursor.remove();
            }

            // We always follow the block on the left.
            if idx > b_idx {
                idx = b_idx;
            }
            i += 1;
        } else {
            break;
        }
    }

    let block = &mut allocator.blocks[idx];
    block.order = i;
    block.free = true;
    block.mapcount.store(0, Ordering::Relaxed);
    block.children.store(0, Ordering::Relaxed);

    let static_block = unsafe { &mut *(block as *mut Block) };
    allocator.levels[i].push_back(static_block);
}

pub fn get_block(addr: PhysAddr) -> Option<&'static Block> {
    let idx = addr.as_usize() / PAGE_SIZE;

    let guard = BUDDY_ALLOCATOR.lock();
    let allocator = guard.as_ref()?;

    if idx >= allocator.block_count {
        return None;
    }

    let block = &allocator.blocks[idx];

    Some(unsafe { &*(block as *const Block) })
}

fn print_memory_map() {
    let memory_map_response = MEMORYMAP_REQUEST
        .get_response()
        .expect("Memory map request failed");

    for entry in memory_map_response.entries() {
        let base = entry.base;
        let length = entry.length;
        let end = base + length;
        let entry_type = entry.entry_type;

        println!(
            "{:<23}: {:#018x} - {:#018x}, size = {:#018x}",
            entry_type_name(entry_type),
            base,
            end,
            length
        );
    }
}

fn entry_type_name(entry_type: EntryType) -> &'static str {
    match entry_type {
        EntryType::USABLE => "USABLE",
        EntryType::RESERVED => "RESERVED",
        EntryType::ACPI_RECLAIMABLE => "ACPI_RECLAIMABLE",
        EntryType::ACPI_NVS => "ACPI_NVS",
        EntryType::BAD_MEMORY => "BAD_MEMORY",
        EntryType::BOOTLOADER_RECLAIMABLE => "BOOTLOADER_RECLAIMABLE",
        EntryType::EXECUTABLE_AND_MODULES => "EXECUTABLE_AND_MODULES",
        EntryType::FRAMEBUFFER => "FRAMEBUFFER",
        _ => "UNKNOWN",
    }
}
