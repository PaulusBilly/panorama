use crate::services::{Job, Services};
use futures::{StreamExt, channel::mpsc};
use gpui::{Context, RenderImage, Task, Window};
use panorama_core::images::{DecodedImage, ImageError, ImageRequest, ImageTarget, ImageUrl};
use std::{
    collections::{HashMap, HashSet, VecDeque},
    hash::Hash,
    sync::Arc,
    time::Instant,
};

/// Round physical image bounds up to reusable 64-pixel steps.
pub fn bucket(logical: f32, scale: f32) -> u32 {
    if !logical.is_finite() || !scale.is_finite() || logical <= 0.0 || scale <= 0.0 {
        return 64;
    }
    ((logical * scale).ceil().min(4096.0) as u32).div_ceil(64) * 64
}

/// Convert straight RGBA8 to GPUI's straight BGRA8 image frames.
pub fn render_image(mut decoded: DecodedImage) -> Result<Arc<RenderImage>, ImageError> {
    if decoded.width == 0
        || decoded.height == 0
        || decoded.width > 4096
        || decoded.height > 4096
        || decoded.rgba.len() != decoded.width as usize * decoded.height as usize * 4
    {
        return Err(ImageError::Decode);
    }
    for pixel in decoded.rgba.as_chunks_mut::<4>().0 {
        pixel.swap(0, 2);
    }
    let rgba = image::RgbaImage::from_raw(decoded.width, decoded.height, decoded.rgba)
        .ok_or(ImageError::Decode)?;
    Ok(Arc::new(RenderImage::new(vec![image::Frame::new(rgba)])))
}

/// Byte-bounded least-recently-used cache with explicit disposal on removal.
pub struct ByteLru<K, V> {
    entries: VecDeque<(K, V, usize)>,
    /// Current retained pixel bytes.
    pub bytes: usize,
    capacity: usize,
}

impl<K: Eq, V> ByteLru<K, V> {
    /// Create a fixed byte budget.
    pub fn new(capacity: usize) -> Self {
        Self {
            entries: VecDeque::new(),
            bytes: 0,
            capacity,
        }
    }
    /// Touch an existing entry and make it most recently used.
    pub fn get(&mut self, key: &K) -> Option<&V> {
        let index = self.entries.iter().position(|entry| &entry.0 == key)?;
        let entry = self.entries.remove(index)?;
        self.entries.push_back(entry);
        self.entries.back().map(|entry| &entry.1)
    }
    /// Insert while disposing replacements, oversized values and every eviction.
    pub fn insert(&mut self, key: K, value: V, bytes: usize, mut drop_image: impl FnMut(V)) {
        if let Some(index) = self.entries.iter().position(|entry| entry.0 == key)
            && let Some((_, old, count)) = self.entries.remove(index)
        {
            self.bytes -= count;
            drop_image(old);
        }
        if bytes > self.capacity {
            drop_image(value);
            return;
        }
        while self.bytes > self.capacity - bytes {
            let Some((_, old, count)) = self.entries.pop_front() else {
                break;
            };
            self.bytes -= count;
            drop_image(old);
        }
        self.bytes += bytes;
        self.entries.push_back((key, value, bytes));
    }
    /// Release all renderer resources when the owning route disappears.
    pub fn clear(&mut self, mut drop_image: impl FnMut(V)) {
        for (_, value, _) in self.entries.drain(..) {
            drop_image(value);
        }
        self.bytes = 0;
    }
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    url: ImageUrl,
    width: u32,
    height: u32,
}

/// Cached renderer handle and its fade-in origin.
#[derive(Clone)]
pub struct CachedImage {
    /// GPUI upload handle, bypassing GPUI's unbounded asset cache.
    pub image: Arc<RenderImage>,
    /// Completion time for image opacity.
    pub loaded: Instant,
}

/// 160 MiB pixel LRU and up to six cancellable visible image loads.
pub struct ImageCache {
    lru: ByteLru<Key, CachedImage>,
    pending: HashMap<Key, Task<()>>,
    visible: HashSet<Key>,
    failed: HashSet<Key>,
    fading: bool,
    _release: Option<gpui::Subscription>,
}

impl Default for ImageCache {
    fn default() -> Self {
        Self {
            lru: ByteLru::new(160 * 1024 * 1024),
            pending: HashMap::new(),
            visible: HashSet::new(),
            failed: HashSet::new(),
            fading: false,
            _release: None,
        }
    }
}

impl ImageCache {
    /// Allocate the single app-wide cache and release every atlas entry on teardown.
    pub fn new(cx: &mut Context<Self>) -> Self {
        let mut cache = Self::default();
        cache._release = Some(cx.on_release(|cache, cx| cache.clear(cx)));
        cache
    }
    /// Whether visible images need loading or fade-in frames.
    pub fn moving(&self) -> bool {
        self.fading || !self.pending.is_empty()
    }
    /// Cancel visible loads after leaving Home.
    pub fn cancel(&mut self) {
        self.pending.clear();
    }

    /// Begin collecting this frame's image interests.
    pub fn begin_frame(&mut self) {
        self.visible.clear();
        self.fading = false;
    }
    /// Cancel loads for cards that have left the virtualized visible range.
    pub fn finish_frame(&mut self) {
        self.pending.retain(|key, _| self.visible.contains(key));
        self.failed.retain(|key| self.visible.contains(key));
    }
    /// Whether this image failed and its next fallback should be selected.
    pub fn failed(&mut self, url: &str, bounds: (f32, f32), scale: f32) -> bool {
        Self::key(url, bounds, scale).is_none_or(|key| {
            self.visible.insert(key.clone());
            self.failed.contains(&key)
        })
    }
    fn key(url: &str, bounds: (f32, f32), scale: f32) -> Option<Key> {
        Some(Key {
            url: ImageUrl::parse(url).ok()?,
            width: bucket(bounds.0, scale),
            height: bucket(bounds.1, scale),
        })
    }
    /// Read a cached offscreen tile without keeping a load alive.
    pub fn cached(&mut self, url: &str, bounds: (f32, f32), scale: f32) -> Option<CachedImage> {
        let key = Self::key(url, bounds, scale)?;
        self.lru.get(&key).cloned()
    }
    /// Request only images used by this frame; conversion also stays off GPUI.
    pub fn request(
        &mut self,
        url: &str,
        bounds: (f32, f32),
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<CachedImage> {
        let key = Self::key(url, bounds, window.scale_factor())?;
        self.visible.insert(key.clone());
        if let Some(entry) = self.lru.get(&key) {
            self.fading |=
                Instant::now().duration_since(entry.loaded) < crate::theme::DURATION_STANDARD;
            return Some(entry.clone());
        }
        if self.failed.contains(&key) || self.pending.contains_key(&key) || self.pending.len() >= 6
        {
            return None;
        }
        let loader = services.images.clone()?;
        let request = ImageRequest {
            url: key.url.clone(),
            target: ImageTarget {
                max_width: key.width,
                max_height: key.height,
            },
        };
        let (sender, mut receiver) = mpsc::unbounded();
        let job = Job(services.runtime.spawn(async move {
            let result = match loader.load(request).await {
                Ok(decoded) => tokio::task::spawn_blocking(move || render_image(decoded))
                    .await
                    .unwrap_or(Err(ImageError::Decode)),
                Err(error) => Err(error),
            };
            let _ = sender.unbounded_send(result);
        }));
        let completed = key.clone();
        let task = cx.spawn_in(window, async move |entity, cx| {
            let _job = job;
            if let Some(result) = receiver.next().await {
                let _ = cx.update(|window, cx| {
                    entity.update(cx, |cache, cx| {
                        cache.pending.remove(&completed);
                        match result {
                            Ok(image) => {
                                let bytes = image.as_bytes(0).map_or(0, |b| b.len());
                                cache.lru.insert(
                                    completed,
                                    CachedImage {
                                        image,
                                        loaded: Instant::now(),
                                    },
                                    bytes,
                                    |entry| {
                                        cx.drop_image(entry.image, Some(window));
                                    },
                                );
                            }
                            Err(_) => {
                                cache.failed.insert(completed);
                            }
                        }
                        cx.notify();
                    })
                });
            }
        });
        self.pending.insert(key, task);
        None
    }
    /// Explicitly remove renderer atlas entries when a route is released.
    pub fn clear(&mut self, cx: &mut gpui::App) {
        self.pending.clear();
        self.lru.clear(|entry| {
            cx.drop_image(entry.image, None);
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn physical_size_buckets() {
        for (logical, scale, expected) in [
            (1.0, 1.0, 64),
            (64.0, 1.0, 64),
            (64.01, 1.0, 128),
            (372.0, 1.0, 384),
            (372.0, 1.5, 576),
            (372.0, 2.0, 768),
            (0.0, 1.0, 64),
            (f32::NAN, 1.0, 64),
            (10000.0, 2.0, 4096),
        ] {
            assert_eq!(bucket(logical, scale), expected);
        }
    }
    #[test]
    fn eviction_calls_disposer_and_accounts_every_byte() {
        let mut cache = ByteLru::new(10);
        let mut dropped = vec![];
        cache.insert(1, "one", 4, |v| dropped.push(v));
        cache.insert(2, "two", 4, |v| dropped.push(v));
        assert_eq!(cache.get(&1), Some(&"one"));
        cache.insert(3, "three", 6, |v| dropped.push(v));
        assert_eq!(dropped, ["two"]);
        assert_eq!(cache.bytes, 10);
        cache.insert(1, "replacement", 3, |v| dropped.push(v));
        assert_eq!(cache.bytes, 9);
        cache.insert(4, "oversized", 11, |v| dropped.push(v));
        assert_eq!(cache.bytes, 9);
        cache.clear(|v| dropped.push(v));
        assert_eq!(cache.bytes, 0);
        assert_eq!(dropped, ["two", "one", "oversized", "three", "replacement"]);
    }
    #[test]
    fn alpha_is_straight_and_channels_are_bgra() {
        let image = render_image(DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![200, 100, 50, 128],
        })
        .unwrap();
        assert_eq!(image.as_bytes(0).unwrap(), &[50, 100, 200, 128]);
    }
}
