use std::collections::{HashSet, VecDeque};
use std::time::{Duration, Instant};

pub struct DedupFilter {
    seen: HashSet<u64>,
    queue: VecDeque<(u64, Instant)>,
    max_capacity: usize,
    ttl: Duration,
}

impl DedupFilter {
    pub fn new(max_capacity: usize, ttl: Duration) -> Self {
        Self {
            seen: HashSet::with_capacity(max_capacity.min(1024)),
            queue: VecDeque::with_capacity(max_capacity.min(1024)),
            max_capacity,
            ttl,
        }
    }

    /// Check if a message ID has already been observed.
    /// If not seen, records it and returns `false`.
    /// If seen within TTL, returns `true`.
    pub fn contains_or_insert(&mut self, msg_id: u64) -> bool {
        self.evict_expired();

        if self.seen.contains(&msg_id) {
            return true;
        }

        if self.queue.len() >= self.max_capacity {
            if let Some((old_id, _)) = self.queue.pop_front() {
                self.seen.remove(&old_id);
            }
        }

        let now = Instant::now();
        self.seen.insert(msg_id);
        self.queue.push_back((msg_id, now));
        false
    }

    fn evict_expired(&mut self) {
        let now = Instant::now();
        while let Some(&(_, timestamp)) = self.queue.front() {
            if now.duration_since(timestamp) >= self.ttl {
                if let Some((old_id, _)) = self.queue.pop_front() {
                    self.seen.remove(&old_id);
                }
            } else {
                break;
            }
        }
    }

    pub fn len(&self) -> usize {
        self.seen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.seen.is_empty()
    }
}
