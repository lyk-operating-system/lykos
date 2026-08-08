use core::mem::offset_of;
use core::ptr::{NonNull, null_mut};
use core::sync::atomic::AtomicU16;

use limine::memory_map::EntryType;
use utils::collections::list::{List, ListNode};

use crate::boot::MEMORYMAP_REQUEST;
use crate::memory::page::{MAX_PAGE_ORDER, Page, PageState};
use crate::memory::{PAGE_SIZE, PhysAddr, hhdm_offset};
use crate::println;
use crate::sync::spinlock::SpinLock;

struct PageDb {
    pages: *mut Page,
    page_count: usize,
}

unsafe impl Sync for PageDb {}

static mut PAGE_DATABASE: PageDb = PageDb {
    pages: core::ptr::null_mut(),
    page_count: 0,
};

const PAGE_LIST_NODE_OFF: usize = offset_of!(Page, state.Free.list_node);

struct BuddyAllocator {
    levels: [List<Page, PAGE_LIST_NODE_OFF>; MAX_PAGE_ORDER + 1],
}

impl BuddyAllocator {
    pub const fn new() -> Self {
        const INIT: List<Page, PAGE_LIST_NODE_OFF> = List::new();
        Self {
            levels: [INIT; MAX_PAGE_ORDER + 1],
        }
    }
}

static BUDDY_ALLOCATOR: SpinLock<BuddyAllocator> = SpinLock::new(BuddyAllocator::new());

unsafe impl Send for BuddyAllocator {}

pub fn init() {
    let memory_map_response = MEMORYMAP_REQUEST
        .get_response()
        .expect("Memory map request failed");

    if memory_map_response.entries().is_empty() {
        panic!("Invalid memory map provided by the bootloader");
    }

    print_memory_map();

    /*
     * Find the last physical memory address we need to track.
     * This means skipping BAD_MEMORY and RESERVED.
     */
    let mut max_phys = 0;
    for entry in memory_map_response.entries() {
        if matches!(
            entry.entry_type,
            EntryType::BAD_MEMORY | EntryType::RESERVED
        ) {
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
    max_phys = (max_phys + PAGE_SIZE as u64 - 1) & !(PAGE_SIZE as u64 - 1);

    let page_count = (max_phys as usize) / PAGE_SIZE;

    // Number of bytes reserverd for the page database.
    let page_db_bytes = page_count * size_of::<Page>();
    let page_db_bytes_aligned = (page_db_bytes + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);

    let mut pages_base_phys = 0;
    let mut pages_ptr: *mut Page = null_mut();
    // Find a usable region, large enough to hold the page database.
    for entry in memory_map_response.entries() {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        if (entry.length as usize) >= page_db_bytes_aligned {
            pages_base_phys = entry.base as usize;
            pages_ptr = (pages_base_phys + hhdm_offset()) as *mut Page;
            break;
        }
    }
    assert!(
        !pages_ptr.is_null(),
        "No usable memory regeion large enough for the page database"
    );

    // Set each page's address and mark them as used for now.
    unsafe {
        for i in 0..page_count {
            pages_ptr.add(i).write(Page {
                addr: PhysAddr(i * PAGE_SIZE),
                order: 0,
                state: PageState::Used {
                    mapcount: AtomicU16::new(0),
                    children: AtomicU16::new(0),
                },
            });
        }

        PAGE_DATABASE = PageDb {
            pages: pages_ptr,
            page_count,
        }
    }

    let mut allocator = BuddyAllocator::new();

    /*
     * Iterate through each entry and set the pages corresponding to a usable memory entry as free
     * using greedy.
     */
    for entry in memory_map_response.entries() {
        if entry.entry_type != EntryType::USABLE {
            continue;
        }

        let mut addr = entry.base as usize;
        let end = (entry.base + entry.length) as usize;
        if entry.base as usize == pages_base_phys {
            addr += page_db_bytes_aligned;
        }

        let mut order = MAX_PAGE_ORDER;

        while addr < end {
            let span = PAGE_SIZE << order;

            if addr + span > end || addr % span != 0 {
                order -= 1;
                continue;
            }

            let idx = addr / PAGE_SIZE;

            unsafe {
                let page_ptr = PAGE_DATABASE.pages.add(idx);
                let page = &mut *page_ptr;

                page.order = order;
                page.state = PageState::Free {
                    list_node: ListNode::default(),
                };

                allocator.levels[order].push_end(NonNull::new_unchecked(page_ptr));
            }

            addr += span;
            order = MAX_PAGE_ORDER;
        }
    }

    *BUDDY_ALLOCATOR.lock() = allocator;

    println!("Buddy allocator initialized.");
}

pub fn alloc(order: usize) -> Option<NonNull<Page>> {
    debug_assert!(order <= MAX_PAGE_ORDER);

    let mut allocator = BUDDY_ALLOCATOR.lock();

    let mut i = order;
    while allocator.levels[i].is_empty() {
        i += 1;
        if i > MAX_PAGE_ORDER {
            return None;
        }
    }

    let page = unsafe { allocator.levels[i].pop_front()?.as_mut() };

    let idx = page.addr.as_usize() / PAGE_SIZE;
    // Split page if needed.
    while i > order {
        // When splitting we modify the metadata of the page on the right.
        let r_idx = idx ^ (1 << (i - 1));

        unsafe {
            let r_page_ptr = PAGE_DATABASE.pages.add(r_idx);
            let r_page = &mut *r_page_ptr;
            r_page.order = i - 1;
            r_page.state = PageState::Free {
                list_node: ListNode::default(),
            };

            allocator.levels[i - 1].push_end(NonNull::new_unchecked(r_page_ptr));
        }

        i -= 1;
    }

    unsafe {
        let ret_ptr = PAGE_DATABASE.pages.add(idx);
        let ret = &mut *ret_ptr;
        ret.order = order;
        ret.state = PageState::Used {
            mapcount: AtomicU16::new(0),
            children: AtomicU16::new(0),
        };

        Some(NonNull::new_unchecked(ret_ptr))
    }
}

pub fn free(page: NonNull<Page>) {
    let mut allocator = BUDDY_ALLOCATOR.lock();

    unsafe {
        let mut idx = page.as_ref().addr.as_usize() / PAGE_SIZE;
        let mut i = page.as_ref().order;

        // Merge pages if needed.
        while i < MAX_PAGE_ORDER {
            let b_idx = idx ^ (1 << i);
            if b_idx >= PAGE_DATABASE.page_count {
                break;
            }

            let buddy_ptr = PAGE_DATABASE.pages.add(b_idx);
            let buddy = &mut *buddy_ptr;

            if matches!(buddy.state, PageState::Free { .. }) && buddy.order == i {
                allocator.levels[i].remove(NonNull::new_unchecked(buddy_ptr));

                buddy.state = PageState::Used {
                    mapcount: AtomicU16::new(0),
                    children: AtomicU16::new(0),
                };

                // We always follow the page on the left.
                if idx > b_idx {
                    idx = b_idx;
                }
                i += 1;
            } else {
                break;
            }
        }

        let page_ptr = PAGE_DATABASE.pages.add(idx);
        let page = &mut *page_ptr;
        page.order = i;
        page.state = PageState::Free {
            list_node: ListNode::default(),
        };

        allocator.levels[i].push_end(NonNull::new_unchecked(page_ptr));
    }
}

pub fn get_page(addr: PhysAddr) -> Option<NonNull<Page>> {
    unsafe {
        debug_assert!(addr.is_aligned_to(PAGE_SIZE));

        let idx = addr.as_usize() / PAGE_SIZE;
        debug_assert!(idx < PAGE_DATABASE.page_count);

        let page_ptr = PAGE_DATABASE.pages.add(idx);
        let page = &mut *page_ptr;
        debug_assert!(matches!(page.state, PageState::Used { .. }));

        Some(NonNull::new_unchecked(page_ptr))
    }
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
