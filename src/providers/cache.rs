//! Small in-memory TTL cache and sliding-window rate limiter for providers.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// A bounded cache. When full, expired entries are dropped first, then the
/// oldest entry. Intended for a few hundred provider responses at most.
pub struct TtlCache<V> {
    ttl: Duration,
    capacity: usize,
    entries: Mutex<HashMap<String, (Instant, V)>>,
}

impl<V: Clone> TtlCache<V> {
    pub fn new(ttl: Duration, capacity: usize) -> Self {
        TtlCache {
            ttl,
            capacity: capacity.max(1),
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub fn get(&self, key: &str) -> Option<V> {
        let map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        map.get(key)
            .filter(|(at, _)| at.elapsed() < self.ttl)
            .map(|(_, v)| v.clone())
    }

    pub fn insert(&self, key: String, value: V) {
        let mut map = self.entries.lock().unwrap_or_else(|p| p.into_inner());
        if map.len() >= self.capacity && !map.contains_key(&key) {
            let ttl = self.ttl;
            map.retain(|_, (at, _)| at.elapsed() < ttl);
            if map.len() >= self.capacity
                && let Some(oldest) = map
                    .iter()
                    .min_by_key(|(_, (at, _))| *at)
                    .map(|(k, _)| k.clone())
            {
                map.remove(&oldest);
            }
        }
        map.insert(key, (Instant::now(), value));
    }
}

/// Allows at most `max` acquisitions per `window`.
pub struct RateLimiter {
    max: usize,
    window: Duration,
    hits: Mutex<VecDeque<Instant>>,
}

impl RateLimiter {
    pub fn new(max: usize, window: Duration) -> Self {
        RateLimiter {
            max,
            window,
            hits: Mutex::new(VecDeque::new()),
        }
    }

    pub fn try_acquire(&self) -> bool {
        let now = Instant::now();
        let mut hits = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        while hits
            .front()
            .is_some_and(|t| now.duration_since(*t) >= self.window)
        {
            hits.pop_front();
        }
        if hits.len() >= self.max {
            return false;
        }
        hits.push_back(now);
        true
    }

    /// Time until a slot frees up (zero when one is free now).
    fn wait_time(&self) -> Duration {
        let hits = self.hits.lock().unwrap_or_else(|p| p.into_inner());
        if hits.len() < self.max {
            return Duration::ZERO;
        }
        hits.front()
            .map(|t| self.window.saturating_sub(t.elapsed()))
            .unwrap_or_default()
    }

    /// Acquires a slot, waiting up to `max_wait` for one to free up, so a
    /// short burst of searches is delayed rather than refused.
    pub async fn acquire(&self, mut max_wait: Duration) -> bool {
        loop {
            if self.try_acquire() {
                return true;
            }
            let wait = self.wait_time().max(Duration::from_millis(10));
            if wait > max_wait {
                return false;
            }
            tokio::time::sleep(wait).await;
            max_wait -= wait;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_returns_fresh_and_hides_expired() {
        let c = TtlCache::new(Duration::from_secs(60), 10);
        c.insert("a".into(), 1);
        assert_eq!(c.get("a"), Some(1));
        assert_eq!(c.get("b"), None);

        let expired = TtlCache::new(Duration::ZERO, 10);
        expired.insert("a".into(), 1);
        assert_eq!(expired.get("a"), None);
    }

    #[test]
    fn cache_is_bounded() {
        let c = TtlCache::new(Duration::from_secs(60), 2);
        c.insert("a".into(), 1);
        c.insert("b".into(), 2);
        c.insert("c".into(), 3);
        let present = ["a", "b", "c"]
            .iter()
            .filter(|k| c.get(k).is_some())
            .count();
        assert_eq!(present, 2);
        assert_eq!(c.get("c"), Some(3));
    }

    #[tokio::test]
    async fn limiter_waits_briefly_for_a_free_slot() {
        let l = RateLimiter::new(1, Duration::from_millis(50));
        assert!(l.acquire(Duration::ZERO).await);
        assert!(!l.acquire(Duration::from_millis(5)).await);
        assert!(l.acquire(Duration::from_millis(500)).await);
    }

    #[test]
    fn limiter_blocks_after_max() {
        let l = RateLimiter::new(2, Duration::from_secs(60));
        assert!(l.try_acquire());
        assert!(l.try_acquire());
        assert!(!l.try_acquire());
    }
}
