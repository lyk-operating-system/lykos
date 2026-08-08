use core::{marker::PhantomData, ptr::NonNull};

pub struct List<T, const OFF: usize> {
    pub head: Option<NonNull<ListNode>>,
    pub tail: Option<NonNull<ListNode>>,
    pub length: usize,
    _marker: PhantomData<T>,
}

pub struct ListNode {
    prev: Option<NonNull<ListNode>>,
    next: Option<NonNull<ListNode>>,
}

impl<T, const OFF: usize> List<T, OFF> {
    pub const fn new() -> Self {
        Self {
            head: None,
            tail: None,
            length: 0,
            _marker: PhantomData,
        }
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.head.is_none()
    }

    pub fn insert_after(&mut self, pos: Option<NonNull<T>>, new: NonNull<T>) {
        unsafe {
            let mut new_node = Self::node(new);

            match pos {
                Some(pos_item) => {
                    let mut pos_node = Self::node(pos_item);

                    // Middle/new node.
                    new_node.as_mut().prev = Some(pos_node);
                    new_node.as_mut().next = pos_node.as_mut().next;

                    // Right node.
                    if let Some(mut next_node) = pos_node.as_ref().next {
                        next_node.as_mut().prev = Some(new_node);
                    }

                    // Left node.
                    pos_node.as_mut().next = Some(new_node);

                    // Update tail if needed.
                    if self.tail == Some(pos_node) {
                        self.tail = Some(new_node)
                    }
                }
                // This case works only if the list is empty.
                None => {
                    debug_assert!(self.is_empty());

                    self.head = Some(new_node);
                    self.tail = Some(new_node);
                    new_node.as_mut().prev = None;
                    new_node.as_mut().next = None;
                }
            }
        }

        self.length += 1;
    }

    pub fn insert_before(&mut self, pos: Option<NonNull<T>>, new: NonNull<T>) {
        unsafe {
            let mut new_node = Self::node(new);

            match pos {
                Some(pos_item) => {
                    let mut pos_node = Self::node(pos_item);

                    // Middle/new node.
                    new_node.as_mut().prev = pos_node.as_mut().prev;
                    new_node.as_mut().next = Some(pos_node);

                    // Left node.
                    if let Some(mut prev_node) = pos_node.as_ref().prev {
                        prev_node.as_mut().next = Some(new_node);
                    }

                    // Right node.
                    pos_node.as_mut().prev = Some(new_node);

                    // Update head if needed.
                    if self.head == Some(pos_node) {
                        self.head = Some(new_node)
                    }
                }
                // This case works only if the list is empty.
                None => {
                    debug_assert!(self.is_empty());

                    self.head = Some(new_node);
                    self.tail = Some(new_node);
                    new_node.as_mut().prev = None;
                    new_node.as_mut().next = None;
                }
            }
        }

        self.length += 1;
    }

    pub fn push_front(&mut self, new: NonNull<T>) {
        let pos = self.head.map(|n| Self::item(n));
        self.insert_before(pos, new);
    }

    pub fn push_end(&mut self, new: NonNull<T>) {
        let pos = self.tail.map(|n| Self::item(n));
        self.insert_after(pos, new);
    }

    pub fn remove(&mut self, node: NonNull<T>) {
        unsafe {
            let mut node_inner = Self::node(node);

            if let Some(mut prev_node) = node_inner.as_mut().prev {
                prev_node.as_mut().next = node_inner.as_mut().next;
            }
            if let Some(mut next_node) = node_inner.as_mut().next {
                next_node.as_mut().prev = node_inner.as_mut().prev;
            }

            if self.head == Some(node_inner) {
                self.head = node_inner.as_mut().next;
            }
            if self.tail == Some(node_inner) {
                self.tail = node_inner.as_mut().prev;
            }
        }

        self.length -= 1;
    }

    pub fn pop_front(&mut self) -> Option<NonNull<T>> {
        let head_node = self.head?;
        let item = Self::item(head_node);
        self.remove(item);
        Some(item)
    }

    pub fn pop_end(&mut self) -> Option<NonNull<T>> {
        let tail_node = self.tail?;
        let item = Self::item(tail_node);
        self.remove(item);
        Some(item)
    }

    /*
     * Helpers
     */

    /// Get the `ListNode` pointer inside `T`.
    #[inline]
    fn node(ptr: NonNull<T>) -> NonNull<ListNode> {
        unsafe { NonNull::new_unchecked(ptr.as_ptr().cast::<u8>().add(OFF).cast::<ListNode>()) }
    }

    /// Get the container `T` pointer from a `ListNode`.
    #[inline]
    fn item(ptr: NonNull<ListNode>) -> NonNull<T> {
        unsafe { NonNull::new_unchecked(ptr.as_ptr().cast::<u8>().sub(OFF).cast::<T>()) }
    }
}

impl ListNode {
    pub const fn default() -> Self {
        Self {
            prev: None,
            next: None,
        }
    }
}
