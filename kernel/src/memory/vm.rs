use intrusive_collections::{RBTree, RBTreeAtomicLink, intrusive_adapter};

use crate::{
    arch::paging::{self, Map},
    boot::KERNEL_ADDRESS_REQUEST,
    memory::{GIB, MIB, PhysAddr, VirtAddr, VmCache, VmProtection, hhdm_offset},
    println,
};

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
enum SegmentType {
    Anonymous,
    File,
    Physical,
}

struct Segment {
    pub base: VirtAddr,
    pub length: usize,
    pub ty: SegmentType,

    link: RBTreeAtomicLink,
}

impl Segment {
    pub fn new(base: VirtAddr, length: usize, ty: SegmentType) -> Self {
        Self {
            base,
            length,
            ty,
            link: RBTreeAtomicLink::new(),
        }
    }

    pub fn end(&self) -> VirtAddr {
        self.base + self.length
    }

    pub fn contains(&self, addr: VirtAddr) -> bool {
        addr >= self.base && addr < self.end()
    }
}

intrusive_adapter!(SegmentAdapter = &'static Segment: Segment { link => RBTreeAtomicLink });

struct AddrSpace {
    map: Map,
    segments: RBTree<SegmentAdapter>,
}

impl AddrSpace {
    pub fn new() -> Option<Self> {
        Some(Self {
            map: Map::new()?,
            segments: RBTree::new(SegmentAdapter::new()),
        })
    }
}

pub fn init() {
    paging::init();

    let mut kernel_addr_space = AddrSpace::new().expect("Could not create kernel address space");

    kernel_addr_space.map.map_range(
        VirtAddr(hhdm_offset()),
        PhysAddr(0),
        4 * GIB,
        VmProtection::FULL,
        VmCache::Standard,
    );

    let kernel_addr_response = KERNEL_ADDRESS_REQUEST
        .get_response()
        .expect("Kernel address request failed");

    kernel_addr_space.map.map_range(
        VirtAddr(kernel_addr_response.virtual_base() as usize),
        PhysAddr(kernel_addr_response.physical_base() as usize),
        2 * MIB,
        VmProtection::FULL,
        VmCache::Standard,
    );

    kernel_addr_space.map.load_map();
    println!("Virtual memory initialized.");
}
