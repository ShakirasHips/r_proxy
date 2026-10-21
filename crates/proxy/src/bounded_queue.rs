use std::collections::VecDeque;
use std::collections::vec_deque::{Drain, Iter};
use std::ops::RangeBounds;

pub struct BoundedQueue<T> {
    queue: VecDeque<T>,
    capacity: usize,
}

impl<T> BoundedQueue<T> {
    pub fn new(capacity: usize) -> BoundedQueue<T> {
        assert!(capacity > 0, "BoundedQueue capacity must be greater than 0");
        BoundedQueue {
            queue: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    #[inline]
    pub fn push(&mut self, value: T) {
        if self.is_full() {
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
    pub fn len(&self) -> usize {
        self.queue.len()
    }

    #[inline]
    fn is_full(&self) -> bool {
        self.queue.len() >= self.capacity
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_evicts_oldest_when_full() {
        let mut q = BoundedQueue::new(3);
        for i in 0..5 {
            q.push(i);
        }
        assert_eq!(q.len(), 3);
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), vec![2, 3, 4]);
    }

    #[test]
    fn test_drain() {
        let mut q = BoundedQueue::new(3);
        q.push(1);
        q.push(2);
        assert_eq!(q.drain(..).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(q.len(), 0);
    }

    #[test]
    #[should_panic]
    fn test_zero_capacity_panics() {
        BoundedQueue::<i32>::new(0);
    }
}
