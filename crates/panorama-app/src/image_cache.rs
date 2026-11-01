use crate::services::{Job, Services};
use futures::{StreamExt, channel::mpsc};
use gpui::{Context, Hsla, RenderImage, Task, Window};
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

/// GPUI has no blur filter, so a blurred copy is baked here: small, since it is all low
/// frequencies and scales up without showing it.
const BLURRED: (f32, f32) = (640.0, 360.0);
const BLUR_SIGMA: f32 = 18.0;

fn blur(decoded: DecodedImage) -> Result<DecodedImage, ImageError> {
    let (width, height) = (decoded.width, decoded.height);
    let rgba = image::RgbaImage::from_raw(width, height, decoded.rgba).ok_or(ImageError::Decode)?;
    Ok(DecodedImage {
        width,
        height,
        rgba: image::imageops::fast_blur(&rgba, BLUR_SIGMA).into_raw(),
    })
}

/// Everything a finished load needs, computed before the pixels reach GPUI.
fn prepare(
    decoded: DecodedImage,
    blurred: bool,
) -> Result<(Arc<RenderImage>, Option<Hsla>), ImageError> {
    let tint = crate::tint::dominant(&decoded.rgba);
    let decoded = if blurred { blur(decoded)? } else { decoded };
    Ok((render_image(decoded)?, tint))
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
    /// Newest entry matching `matches`, read without changing eviction order.
    pub fn latest(&self, matches: impl Fn(&K) -> bool) -> Option<&V> {
        self.entries
            .iter()
            .rev()
            .find(|entry| matches(&entry.0))
            .map(|entry| &entry.1)
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

/// Where an image comes from. Fixture stills are matched here, before `ImageUrl::parse`, so
/// the core's https-only rule stays as it is.
#[derive(Clone, PartialEq, Eq, Hash)]
enum Source {
    Remote(ImageUrl),
    Fixture(&'static str),
}

#[derive(Clone, PartialEq, Eq, Hash)]
struct Key {
    url: Source,
    width: u32,
    height: u32,
    blurred: bool,
}

/// Cached renderer handle and its fade-in origin.
#[derive(Clone)]
pub struct CachedImage {
    /// GPUI upload handle, bypassing GPUI's unbounded asset cache.
    pub image: Arc<RenderImage>,
    /// Completion time for image opacity.
    pub loaded: Instant,
    /// The color this image reads as, for placeholders of its larger siblings.
    pub tint: Option<Hsla>,
}

/// 160 MiB pixel LRU and up to six cancellable visible image loads.
pub struct ImageCache {
    lru: ByteLru<Key, CachedImage>,
    pending: HashMap<Key, Task<()>>,
    visible: HashSet<Key>,
    failed: HashSet<Key>,
    fading: bool,
    /// Bundled `fixture://` stills resolve only in fixture runs, never from addon data.
    fixtures: bool,
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
            fixtures: false,
            _release: None,
        }
    }
}

impl ImageCache {
    /// Allocate the single app-wide cache and release every atlas entry on teardown.
    pub fn new(fixtures: bool, cx: &mut Context<Self>) -> Self {
        let mut cache = Self {
            fixtures,
            ..Self::default()
        };
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
        self.key(url, bounds, scale).is_none_or(|key| {
            self.visible.insert(key.clone());
            self.failed.contains(&key)
        })
    }
    fn key(&self, url: &str, bounds: (f32, f32), scale: f32) -> Option<Key> {
        let url = match url.strip_prefix("fixture://") {
            Some(_) if !self.fixtures => return None,
            Some(name) => Source::Fixture(crate::fixtures::STILLS.iter().find(|s| s.0 == name)?.0),
            None => Source::Remote(ImageUrl::parse(url).ok()?),
        };
        Some(Key {
            url,
            width: bucket(bounds.0, scale),
            height: bucket(bounds.1, scale),
            blurred: false,
        })
    }
    /// Read a cached offscreen tile without keeping a load alive.
    pub fn cached(&mut self, url: &str, bounds: (f32, f32), scale: f32) -> Option<CachedImage> {
        let key = self.key(url, bounds, scale)?;
        self.lru.get(&key).cloned()
    }
    /// The color of `url` at whatever size it was last decoded, without touching eviction order.
    pub fn tint(&self, url: &str) -> Option<Hsla> {
        let key = self.key(url, (0.0, 0.0), 1.0)?;
        self.lru.latest(|cached| cached.url == key.url)?.tint
    }
    /// The sharp copy of `url` last on screen, at whatever size.
    pub fn newest(&self, url: &str) -> Option<Arc<RenderImage>> {
        let key = self.key(url, (0.0, 0.0), 1.0)?;
        let found = self
            .lru
            .latest(|cached| cached.url == key.url && !cached.blurred)?;
        Some(found.image.clone())
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
        let key = self.key(url, bounds, window.scale_factor())?;
        self.fetch(key, services, window, cx)
    }
    /// The heavily blurred copy of an image, for crossfading over the sharp one.
    pub fn request_blurred(
        &mut self,
        url: &str,
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<CachedImage> {
        let key = Key {
            blurred: true,
            ..self.key(url, BLURRED, 1.0)?
        };
        self.fetch(key, services, window, cx)
    }
    fn fetch(
        &mut self,
        key: Key,
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> Option<CachedImage> {
        self.visible.insert(key.clone());
        if let Some(entry) = self.lru.get(&key) {
            self.fading |=
                Instant::now().duration_since(entry.loaded) < crate::theme::DURATION_STANDARD;
            return Some(entry.clone());
        }
        // A resize changes the size bucket every few pixels; show the same image at its last
        // size until this one decodes, otherwise the artwork blanks for the whole drag.
        let stale = self
            .lru
            .latest(|cached| cached.url == key.url && cached.blurred == key.blurred)
            .cloned();
        if self.failed.contains(&key) || self.pending.contains_key(&key) || self.pending.len() >= 6
        {
            return stale;
        }
        let blurred = key.blurred;
        let (sender, mut receiver) = mpsc::unbounded();
        let job = match &key.url {
            Source::Remote(url) => {
                let loader = services.images.clone()?;
                let request = ImageRequest {
                    url: url.sized_for(key.width),
                    target: ImageTarget {
                        max_width: key.width,
                        max_height: key.height,
                    },
                };
                Job(services.runtime.spawn(async move {
                    let result = match loader.load(request).await {
                        Ok(decoded) => {
                            tokio::task::spawn_blocking(move || prepare(decoded, blurred))
                                .await
                                .unwrap_or(Err(ImageError::Decode))
                        }
                        Err(error) => Err(error),
                    };
                    let _ = sender.unbounded_send(result);
                }))
            }
            Source::Fixture(name) => {
                let bytes = crate::fixtures::STILLS.iter().find(|s| s.0 == *name)?.1;
                Job(services.runtime.spawn(async move {
                    let result = tokio::task::spawn_blocking(move || {
                        let rgba = image::load_from_memory(bytes)
                            .map_err(|_| ImageError::Decode)?
                            .into_rgba8();
                        prepare(
                            DecodedImage {
                                width: rgba.width(),
                                height: rgba.height(),
                                rgba: rgba.into_raw(),
                            },
                            blurred,
                        )
                    })
                    .await
                    .unwrap_or(Err(ImageError::Decode));
                    let _ = sender.unbounded_send(result);
                }))
            }
        };
        let completed = key.clone();
        let task = cx.spawn_in(window, async move |entity, cx| {
            let _job = job;
            if let Some(result) = receiver.next().await {
                let _ = cx.update(|window, cx| {
                    entity.update(cx, |cache, cx| {
                        cache.pending.remove(&completed);
                        match result {
                            Ok((image, tint)) => {
                                let bytes = image.as_bytes(0).map_or(0, |b| b.len());
                                // A resized copy of an image already shown must not fade in again.
                                let loaded = cache
                                    .lru
                                    .latest(|cached| {
                                        cached.url == completed.url
                                            && cached.blurred == completed.blurred
                                    })
                                    .map_or_else(Instant::now, |entry| entry.loaded);
                                cache.lru.insert(
                                    completed,
                                    CachedImage {
                                        image,
                                        loaded,
                                        tint,
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
        stale
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
    fn latest_returns_newest_match_without_touching_order() {
        let mut cache = ByteLru::new(100);
        cache.insert(("a", 64), "a-small", 1, |_| {});
        cache.insert(("b", 64), "b", 1, |_| {});
        cache.insert(("a", 128), "a-large", 1, |_| {});
        assert_eq!(cache.latest(|key| key.0 == "a"), Some(&"a-large"));
        assert_eq!(cache.latest(|key| key.0 == "c"), None);
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
    fn fixture_urls_resolve_only_in_fixture_runs() {
        let fixtures = ImageCache {
            fixtures: true,
            ..ImageCache::default()
        };
        let live = ImageCache::default();
        let key = |cache: &ImageCache, url| cache.key(url, (100.0, 100.0), 1.0);
        assert!(key(&fixtures, "fixture://dusk.jpg").is_some());
        assert!(key(&fixtures, "fixture://missing.jpg").is_none());
        assert!(key(&fixtures, "http://dusk.jpg").is_none());
        assert!(key(&live, "fixture://dusk.jpg").is_none());
    }
    #[test]
    fn blur_spreads_a_hard_edge() {
        let (width, height) = (64, 8);
        let rgba = (0..width * height)
            .flat_map(|i| [if i % width < width / 2 { 0 } else { 255 }, 0, 0, 255])
            .collect();
        let blurred = blur(DecodedImage {
            width,
            height,
            rgba,
        })
        .unwrap();
        let red = |x: u32| blurred.rgba[(x * 4) as usize];
        assert!(red(30) > 40 && red(33) < 215, "{} {}", red(30), red(33));
        assert!(red(30) < red(33));
    }
    #[test]
    fn tint_is_found_from_any_decoded_size() {
        let mut cache = ImageCache::default();
        let url = "https://example.com/still.jpg";
        let image = render_image(DecodedImage {
            width: 1,
            height: 1,
            rgba: vec![0, 0, 0, 255],
        })
        .unwrap();
        let tint = gpui::red();
        let key = cache.key(url, (372.0, 186.0), 2.0).unwrap();
        let entry = CachedImage {
            image,
            loaded: Instant::now(),
            tint: Some(tint),
        };
        cache.lru.insert(key, entry, 1, |_| {});
        assert_eq!(cache.tint(url), Some(tint));
        assert_eq!(cache.tint("https://example.com/other.jpg"), None);
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
