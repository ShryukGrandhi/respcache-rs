//! An LRU cache with optional per-key expiry. Recency is tracked with a monotonically increasing counter and
//! a BTreeMap from counter to key, so get/set/evict are O(log n).

use std::collections::{BTreeMap, HashMap};
use std::time::{Duration, Instant};

struct Item {
    value: Vec<u8>,
    tick: u64,
    expires: Option<Instant>,
}

pub struct Cache {
    items: HashMap<Vec<u8>, Item>,
    order: BTreeMap<u64, Vec<u8>>,
    tick: u64,
    capacity: usize,
    pub evictions: u64,
}

impl Cache {
    pub fn new(capacity: usize) -> Self {
        Cache { items: HashMap::new(), order: BTreeMap::new(), tick: 0, capacity: capacity.max(1), evictions: 0 }
    }

    fn touch(&mut self, key: &[u8]) {
        self.tick += 1;
        let t = self.tick;
        if let Some(item) = self.items.get_mut(key) {
            self.order.remove(&item.tick);
            item.tick = t;
            self.order.insert(t, key.to_vec());
        }
    }

    fn expired(&self, key: &[u8], now: Instant) -> bool {
        matches!(self.items.get(key), Some(Item { expires: Some(e), .. }) if *e <= now)
    }

    fn remove_internal(&mut self, key: &[u8]) -> bool {
        match self.items.remove(key) {
            Some(item) => {
                self.order.remove(&item.tick);
                true
            }
            None => false,
        }
    }

    pub fn get(&mut self, key: &[u8], now: Instant) -> Option<Vec<u8>> {
        if self.expired(key, now) {
            self.remove_internal(key);
            return None;
        }
        if !self.items.contains_key(key) {
            return None;
        }
        self.touch(key);
        self.items.get(key).map(|i| i.value.clone())
    }

    pub fn set(&mut self, key: &[u8], value: Vec<u8>, ttl: Option<Duration>, now: Instant) {
        self.remove_internal(key);
        while self.items.len() >= self.capacity {
            // evict the least recently used key
            let (&oldest, _) = self.order.iter().next().expect("non-empty");
            let victim = self.order.remove(&oldest).unwrap();
            self.items.remove(&victim);
            self.evictions += 1;
        }
        self.tick += 1;
        self.order.insert(self.tick, key.to_vec());
        self.items.insert(key.to_vec(), Item { value, tick: self.tick, expires: ttl.map(|d| now + d) });
    }

    pub fn delete(&mut self, key: &[u8]) -> bool { self.remove_internal(key) }

    pub fn exists(&mut self, key: &[u8], now: Instant) -> bool { self.get(key, now).is_some() }

    /// Set a TTL on an existing key; false if it does not exist.
    pub fn expire(&mut self, key: &[u8], ttl: Duration, now: Instant) -> bool {
        if self.expired(key, now) {
            self.remove_internal(key);
            return false;
        }
        match self.items.get_mut(key) {
            Some(item) => {
                item.expires = Some(now + ttl);
                true
            }
            None => false,
        }
    }

    /// Redis TTL semantics: -2 missing, -1 no expiry, else whole seconds remaining.
    pub fn ttl(&mut self, key: &[u8], now: Instant) -> i64 {
        if self.expired(key, now) {
            self.remove_internal(key);
            return -2;
        }
        match self.items.get(key) {
            None => -2,
            Some(Item { expires: None, .. }) => -1,
            // rounded to the nearest second, as Redis does
            Some(Item { expires: Some(e), .. }) => ((e.saturating_duration_since(now).as_millis() + 500) / 1000) as i64,
        }
    }

    pub fn len(&self) -> usize { self.items.len() }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn evicts_least_recently_used() {
        let now = Instant::now();
        let mut c = Cache::new(2);
        c.set(b"a", b"1".to_vec(), None, now);
        c.set(b"b", b"2".to_vec(), None, now);
        assert_eq!(c.get(b"a", now), Some(b"1".to_vec())); // a is now the most recent
        c.set(b"c", b"3".to_vec(), None, now);              // evicts b
        assert_eq!(c.get(b"b", now), None);
        assert_eq!(c.get(b"a", now), Some(b"1".to_vec()));
        assert_eq!(c.get(b"c", now), Some(b"3".to_vec()));
        assert_eq!(c.evictions, 1);
        assert_eq!(c.len(), 2);
    }

    #[test]
    fn overwrite_does_not_evict() {
        let now = Instant::now();
        let mut c = Cache::new(2);
        c.set(b"a", b"1".to_vec(), None, now);
        c.set(b"b", b"2".to_vec(), None, now);
        c.set(b"a", b"9".to_vec(), None, now);
        assert_eq!(c.len(), 2);
        assert_eq!(c.evictions, 0);
        assert_eq!(c.get(b"a", now), Some(b"9".to_vec()));
    }

    #[test]
    fn expiry_and_ttl() {
        let now = Instant::now();
        let mut c = Cache::new(10);
        c.set(b"k", b"v".to_vec(), Some(Duration::from_secs(10)), now);
        assert_eq!(c.ttl(b"k", now), 10);
        assert_eq!(c.ttl(b"missing", now), -2);
        c.set(b"p", b"v".to_vec(), None, now);
        assert_eq!(c.ttl(b"p", now), -1);
        let later = now + Duration::from_secs(11);
        assert_eq!(c.get(b"k", later), None);
        assert!(!c.expire(b"k", Duration::from_secs(5), later));
        assert!(c.expire(b"p", Duration::from_secs(5), later));
        assert_eq!(c.ttl(b"p", later), 5);
    }
}
