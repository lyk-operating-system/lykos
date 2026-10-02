use core::{
    cell::UnsafeCell,
    hint::spin_loop,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicBool, Ordering},
};

use crate::arch::lcpu;

pub struct Spinlock<T, const DISABLE_INT: bool = true> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

pub type RawSpinlock<T> = Spinlock<T, false>;
pub type RawSpinlockGuard<'a, T> = SpinlockGuard<'a, T, false>;

unsafe impl<T: Send, const DISABLE_INT: bool> Sync for Spinlock<T, DISABLE_INT> {}
unsafe impl<T: Send, const DISABLE_INT: bool> Send for Spinlock<T, DISABLE_INT> {}

impl<T, const DISABLE_INT: bool> Spinlock<T, DISABLE_INT> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> SpinlockGuard<'_, T, DISABLE_INT> {
        let prev_int_state = if DISABLE_INT { lcpu::irq_save() } else { false };

        while self
            .locked
            .compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            while self.locked.load(Ordering::Relaxed) {
                spin_loop();
            }
        }

        SpinlockGuard {
            lock: self,
            prev_int_state,
        }
    }

    #[inline(always)]
    fn unlock(&self) {
        self.locked.store(false, Ordering::Release);
    }
}

pub struct SpinlockGuard<'a, T, const DISABLE_INT: bool = true> {
    lock: &'a Spinlock<T, DISABLE_INT>,
    prev_int_state: bool,
}

impl<T: ?Sized, const DISABLE_INT: bool> !Send for SpinlockGuard<'_, T, DISABLE_INT> {}

impl<T, const DISABLE_INT: bool> Deref for SpinlockGuard<'_, T, DISABLE_INT> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.value.get() }
    }
}

impl<T, const DISABLE_INT: bool> DerefMut for SpinlockGuard<'_, T, DISABLE_INT> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T, const DISABLE_INT: bool> Drop for SpinlockGuard<'_, T, DISABLE_INT> {
    fn drop(&mut self) {
        self.lock.unlock();

        if DISABLE_INT {
            lcpu::irq_restore(self.prev_int_state);
        }
    }
}
