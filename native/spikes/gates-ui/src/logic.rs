use serde::Deserialize;
use std::{
    collections::{HashMap, HashSet},
    hash::Hash,
    io::Read,
    time::{Duration, Instant},
};

pub const JSON_CAP: usize = 2 * 1024 * 1024;

#[derive(Clone, Debug, Deserialize)]
pub struct Poster {
    pub id: String,
    pub name: String,
    pub poster: String,
}

#[derive(Deserialize)]
struct Catalog {
    metas: Vec<serde_json::Value>,
}

pub fn read_capped(reader: impl Read, cap: usize) -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    reader
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > cap {
        return Err(format!("Body exceeds {cap} bytes"));
    }
    Ok(bytes)
}

pub fn parse_catalog(bytes: &[u8]) -> Result<(usize, Vec<Poster>), String> {
    if bytes.len() > JSON_CAP {
        return Err("Catalog exceeds 2 MiB".into());
    }
    let catalog: Catalog = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let count = catalog.metas.len();
    let posters = catalog
        .metas
        .into_iter()
        .filter_map(|item| serde_json::from_value::<Poster>(item).ok())
        .filter(|item| {
            reqwest::Url::parse(&item.poster)
                .is_ok_and(|url| url.scheme() == "https" && url.host_str().is_some())
        })
        .collect();
    Ok((count, posters))
}

struct Entry<V> {
    value: V,
    bytes: usize,
    used: u64,
}

pub struct ByteLru<K, V> {
    entries: HashMap<K, Entry<V>>,
    pub bytes: usize,
    cap: usize,
    clock: u64,
}

impl<K: Eq + Hash + Clone, V> ByteLru<K, V> {
    pub fn new(cap: usize) -> Self {
        Self {
            entries: HashMap::new(),
            bytes: 0,
            cap,
            clock: 0,
        }
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn contains(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        self.clock += 1;
        self.entries.get_mut(key).map(|e| {
            e.used = self.clock;
            &e.value
        })
    }
    pub fn insert(&mut self, key: K, value: V, bytes: usize, pinned: &HashSet<K>) -> Vec<V> {
        let mut evicted = Vec::new();
        let replace_bytes = self.entries.get(&key).map_or(0, |e| e.bytes);
        let reclaimable: usize = self
            .entries
            .iter()
            .filter(|(k, _)| !pinned.contains(*k) && **k != key)
            .map(|(_, e)| e.bytes)
            .sum();
        if bytes > self.cap || self.bytes - replace_bytes + bytes > self.cap + reclaimable {
            evicted.push(value);
            return evicted;
        }
        if let Some(old) = self.entries.remove(&key) {
            self.bytes -= old.bytes;
            evicted.push(old.value);
        }
        while self.bytes + bytes > self.cap {
            let victim = self
                .entries
                .iter()
                .filter(|(k, _)| !pinned.contains(*k))
                .min_by_key(|(_, e)| e.used)
                .map(|(k, _)| k.clone())
                .unwrap();
            let old = self.entries.remove(&victim).unwrap();
            self.bytes -= old.bytes;
            evicted.push(old.value);
        }
        self.clock += 1;
        self.bytes += bytes;
        self.entries.insert(
            key,
            Entry {
                value,
                bytes,
                used: self.clock,
            },
        );
        evicted
    }
}

pub fn bezier(x: f32) -> f32 {
    let x = x.clamp(0.0, 1.0);
    if x == 0.0 || x == 1.0 {
        return x;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for _ in 0..24 {
        let t = (lo + hi) * 0.5;
        let u = 1.0 - t;
        let sample = 3.0 * u * u * t * 0.22 + 3.0 * u * t * t * 0.36 + t * t * t;
        if sample < x {
            lo = t;
        } else {
            hi = t;
        }
    }
    let t = (lo + hi) * 0.5;
    1.0 - (1.0 - t).powi(3)
}

#[derive(Clone, Copy)]
pub struct Tween {
    pub from: f32,
    pub to: f32,
    pub start: Instant,
    pub duration: Duration,
    pub eased: bool,
}

impl Tween {
    pub fn fixed(value: f32) -> Self {
        Self {
            from: value,
            to: value,
            start: Instant::now(),
            duration: Duration::ZERO,
            eased: false,
        }
    }
    pub fn progress(&self, now: Instant) -> f32 {
        if self.duration.is_zero() {
            return 1.0;
        }
        (now.saturating_duration_since(self.start).as_secs_f32() / self.duration.as_secs_f32())
            .clamp(0.0, 1.0)
    }
    pub fn sample(&self, progress: f32) -> f32 {
        self.from
            + (self.to - self.from)
                * if self.eased {
                    bezier(progress)
                } else {
                    progress
                }
    }
    pub fn value(&self, now: Instant) -> f32 {
        self.sample(self.progress(now))
    }
    pub fn retarget(&mut self, to: f32, duration: Duration, eased: bool, now: Instant) {
        *self = Self {
            from: self.value(now),
            to,
            start: now,
            duration,
            eased,
        };
    }
    pub fn active(&self, now: Instant) -> bool {
        self.from != self.to && self.progress(now) < 1.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn byte_cap_evicts_least_recent_unpinned_and_rejects_oversize() {
        let mut cache = ByteLru::new(10);
        let mut pinned = HashSet::new();
        assert!(cache.insert(1, "one", 4, &pinned).is_empty());
        cache.insert(2, "two", 4, &pinned);
        cache.get(&1);
        assert_eq!(cache.insert(3, "three", 4, &pinned), ["two"]);
        pinned.insert(1);
        assert_eq!(cache.insert(4, "four", 6, &pinned), ["three"]);
        assert_eq!(cache.bytes, 10);
        assert_eq!(cache.insert(5, "huge", 11, &pinned), ["huge"]);
        pinned.insert(4);
        assert_eq!(cache.insert(6, "six", 4, &pinned), ["six"]);
        assert_eq!(cache.len(), 2);
        assert_eq!(cache.insert(1, "replacement", 3, &pinned), ["one"]);
        assert_eq!(cache.bytes, 9);
    }
    #[test]
    fn easing_known_parametric_values() {
        assert_eq!(bezier(0.0), 0.0);
        assert_eq!(bezier(1.0), 1.0);
        assert!((bezier(0.3425) - 0.875).abs() < 0.00001);
        assert!((bezier(0.1590625) - 0.578125).abs() < 0.00001);
        for i in 0..100 {
            assert!(bezier(i as f32 / 100.0) <= bezier((i + 1) as f32 / 100.0));
        }
    }
    #[test]
    fn reversal_preserves_current_value_in_both_directions() {
        let start = Instant::now();
        let mut tween = Tween {
            from: 0.0,
            to: 1.0,
            start,
            duration: Duration::from_millis(220),
            eased: true,
        };
        let now = start + Duration::from_millis(70);
        let value = tween.value(now);
        tween.retarget(0.0, Duration::from_millis(160), true, now);
        assert_eq!(tween.value(now), value);
        assert!(tween.value(now + Duration::from_millis(30)) < value);
        let now = now + Duration::from_millis(30);
        let value = tween.value(now);
        tween.retarget(1.0, Duration::from_millis(220), true, now);
        assert_eq!(tween.value(now), value);
        assert_eq!(tween.value(now + Duration::from_millis(220)), 1.0);
    }
    #[test]
    fn catalog_filters_insecure_missing_and_malformed_items() {
        let data = br#"{"metas":[{"id":"a","name":"A","poster":"https://example.com/a.jpg"},{"id":"b","name":"B","poster":"http://example.com/b.jpg"},{"id":"c","name":"C"},{"id":"d","name":"D","poster":"file:///a"},{"id":"e","name":"E","poster":"https://"}]}"#;
        let (raw_count, posters) = parse_catalog(data).unwrap();
        assert_eq!(raw_count, 5);
        assert_eq!(posters.len(), 1);
        assert_eq!(posters[0].id, "a");
        assert!(parse_catalog(&vec![b' '; JSON_CAP + 1]).is_err());
        assert!(read_capped(&b"12345"[..], 4).is_err());
        assert_eq!(read_capped(&b"1234"[..], 4).unwrap(), b"1234");
    }
}
