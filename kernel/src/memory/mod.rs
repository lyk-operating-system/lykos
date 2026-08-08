use bitflags::bitflags;

use crate::boot::HHDM_REQUEST;
use core::ops::{Add, Sub};

pub mod buddy;
pub mod page;
pub mod vm;

pub const PAGE_SIZE: usize = 4096;

pub const KIB: usize = 1024;
pub const MIB: usize = KIB * 1024;
pub const GIB: usize = MIB * 1024;

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PhysAddr(pub usize);

#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct VirtAddr(pub usize);

macro_rules! addr_impl {
    ($name:ident) => {
        impl $name {
            pub fn as_usize(self) -> usize {
                self.0
            }

            pub fn is_null(self) -> bool {
                self.0 == 0
            }

            pub fn is_aligned_to(self, align: usize) -> bool {
                self.0 % align == 0
            }

            pub fn align_to(self, align: usize) -> Self {
                Self(self.0.next_multiple_of(align))
            }

            pub fn down_align_to(self, align: usize) -> Self {
                Self(self.0 & !(align - 1))
            }
        }

        impl Add<usize> for $name {
            type Output = Self;

            fn add(self, off: usize) -> Self::Output {
                Self(self.0.wrapping_add(off))
            }
        }

        impl Sub<usize> for $name {
            type Output = Self;

            fn sub(self, off: usize) -> Self::Output {
                Self(self.0.wrapping_sub(off))
            }
        }
    };
}

addr_impl!(PhysAddr);
addr_impl!(VirtAddr);

pub fn hhdm_offset() -> usize {
    HHDM_REQUEST
        .get_response()
        .expect("HHDM request failed")
        .offset() as usize
}

bitflags! {
    #[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
    pub struct VmProtection: usize {
        const READ = 1;
        const WRITE = 2;
        const EXECUTE = 4;

        const FULL = Self::READ.bits() | Self::WRITE.bits() | Self::EXECUTE.bits();
    }
}

#[derive(Clone, Copy, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub enum VmCache {
    Standard,
    WriteThrough,
    WriteCombine,
    None,
}
