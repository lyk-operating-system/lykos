use core::{
    marker::PhantomData,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{arch::lcpu::cpu_local_base, memory::PAGE_SIZE};

static NEXT_OFFSET: AtomicUsize = AtomicUsize::new(1024);

pub struct PerCpu<T> {
    offset: usize,
    _phantom: PhantomData<T>,
}

unsafe impl<T> Sync for PerCpu<T> {}

impl<T> PerCpu<T> {
    pub fn new() -> Self {
        let size = size_of::<T>();
        let align = align_of::<T>();

        let offset = NEXT_OFFSET
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |val| {
                let aligned = (val + align - 1) & !(align - 1);
                let next = aligned + size;

                if next > PAGE_SIZE { None } else { Some(next) }
            })
            .expect("Out of per-CPU memory!");

        let offset = (offset + align - 1) & !(align - 1);

        Self {
            offset,
            _phantom: PhantomData,
        }
    }

    /// Retrieves a mutable reference to the current CPU's local data.
    /// # Warning
    /// Preemption must be disabled before calling this function.
    pub fn get_local(&self) -> &mut T {
        let base = cpu_local_base();

        unsafe { &mut *base.add(self.offset).cast::<T>() }
    }
}
