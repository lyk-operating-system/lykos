use core::{
    ptr::{self, NonNull},
    sync::atomic::{AtomicPtr, AtomicU128, Ordering},
};

use crate::{
    memory::{
        MAX_NUMA_NODES, PAGE_SIZE,
        buddy::{self, Page},
        hhdm_offset,
    },
    percpu::PerCpu,
    sync::spinlock::SpinLock,
};
use intrusive_collections::offset_of;
use utils::collections::list::{List, ListNode};

pub struct Cache {
    per_cpu: PerCpu<CacheCpu>,

    name: &'static str,
    size: usize,     // Object size with metadata.
    obj_size: usize, // Real object size.
    align: usize,

    order: usize,       // Page order to request per slab.
    min_partial: usize, // Minimum partial slabs kept per node.

    nodes: [SpinLock<CacheNode>; MAX_NUMA_NODES],
}

#[repr(C, align(64))]
struct CacheCpu {
    freelist: AtomicPtr<u8>, // Free objects from the active slab.
    tid: usize,              // Transaction ID.
    active: *mut Slab,
    partial: List<Slab, SLAB_LIST_NODE_OFF>, // Partial slabs temporarily pinned to this CPU.
}

struct CacheNode {
    partial: List<Slab, SLAB_LIST_NODE_OFF>,
    full: List<Slab, SLAB_LIST_NODE_OFF>,
}

struct Slab {
    freelist: AtomicPtr<u8>,
    inuse: u16,
    objects: u16,

    frozen: bool,
    memory: *mut u8,

    owner: NonNull<Cache>,
    list_node: ListNode,
}

const SLAB_LIST_NODE_OFF: usize = offset_of!(Slab, list_node);

impl Cache {
    pub fn alloc(&self) -> Option<NonNull<u8>> {
        let per_cpu = self.per_cpu.get_local();

        // Fast path
        let mut head = per_cpu.freelist.load(Ordering::Acquire);
        loop {
            let (current_freelist, current_tid) = per_cpu.read_freelist_and_tid();

            if current_freelist.is_null() {
                break; // Fallthrough to slow paths
            }

            let next = unsafe { *(head as *const *mut u8) };
            match per_cpu.freelist.compare_exchange_weak(
                head,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => {
                    unsafe { (*per_cpu.active).inuse += 1 };
                    return NonNull::new(head);
                }
                Err(new_head) => head = new_head,
            }
        }

        // Slow path 1
        let active_ptr = per_cpu.active;
        if !active_ptr.is_null() {
            let active_slab = unsafe { &mut *active_ptr };

            let slab_head = active_slab
                .freelist
                .swap(ptr::null_mut(), Ordering::Acquire);

            if !slab_head.is_null() {
                let next = unsafe { *(slab_head as *const *mut u8) };
                per_cpu.freelist.store(next, Ordering::Release);
                active_slab.inuse += 1;

                return NonNull::new(slab_head);
            }
        }

        // Slow path 2
        if let Some(mut partial_slab_ptr) = per_cpu.partial.pop_front() {
            let partial_slab = unsafe { partial_slab_ptr.as_mut() };

            per_cpu.active = partial_slab;
            partial_slab.frozen = true;

            let slab_head = partial_slab
                .freelist
                .swap(ptr::null_mut(), Ordering::Acquire);

            if !slab_head.is_null() {
                let next = unsafe { *(slab_head as *const *mut u8) };
                per_cpu.freelist.store(next, Ordering::Release);
                partial_slab.inuse += 1;

                return NonNull::new(slab_head);
            }
        }

        // Slow path 3
        let numa_id = 0; // TODO: use actual numa id
        let mut node = self.nodes[numa_id].lock();

        let limit = self.min_partial / 2;
        let mut migrated = 0;
        while let Some(slab_ptr) = node.partial.pop_front() {
            per_cpu.partial.push_front(slab_ptr);
            migrated += 1;
            if migrated >= limit {
                break;
            }
        }

        drop(node);

        if let Some(mut partial_slab_ptr) = per_cpu.partial.pop_front() {
            let partial_slab = unsafe { partial_slab_ptr.as_mut() };

            per_cpu.active = partial_slab;
            partial_slab.frozen = true;

            let slab_head = partial_slab
                .freelist
                .swap(ptr::null_mut(), Ordering::Acquire);

            if !slab_head.is_null() {
                let next = unsafe { *(slab_head as *const *mut u8) };
                per_cpu.freelist.store(next, Ordering::Release);
                partial_slab.inuse += 1;

                return NonNull::new(slab_head);
            }
        }

        // Slow path 4
        let mut new_slab_ptr = Slab::new(self)?;
        let new_slab = unsafe { new_slab_ptr.as_mut() };

        per_cpu.active = new_slab;
        let slab_head = new_slab.freelist.swap(ptr::null_mut(), Ordering::Acquire);

        if !slab_head.is_null() {
            let next = unsafe { *(slab_head as *const *mut u8) };
            per_cpu.freelist.store(next, Ordering::Release);
            new_slab.inuse += 1;

            return NonNull::new(slab_head);
        }

        None
    }
}

impl Slab {
    pub fn new(cache: &Cache) -> Option<NonNull<Slab>> {
        let page_ptr = buddy::alloc(cache.order as usize)?;
        let page = unsafe { page_ptr.as_ref() };

        let memory = (page.addr.as_usize() + hhdm_offset()) as *mut u8;
        let slab_ptr = memory as *mut Slab;

        let header_size = size_of::<Slab>();
        let obj_start = (header_size + cache.align - 1) & !(cache.align - 1);

        let usable_bytes = (PAGE_SIZE << cache.order) - obj_start;
        let obj_cnt = usable_bytes / cache.size;

        let mut head = ptr::null_mut();
        for i in (0..obj_cnt).rev() {
            unsafe {
                let obj_ptr = memory.add(obj_start + i * cache.size);
                *(obj_ptr as *mut *mut u8) = head;
                head = obj_ptr;
            }
        }

        let slab = Self {
            freelist: AtomicPtr::new(head),
            inuse: 0,
            objects: obj_cnt as u16,
            frozen: true,
            memory: unsafe { memory.add(obj_start) },
            owner: NonNull::from(cache),
            list_node: ListNode::default(),
        };

        unsafe {
            core::ptr::write(slab_ptr, slab);
            Some(NonNull::new_unchecked(slab_ptr))
        }
    }
}
