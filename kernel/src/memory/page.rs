use core::sync::atomic::{AtomicU16, Ordering};

use utils::collections::list::ListNode;

use crate::memory::PhysAddr;

pub const MAX_PAGE_ORDER: usize = 10;

pub struct Page {
    pub addr: PhysAddr,
    pub order: usize,
    pub state: PageState,
}

pub(super) enum PageState {
    Free {
        list_node: ListNode,
    },
    Used {
        mapcount: AtomicU16,
        children: AtomicU16,
    },
}

impl Page {
    pub fn inc_children(&self) {
        let PageState::Used { children, .. } = &self.state else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        children.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_children(&self) -> bool {
        let PageState::Used { children, .. } = &self.state else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        let old = children.fetch_sub(1, Ordering::Relaxed);
        old == 1
    }

    pub fn set_children(&self, count: u16) {
        let PageState::Used { children, .. } = &self.state else {
            unsafe { core::hint::unreachable_unchecked() }
        };

        children.store(count, Ordering::Relaxed);
    }
}
