use std::collections::VecDeque;
use std::collections::vec_deque::{Drain, Iter};
use std::ops::RangeBounds;

pub struct BoundedQueue<T> {
    queue: VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedQueue<T> {
    pub fn new(capacity: usize) -> BoundedQueue<T> {
        BoundedQueue {
            queue: VecDeque::new(),
            capacity,
        }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        if (self.is_full()) {
            self.queue.pop_front();
        }
        self.queue.push_back(value);
    }

    #[inline]
    pub fn drain<R>(&mut self, range: R) -> Drain<'_, T>
    where
        R: RangeBounds<usize>,
    {
        self.queue.drain(range)
    }

    #[inline]
    pub fn iter(&self) -> Iter<'_, T> {
        self.queue.iter()
    }

    #[inline]
    fn is_full(&self) -> bool {
        self.capacity == self.queue.len()
    }
}
