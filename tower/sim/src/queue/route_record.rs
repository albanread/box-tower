// ts: queue/route-record.ts

// 1288:c05a TowerRouteQueueRecord (324-byte per floor-slot)
//
// Per-floor route-request ring buffer, mirroring the binary's
// TowerRouteQueueRecord layout (2 count/head bytes per direction + two
// 40-entry request ref arrays). Routing-wise the only observable part is
// the FIFO order of the ring and the size-40 silent wrap.

use serde::{Deserialize, Serialize};

const ROUTE_QUEUE_CAPACITY: i64 = 40;

/// Fixed-40-entry ring of route request ids for one direction of one floor.
///
/// Binary quirk: size-40 ring silently overwrites head on 41st enqueue.
/// `enqueue_request_into_route_queue` (1218:1002) computes the write slot as
/// `(head + count) % 40` with no capacity check. On the 41st enqueue the
/// write index wraps back to `head`, overwriting the oldest entry; `count`
/// then saturates at 40 (the binary increments count but the 41st slot is
/// the one we just clobbered, so effectively head advances).
///
/// Upstream's `readonly capacity` is an instance field, so a snapshot holds
/// it (always 40): `{capacity, items, head, count}`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RouteRequestRing {
    pub capacity: i64,
    pub items: Vec<String>,
    pub head: i64,
    pub count: i64,
}

impl Default for RouteRequestRing {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteRequestRing {
    pub fn new() -> Self {
        RouteRequestRing {
            capacity: ROUTE_QUEUE_CAPACITY,
            items: vec![String::new(); ROUTE_QUEUE_CAPACITY as usize],
            head: 0,
            count: 0,
        }
    }

    /// Reconstitute a RouteRequestRing from a plain-object snapshot.
    ///
    /// Upstream takes `{ items, head, count }`; the fields are the arguments.
    /// An item is `None` where the snapshot held `null` (upstream's
    /// `data.items[i] ?? ""`).
    pub fn from(items: &[Option<String>], head: i64, count: i64) -> RouteRequestRing {
        let mut buf = RouteRequestRing::new();
        let len = (ROUTE_QUEUE_CAPACITY as usize).min(items.len());
        for i in 0..len {
            buf.items[i] = items[i].clone().unwrap_or_default();
        }
        buf.head = head % ROUTE_QUEUE_CAPACITY;
        buf.count = ROUTE_QUEUE_CAPACITY.min(0.max(count));
        buf
    }

    pub fn size(&self) -> i64 {
        self.count
    }

    pub fn is_full(&self) -> bool {
        self.count >= ROUTE_QUEUE_CAPACITY
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// Append a request id to the ring.
    ///
    /// Binary quirk: size-40 ring silently overwrites head on 41st enqueue.
    /// When the ring is already full, the write slot is `(head + 40) % 40 ==
    /// head`, so the oldest entry is clobbered and the effective queue order
    /// shifts by one (head now points at what used to be index 1). Returns
    /// true unconditionally — the binary has no full flag.
    pub fn push(&mut self, item: &str) -> bool {
        let write_index = (self.head + self.count) % ROUTE_QUEUE_CAPACITY;
        self.items[write_index as usize] = item.to_string();
        if self.count < ROUTE_QUEUE_CAPACITY {
            self.count += 1;
        } else {
            // Binary quirk: size-40 ring silently overwrites head on 41st enqueue.
            // The 41st push writes at index `head`, and the emulated count stays
            // pinned at 40; logical head advances so the newly-written item is
            // the new tail (last-to-pop).
            self.head = (self.head + 1) % ROUTE_QUEUE_CAPACITY;
        }
        true
    }

    /// Remove and return the oldest item, or undefined if empty.
    pub fn pop(&mut self) -> Option<String> {
        if self.count <= 0 {
            return None;
        }
        let item = self.items[self.head as usize].clone();
        self.head = (self.head + 1) % ROUTE_QUEUE_CAPACITY;
        self.count -= 1;
        Some(item)
    }

    /// Return all items in queue order without removing them.
    pub fn peek_all(&self) -> Vec<String> {
        (0..self.count)
            .map(|i| self.items[((self.head + i) % ROUTE_QUEUE_CAPACITY) as usize].clone())
            .collect()
    }

    /// Scan-and-remove: pop the first occurrence of `item` while preserving
    /// the relative order of the remaining entries. Returns true if removed.
    /// Mirrors the compaction pass of `remove_request_from_unit_queue`
    /// (1218:142a).
    pub fn remove_first(&mut self, item: &str) -> bool {
        if self.count == 0 {
            return false;
        }
        let mut remaining: Vec<String> = Vec::new();
        let mut found = false;
        while !self.is_empty() {
            let popped = self.pop();
            if !found && popped.as_deref() == Some(item) {
                found = true;
                continue;
            }
            if let Some(p) = popped {
                remaining.push(p);
            }
        }
        for id in &remaining {
            self.push(id);
        }
        found
    }
}

pub const ROUTE_QUEUE_CAPACITY_CONST: i64 = ROUTE_QUEUE_CAPACITY;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_41st_push_overwrites_the_oldest() {
        let mut r = RouteRequestRing::new();
        for i in 0..41 {
            r.push(&format!("r{i}"));
        }
        assert_eq!(r.size(), 40);
        assert_eq!(r.head, 1);
        let all = r.peek_all();
        assert_eq!(all.first().map(String::as_str), Some("r1"));
        assert_eq!(all.last().map(String::as_str), Some("r40"));
        assert_eq!(r.pop().as_deref(), Some("r1"));
    }

    #[test]
    fn remove_first_keeps_order() {
        let mut r = RouteRequestRing::new();
        for id in ["a", "b", "a", "c"] {
            r.push(id);
        }
        assert!(r.remove_first("a"));
        assert_eq!(r.peek_all(), ["b", "a", "c"]);
        assert!(!r.remove_first("z"));
        assert_eq!(r.peek_all(), ["b", "a", "c"]);
    }

    #[test]
    fn from_clamps() {
        let items = vec![Some("x".to_string()), None];
        let r = RouteRequestRing::from(&items, 41, 99);
        assert_eq!((r.head, r.count, r.items.len()), (1, 40, 40));
        assert_eq!((r.items[0].as_str(), r.items[1].as_str()), ("x", ""));
        assert_eq!(
            serde_json::to_value(&RouteRequestRing::new()).unwrap().as_object().unwrap().keys().collect::<Vec<_>>(),
            ["capacity", "items", "head", "count"]
        );
    }
}
