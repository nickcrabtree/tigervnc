//! Shared Adaptive Replacement Cache (ARC) for decoded rectangles.
//!
//! This module provides a generic ARC implementation that can be reused by
//! both the in‑memory ContentCache (session-only, u64 keys) and the
//! PersistentClientCache (hash keys, disk-backed).
//!
//! The design loosely follows the C++ ContentCache/PersistentCache ARC structure:
//! - T1/T2: resident lists (recently vs frequently used)
//! - B1/B2: ghost lists (evicted keys, no data)
//! - p: adaptive target size for T1
//!
//! This layer only tracks keys and byte sizes and can be wrapped by higher
//! level caches that store the actual payloads.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// Which ARC list a key currently lives in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ListKind {
    None,
    T1,
    T2,
    B1,
    B2,
}

/// Generic ARC cache core that tracks keys and sizes, but not payloads.
///
/// K is typically u64 (ContentCache) or [u8; 16] (PersistentCache).
#[derive(Debug)]
pub struct ArcCache<K> {
    /// Maximum capacity in bytes for resident entries (T1 + T2).
    max_bytes: usize,
    /// Current resident size in bytes.
    current_bytes: usize,
    /// Adaptive target size for T1 (in bytes).
    p_bytes: usize,

    /// Recency list (resident): keys used once recently.
    t1: VecDeque<K>,
    /// Frequency list (resident): keys used at least twice.
    t2: VecDeque<K>,
    /// Ghost list for T1 evictions.
    b1: VecDeque<K>,
    /// Ghost list for T2 evictions.
    b2: VecDeque<K>,

    /// Per-key metadata: which list and size in bytes (for resident).
    list_map: HashMap<K, (ListKind, usize)>,

    /// Pending evictions (keys removed from resident sets).
    pending_evictions: Vec<K>,
}

impl<K> ArcCache<K>
where
    K: Eq + Hash + Clone,
{
    /// Create a new ARC cache with the given byte capacity.
    pub fn new(max_bytes: usize) -> Self {
        Self {
            max_bytes,
            current_bytes: 0,
            p_bytes: 0,
            t1: VecDeque::new(),
            t2: VecDeque::new(),
            b1: VecDeque::new(),
            b2: VecDeque::new(),
            list_map: HashMap::new(),
            pending_evictions: Vec::new(),
        }
    }

    /// Returns the configured capacity in bytes.
    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// Returns current resident size in bytes.
    pub fn current_bytes(&self) -> usize {
        self.current_bytes
    }

    /// Returns the current target size for T1 (in bytes).
    pub fn target_t1_bytes(&self) -> usize {
        self.p_bytes
    }

    /// Returns counts of keys in each list (T1,T2,B1,B2).
    pub fn list_lengths(&self) -> (usize, usize, usize, usize) {
        (self.t1.len(), self.t2.len(), self.b1.len(), self.b2.len())
    }

    /// Record a hit on a resident key. Caller must ensure the key is present
    /// in T1 or T2.
    pub fn on_hit(&mut self, key: &K) {
        if let Some((kind, size)) = self.list_map.get(key).cloned() {
            match kind {
                ListKind::T1 => {
                    // Promote to T2.
                    self.remove_from_list(key, ListKind::T1);
                    self.t2.push_front(key.clone());
                    self.list_map.insert(key.clone(), (ListKind::T2, size));
                }
                ListKind::T2 => {
                    // Move to front of T2.
                    self.remove_from_list(key, ListKind::T2);
                    self.t2.push_front(key.clone());
                }
                _ => {
                    // Not expected for on_hit.
                }
            }
        }
    }

    /// Insert or reinsert a resident entry of the given size (in bytes).
    ///
    /// Returns any keys that were evicted as a result.
    pub fn insert_resident(&mut self, key: K, size_bytes: usize) -> Vec<K> {
        let mut evicted = Vec::new();
        if self.max_bytes > 0 {
            while self.current_bytes + size_bytes > self.max_bytes {
                if !self.replace(&mut evicted) {
                    break;
                }
            }
        }

        // Insert into T1 as a new entry.
        self.remove_any(&key);
        self.t1.push_front(key.clone());
        self.list_map
            .insert(key.clone(), (ListKind::T1, size_bytes));
        self.current_bytes += size_bytes;

        evicted
    }

    /// Remove a resident key completely (if present) and return its size.
    pub fn remove_resident(&mut self, key: &K) -> Option<usize> {
        if let Some((kind, size)) = self.list_map.remove(key) {
            match kind {
                ListKind::T1 => self.remove_from_list(key, ListKind::T1),
                ListKind::T2 => self.remove_from_list(key, ListKind::T2),
                ListKind::B1 => self.remove_from_list(key, ListKind::B1),
                ListKind::B2 => self.remove_from_list(key, ListKind::B2),
                ListKind::None => {}
            }
            if matches!(kind, ListKind::T1 | ListKind::T2) {
                self.current_bytes = self.current_bytes.saturating_sub(size);
            }
            Some(size)
        } else {
            None
        }
    }

    /// Mark a ghost hit in B1 (recently evicted from T1).
    pub fn on_ghost_hit_b1(&mut self, key: &K) {
        let b1_len = self.b1.len().max(1);
        let b2_len = self.b2.len().max(1);
        let delta_entries = (b2_len / b1_len).max(1);
        let delta_bytes = delta_entries * self.average_entry_size_bytes();
        self.p_bytes = (self.p_bytes + delta_bytes).min(self.max_bytes);
        self.remove_from_list(key, ListKind::B1);
    }

    /// Mark a ghost hit in B2 (recently evicted from T2).
    pub fn on_ghost_hit_b2(&mut self, key: &K) {
        let b1_len = self.b1.len().max(1);
        let b2_len = self.b2.len().max(1);
        let delta_entries = (b1_len / b2_len).max(1);
        let delta_bytes = delta_entries * self.average_entry_size_bytes();
        self.p_bytes = self.p_bytes.saturating_sub(delta_bytes);
        self.remove_from_list(key, ListKind::B2);
    }

    /// Retrieve and clear the list of keys that have been evicted since the
    /// last call (for eviction notifications to server).
    pub fn take_pending_evictions(&mut self) -> Vec<K> {
        std::mem::take(&mut self.pending_evictions)
    }

    fn average_entry_size_bytes(&self) -> usize {
        if self.list_map.is_empty() {
            1
        } else {
            self.current_bytes.max(1) / self.list_map.len().max(1)
        }
    }

    fn replace(&mut self, evicted: &mut Vec<K>) -> bool {
        if self.t1.is_empty() && self.t2.is_empty() {
            return false;
        }

        // Decide whether to evict from T1 or T2 based on |T1| vs p.
        let t1_bytes = self.sum_bytes(&self.t1, ListKind::T1);
        let from_t1 = t1_bytes > self.p_bytes || self.t2.is_empty();

        if from_t1 {
            if let Some(victim) = self.t1.pop_back() {
                if let Some((_, size)) = self.list_map.get(&victim).cloned() {
                    self.current_bytes = self.current_bytes.saturating_sub(size);
                    self.list_map.insert(victim.clone(), (ListKind::B1, 0));
                    self.b1.push_front(victim.clone());
                    self.pending_evictions.push(victim.clone());
                    evicted.push(victim);
                    return true;
                }
            }
        } else {
            if let Some(victim) = self.t2.pop_back() {
                if let Some((_, size)) = self.list_map.get(&victim).cloned() {
                    self.current_bytes = self.current_bytes.saturating_sub(size);
                    self.list_map.insert(victim.clone(), (ListKind::B2, 0));
                    self.b2.push_front(victim.clone());
                    self.pending_evictions.push(victim.clone());
                    evicted.push(victim);
                    return true;
                }
            }
        }

        false
    }

    fn sum_bytes(&self, list: &VecDeque<K>, kind: ListKind) -> usize {
        list.iter()
            .filter_map(|k| {
                self.list_map
                    .get(k)
                    .and_then(|(lk, sz)| if *lk == kind { Some(*sz) } else { None })
            })
            .sum()
    }

    fn remove_from_list(&mut self, key: &K, kind: ListKind) {
        let list = match kind {
            ListKind::T1 => &mut self.t1,
            ListKind::T2 => &mut self.t2,
            ListKind::B1 => &mut self.b1,
            ListKind::B2 => &mut self.b2,
            ListKind::None => return,
        };
        if let Some(pos) = list.iter().position(|k| k == key) {
            list.remove(pos);
        }
    }

    fn remove_any(&mut self, key: &K) {
        if let Some((kind, _)) = self.list_map.remove(key) {
            self.remove_from_list(key, kind);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic_insert_and_evict() {
        let mut arc: ArcCache<u64> = ArcCache::new(100);
        let evicted = arc.insert_resident(1, 80);
        assert!(evicted.is_empty());
        assert_eq!(arc.current_bytes(), 80);

        let evicted = arc.insert_resident(2, 40);
        // Must have evicted something to stay within 100 bytes.
        assert!(!evicted.is_empty());
        assert!(arc.current_bytes() <= 100);
    }
}