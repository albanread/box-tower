// ts: ring-buffer.ts

use serde::{Deserialize, Serialize};

/// Fixed-capacity ring buffer backed by a pre-allocated array.
///
/// Serializes as plain JSON (`{items, head, count}`) and can be
/// reconstituted from that shape via `RingBuffer.from()`.
///
/// Upstream's `capacity` is a constructor parameter property, so it is
/// serialised too (first, as the constructor defines it first).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RingBuffer<T> {
    pub capacity: i64,
    pub items: Vec<T>,
    pub head: i64,
    pub count: i64,
}

impl<T: Clone> RingBuffer<T> {
    pub fn new(capacity: i64, fill: T) -> Self {
        RingBuffer {
            capacity,
            items: vec![fill; capacity as usize],
            head: 0,
            count: 0,
        }
    }

    /// Reconstitute a RingBuffer from a plain-object snapshot.
    ///
    /// Upstream takes `{ items, head, count }`; the fields are the arguments.
    pub fn from(items: Vec<T>, head: i64, count: i64) -> Self {
        let capacity = items.len() as i64;
        RingBuffer {
            capacity,
            items,
            head,
            count,
        }
    }

    pub fn size(&self) -> i64 {
        self.count
    }

    pub fn is_full(&self) -> bool {
        self.count >= self.capacity
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Append an item. Returns false if the buffer is full.
    pub fn push(&mut self, item: T) -> bool {
        if self.count >= self.capacity {
            return false;
        }
        let write_index = (self.head + self.count) % self.capacity;
        self.items[write_index as usize] = item;
        self.count += 1;
        true
    }

    /// Remove and return the oldest item, or undefined if empty.
    pub fn pop(&mut self) -> Option<T> {
        if self.count <= 0 {
            return None;
        }
        let item = self.items[self.head as usize].clone();
        self.head = (self.head + 1) % self.capacity;
        self.count -= 1;
        Some(item)
    }

    /// Return all items in queue order without removing them.
    pub fn peek_all(&self) -> Vec<T> {
        (0..self.count)
            .map(|i| self.items[((self.head + i) % self.capacity) as usize].clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fifo_with_wrap() {
        let mut b = RingBuffer::new(3, 0i64);
        assert!(b.push(1) && b.push(2) && b.push(3));
        assert!(b.is_full() && !b.push(4));
        assert_eq!(b.pop(), Some(1));
        assert!(b.push(4));
        assert_eq!(b.peek_all(), [2, 3, 4]);
        assert_eq!(serde_json::to_string(&b).unwrap(), r#"{"capacity":3,"items":[4,2,3],"head":1,"count":3}"#);
        let c = RingBuffer::from(b.items.clone(), b.head, b.count);
        assert_eq!(c, b);
    }
}
