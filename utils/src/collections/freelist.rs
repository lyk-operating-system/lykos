use core::ptr::NonNull;

pub struct Freelist {
    head: Option<NonNull<u8>>,
}

impl Freelist {
    pub const fn new() -> Self {
        Self { head: None }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    #[inline]
    pub unsafe fn push(&mut self, ptr: NonNull<u8>) {
        unsafe {
            ptr.as_ptr().cast::<Option<NonNull<u8>>>().write(self.head);
        }
        self.head = Some(ptr);
    }

    #[inline]
    pub fn pop(&mut self) -> Option<NonNull<u8>> {
        let ret = self.head?;

        unsafe {
            self.head = ret.as_ptr().cast::<Option<NonNull<u8>>>().read();
        }

        Some(ret)
    }
}
