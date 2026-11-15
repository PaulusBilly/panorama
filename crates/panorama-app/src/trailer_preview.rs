//! Trailers inside the interface. A lift card's preview looks up its film's trailer once it has
//! opened and plays it muted inside the artwork, fading in over it at the first frame and back
//! out after `PREVIEW_FOR`. Opening the film hands the playing clip on (see [`Handoff`]): it keeps
//! playing as the card grows, then as the film page's banner, drawn at the banner's size. A card
//! streams only as sharp as it needs, so the banner opens a sharper stream of the same trailer
//! a few seconds ahead, holds it on that frame, and crossfades to it as the first one gets
//! there. The artwork always stays underneath, so a film without a trailer, or one YouTube refuses, looks
//! exactly as it did.
use crate::{
    services::{Job, Services},
    theme::motion::{DURATION_DELIBERATE, DURATION_STANDARD, EASE_OUT},
    transition::Tween,
};
use gpui::{App, Context, Div, Global, RenderImage, Task, Window, div, img, prelude::*, px};
use panorama_core::addons::Kind;
use panorama_mpv::{Clip, ClipState, MAX_CLIP_WIDTH};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a trailer plays before giving way to the artwork again, counted from when it first
/// shows, across the card, the zoom and the banner.
const PREVIEW_FOR: Duration = Duration::from_secs(30);
/// How far ahead of the playing trailer a sharper stream is held, to have it open and on its
/// frame in time: a lookup and a stream's first frame each take about a second.
const SHARPEN_LEAD: f64 = 3.5;
/// Attempts at a sharper stream per visit; one that misses its moment is tried again.
const SHARPEN_TRIES: u8 = 3;
/// Frames with picture in them that decide how much letterbox to crop.
const BAR_SAMPLES: u8 = 8;
/// A channel this dark or darker counts as letterbox black (video black sits near 16).
const BLACK: u8 = 28;

/// A film's trailer, playing or on its way.
pub struct TrailerPreview {
    film: String,
    lookup: Option<Task<()>>,
    clip: Option<Clip>,
    image: Option<Arc<RenderImage>>,
    fade: Tween,
    /// Letterbox baked into the top and bottom of the video, as a share of its height, smallest
    /// seen so far, and how many frames with picture have been measured. Shown once
    /// `BAR_SAMPLES` agree, so the crop never jumps.
    bars: (f32, u8),
    /// When the trailer gives way to the artwork; set as it fades in, cleared once it has
    /// ended, so it plays only once.
    ends: Option<Instant>,
    /// The width of the stream playing, once known.
    width: u32,
    /// A sharper stream held on the frame at these seconds, waiting to take over.
    hold: Option<f64>,
    /// The lookup found nothing sharper, or nothing at all.
    gave_up: bool,
}

impl TrailerPreview {
    /// Looks up the trailer of film `id` for a place `size` points large and plays it there once
    /// found. `found` hands the clip back to whichever view owns this trailer by then.
    pub fn start<V: 'static>(
        kind: Kind,
        id: &str,
        size: (f32, f32),
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<V>,
        found: impl Fn(&mut V) -> Option<&mut TrailerPreview> + 'static,
    ) -> Self {
        let pixels = pixels(size, window);
        let lookup = Self::look_up(kind, id, pixels, pixels.0, None, services, cx, found);
        Self {
            film: id.to_owned(),
            lookup: Some(lookup),
            clip: None,
            image: None,
            fade: Tween::fixed(0.0),
            bars: (1.0, 0),
            ends: None,
            width: 0,
            hold: None,
            gave_up: false,
        }
    }

    /// A sharper stream of `base`'s trailer for a place `size` points large, opened
    /// `SHARPEN_LEAD` seconds ahead and held there; `None` before `base` reports a position.
    #[allow(clippy::too_many_arguments)]
    pub fn sharpen<V: 'static>(
        base: &TrailerPreview,
        kind: Kind,
        size: (f32, f32),
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<V>,
        found: impl Fn(&mut V) -> Option<&mut TrailerPreview> + 'static,
    ) -> Option<Self> {
        let hold = base.position()? + SHARPEN_LEAD;
        let pixels = pixels(size, window);
        let wanted = pixels.0.min(MAX_CLIP_WIDTH);
        let held = Some((hold, base.width));
        let lookup = Self::look_up(kind, &base.film, pixels, wanted, held, services, cx, found);
        Some(Self {
            film: base.film.clone(),
            lookup: Some(lookup),
            clip: None,
            image: None,
            fade: Tween::fixed(0.0),
            // The same trailer drawn at the same shape: the measured letterbox holds.
            bars: base.bars,
            ends: base.ends,
            width: 0,
            hold: Some(hold),
            gave_up: false,
        })
    }

    /// Finds a stream about `width` pixels wide and plays it at `pixels`; `held` opens it
    /// paused at those seconds, and only if it is wider than the one given.
    #[allow(clippy::too_many_arguments)]
    fn look_up<V: 'static>(
        kind: Kind,
        id: &str,
        pixels: (u32, u32),
        width: u32,
        held: Option<(f64, u32)>,
        services: &Arc<Services>,
        cx: &mut Context<V>,
        found: impl Fn(&mut V) -> Option<&mut TrailerPreview> + 'static,
    ) -> Task<()> {
        let mut job: Job<_> = services.trailer(kind, id.to_owned(), width);
        let film = id.to_owned();
        cx.spawn(async move |view, cx| {
            let result = (&mut job.0).await.ok().flatten();
            let _ = view.update(cx, |view, cx| {
                let Some(trailer) = found(view).filter(|trailer| trailer.film == film) else {
                    return;
                };
                if trailer.clip.is_some() {
                    return;
                }
                let sharper = |width: u32| held.is_none_or(|(_, playing)| width > playing);
                match result {
                    Some((variant, library)) if sharper(variant.width) => {
                        trailer.width = variant.width;
                        trailer.clip = library
                            .clip(
                                variant.url.as_str(),
                                pixels.0,
                                pixels.1,
                                held.map(|(at, _)| at),
                                held.is_some(),
                            )
                            .ok();
                        trailer.gave_up = trailer.clip.is_none();
                    }
                    _ => trailer.gave_up = true,
                }
                cx.notify();
            });
        })
    }

    pub fn film(&self) -> &str {
        &self.film
    }

    /// Whether a clip is playing, so the trailer can be handed on without its lookup.
    pub fn playing(&self) -> bool {
        self.clip
            .as_ref()
            .is_some_and(|clip| clip.state() != ClipState::Failed)
    }

    /// Seconds into the trailer, once it plays.
    pub fn position(&self) -> Option<f64> {
        self.clip.as_ref()?.position()
    }

    /// Whether the trailer has finished fading in and is still on.
    pub fn shown(&self) -> bool {
        self.ends.is_some() && !self.fade.moving(Instant::now(), false)
    }

    /// Draws later frames for a place `size` points large.
    pub fn resize(&self, size: (f32, f32), window: &Window) {
        if let Some(clip) = &self.clip {
            let pixels = pixels(size, window);
            clip.resize(pixels.0, pixels.1);
        }
    }

    /// Takes the clip's newest frame, if any, and ends the trailer once its time is up. The
    /// frame it replaces leaves the window's atlas after the next frame is drawn, once nothing
    /// paints it any more.
    pub fn advance(&mut self, window: &Window) {
        let now = Instant::now();
        if self.ends.is_some_and(|ends| now >= ends) {
            self.ends = None;
            self.stop();
            self.fade
                .retarget(0.0, DURATION_DELIBERATE, Some(EASE_OUT), now);
            return;
        }
        let Some(frame) = self.clip.as_ref().and_then(Clip::take_frame) else {
            if self
                .clip
                .as_ref()
                .is_some_and(|clip| clip.state() == ClipState::Failed)
            {
                self.clip = None;
            }
            return;
        };
        if self.bars.1 < BAR_SAMPLES
            && let Some(bar) = letterbox(&frame.bgra, frame.width, frame.height)
        {
            let share = bar as f32 / frame.height.max(1) as f32;
            self.bars = (self.bars.0.min(share), self.bars.1 + 1);
            if self.bars.1 == BAR_SAMPLES {
                self.fade
                    .retarget(1.0, DURATION_DELIBERATE, Some(EASE_OUT), now);
                self.ends = Some(now + PREVIEW_FOR);
            }
        }
        let Some(buffer) = image::RgbaImage::from_raw(frame.width, frame.height, frame.bgra) else {
            return;
        };
        // GPUI frames are BGRA, which is how the clip draws them.
        let image = Arc::new(RenderImage::new(vec![image::Frame::new(buffer)]));
        if let Some(old) = self.image.replace(image) {
            window.on_next_frame(move |window, _| {
                let _ = window.drop_image(old);
            });
        }
    }

    /// Stops playback, keeping the last frame so the artwork can settle with it.
    pub fn stop(&mut self) {
        self.lookup = None;
        self.clip = None;
    }

    /// Releases the last frame from the window's atlas.
    pub fn release(mut self, window: &Window) {
        self.stop();
        if let Some(old) = self.image.take() {
            window.on_next_frame(move |window, _| {
                let _ = window.drop_image(old);
            });
        }
    }

    /// Frames are needed while the clip plays or fades.
    pub fn moving(&self, reduced: bool) -> bool {
        self.playing() || self.fade.moving(Instant::now(), reduced)
    }

    /// The trailer over the artwork, `bounds` points and covering it; nothing before a frame.
    /// `shown` scales its opacity, so artwork that fades takes its trailer with it.
    pub fn layer(&self, bounds: (f32, f32), shown: f32, reduced: bool) -> Option<Div> {
        if self.bars.1 < BAR_SAMPLES {
            return None;
        }
        let image = self.image.clone()?;
        let size = image.size(0);
        let video = (size.width.0 as f32, size.height.0 as f32);
        let (left, top, width, height) = crop(video, self.bars.0 * video.1, bounds);
        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .opacity(self.fade.value(Instant::now(), reduced) * shown)
                .child(
                    img(image)
                        .absolute()
                        .left(px(left))
                        .top(px(top))
                        .w(px(width))
                        .h(px(height)),
                ),
        )
    }
}

/// Device pixels a clip is drawn at for a place `size` points large: 16:9 like the video,
/// covering the place, so mpv never adds bars of its own and the measured letterbox holds at
/// every size. The layer crops the overflow.
fn pixels(size: (f32, f32), window: &Window) -> (u32, u32) {
    let scale = window.scale_factor();
    let (width, height) = cover_16_9((size.0 * scale, size.1 * scale));
    (
        width.round().max(1.0) as u32,
        height.round().max(1.0) as u32,
    )
}

fn cover_16_9((width, height): (f32, f32)) -> (f32, f32) {
    const RATIO: f32 = 16.0 / 9.0;
    if width / height.max(1.0) >= RATIO {
        (width, width / RATIO)
    } else {
        (height * RATIO, height)
    }
}

/// A playing trailer on its way from a clicked card to its film page: the zoom draws it while
/// the card grows, and the page takes it for its banner. At most one is in flight.
#[derive(Default)]
pub struct Handoff(Option<TrailerPreview>);

impl Global for Handoff {}

impl Handoff {
    /// Hands a playing trailer on, releasing any earlier one that was never taken.
    pub fn give(trailer: TrailerPreview, window: &Window, cx: &mut App) {
        if let Some(stale) = cx.default_global::<Self>().0.replace(trailer) {
            stale.release(window);
        }
    }

    /// The trailer in flight for film `id`.
    pub fn get<'a>(id: &str, cx: &'a mut App) -> Option<&'a mut TrailerPreview> {
        cx.default_global::<Self>()
            .0
            .as_mut()
            .filter(|trailer| trailer.film == id)
    }

    /// Takes the trailer in flight for film `id`.
    pub fn take(id: &str, cx: &mut App) -> Option<TrailerPreview> {
        let slot = &mut cx.default_global::<Self>().0;
        if slot.as_ref().is_some_and(|trailer| trailer.film == id) {
            slot.take()
        } else {
            None
        }
    }

    /// Releases a trailer in flight unless it belongs to film `keep`, so one nobody takes
    /// (the zoom turned back, or another page opened) stops playing.
    pub fn settle(keep: Option<&str>, window: &Window, cx: &mut App) {
        if !cx.has_global::<Self>() {
            return;
        }
        let slot = &mut cx.global_mut::<Self>().0;
        if slot
            .as_ref()
            .is_some_and(|trailer| keep != Some(trailer.film.as_str()))
            && let Some(stale) = slot.take()
        {
            stale.release(window);
        }
    }
}

/// The film page's banner trailer: the one handed on from the card that opened the page, or one
/// of its own, played once per visit, and swapped for a sharper stream when it can be.
#[derive(Default)]
pub struct BannerTrailer {
    trailer: Option<TrailerPreview>,
    /// A sharper stream of the same trailer, held ahead until the playing one reaches it.
    sharper: Option<TrailerPreview>,
    tries: u8,
    started: bool,
}

impl BannerTrailer {
    /// Keeps the banner's trailer playing at `size` while the page is `active`, and lets it go
    /// once the page is not, so the next visit plays it again. `banner` finds this banner in
    /// the view that owns it.
    #[allow(clippy::too_many_arguments)]
    pub fn sync<V: 'static>(
        &mut self,
        kind: Kind,
        id: &str,
        size: (f32, f32),
        active: bool,
        reduced: bool,
        services: Option<&Arc<Services>>,
        window: &Window,
        cx: &mut Context<V>,
        banner: fn(&mut V) -> &mut BannerTrailer,
    ) {
        if !active || reduced {
            for trailer in [self.trailer.take(), self.sharper.take()]
                .into_iter()
                .flatten()
            {
                trailer.release(window);
            }
            (self.started, self.tries) = (false, 0);
            return;
        }
        if !self.started {
            self.started = true;
            self.trailer = Handoff::take(id, cx).or_else(|| {
                services.map(|services| {
                    TrailerPreview::start(kind, id, size, services, window, cx, move |view| {
                        banner(view).trailer.as_mut()
                    })
                })
            });
        }
        let Some(trailer) = &mut self.trailer else {
            return;
        };
        trailer.resize(size, window);
        trailer.advance(window);
        let wanted = (size.0 * window.scale_factor()).min(MAX_CLIP_WIDTH as f32) as u32;
        if self.sharper.is_none()
            && self.tries < SHARPEN_TRIES
            && trailer.shown()
            && trailer.width > 0
            && (trailer.width as f32) < wanted as f32 * 0.9
            && let Some(services) = services
        {
            self.sharper =
                TrailerPreview::sharpen(trailer, kind, size, services, window, cx, move |view| {
                    banner(view).sharper.as_mut()
                });
            self.tries += u8::from(self.sharper.is_some());
        }
        self.settle_sharper(size, window);
    }

    /// Plays the sharper stream once the trailer reaches the frame it holds, and lets the
    /// trailer go once it has faded in over it; drops one that misses its moment or finds
    /// nothing sharper.
    fn settle_sharper(&mut self, size: (f32, f32), window: &Window) {
        let (Some(trailer), Some(sharper)) = (&self.trailer, &mut self.sharper) else {
            return;
        };
        sharper.resize(size, window);
        sharper.advance(window);
        let missed = match (sharper.hold, trailer.position()) {
            _ if sharper.gave_up || !trailer.playing() => {
                if sharper.gave_up {
                    self.tries = SHARPEN_TRIES;
                }
                true
            }
            // Past the held frame (where mpv actually stopped, once it says), or looped back
            // to the start of the trailer.
            (Some(hold), Some(at)) if at >= sharper.position().unwrap_or(hold) - 0.02 => {
                if sharper.image.is_some() && sharper.playing() {
                    if let Some(clip) = &sharper.clip {
                        clip.resume();
                    }
                    sharper.hold = None;
                    sharper
                        .fade
                        .retarget(1.0, DURATION_STANDARD, Some(EASE_OUT), Instant::now());
                    false
                } else {
                    true
                }
            }
            (Some(hold), Some(at)) => at + 2.0 * SHARPEN_LEAD < hold,
            (None, _) => {
                if !sharper.fade.moving(Instant::now(), false)
                    && let Some(sharper) = self.sharper.take()
                    && let Some(old) = self.trailer.replace(sharper)
                {
                    old.release(window);
                }
                false
            }
            (Some(_), None) => false,
        };
        if missed && let Some(sharper) = self.sharper.take() {
            sharper.release(window);
        }
    }

    pub fn moving(&self, reduced: bool) -> bool {
        [&self.trailer, &self.sharper]
            .into_iter()
            .flatten()
            .any(|trailer| trailer.moving(reduced))
    }

    pub fn layer(&self, bounds: (f32, f32), shown: f32, reduced: bool) -> Option<Div> {
        let below = self.trailer.as_ref()?.layer(bounds, shown, reduced)?;
        let above = self
            .sharper
            .as_ref()
            .and_then(|sharper| sharper.layer(bounds, shown, reduced));
        Some(div().absolute().inset_0().child(below).children(above))
    }
}

/// Rows of black at both the top and bottom of a BGRA frame (the smaller of the two, so an
/// uneven dark edge in a scene is never cut); `None` for a frame with no picture at all.
fn letterbox(bgra: &[u8], width: u32, height: u32) -> Option<u32> {
    let (width, height) = (width as usize, height as usize);
    if width == 0 || bgra.len() < width * height * 4 {
        return None;
    }
    let dark = |row: usize| {
        bgra[row * width * 4..(row + 1) * width * 4]
            .as_chunks::<4>()
            .0
            .iter()
            .step_by(3)
            .all(|pixel| pixel[0] <= BLACK && pixel[1] <= BLACK && pixel[2] <= BLACK)
    };
    let top = (0..height).take_while(|&row| dark(row)).count();
    if top == height {
        return None;
    }
    let bottom = (0..height).rev().take_while(|&row| dark(row)).count();
    // A bar never takes more than a third of the frame; past that it is a dark scene.
    Some(top.min(bottom).min(height / 3) as u32)
}

/// Where a `video`-sized frame with `bar` rows of letterbox above and below its picture is drawn
/// so the picture covers `bounds`, centred: left, top, width and height.
fn crop(video: (f32, f32), bar: f32, bounds: (f32, f32)) -> (f32, f32, f32, f32) {
    let picture = (video.1 - 2.0 * bar).max(1.0);
    let scale = (bounds.0 / video.0).max(bounds.1 / picture);
    let (width, height) = (video.0 * scale, video.1 * scale);
    let left = (bounds.0 - width) / 2.0;
    let top = (bounds.1 - picture * scale) / 2.0 - bar * scale;
    (left, top, width, height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(width: u32, height: u32, bars: (u32, u32)) -> Vec<u8> {
        let mut bgra = vec![16; (width * height * 4) as usize];
        for row in bars.0..height - bars.1 {
            let start = (row * width * 4) as usize;
            bgra[start..start + (width * 4) as usize].fill(120);
        }
        bgra
    }

    #[test]
    fn clips_are_drawn_16_9_covering_their_place() {
        assert_eq!(cover_16_9((1600.0, 900.0)), (1600.0, 900.0));
        // A 16:10 banner: as tall, wider.
        assert_eq!(cover_16_9((1440.0, 900.0)), (1600.0, 900.0));
        // A wide strip: as wide, taller.
        assert_eq!(cover_16_9((1600.0, 600.0)), (1600.0, 900.0));
    }

    #[test]
    fn letterbox_is_the_smaller_black_edge() {
        assert_eq!(letterbox(&frame(32, 18, (3, 3)), 32, 18), Some(3));
        assert_eq!(letterbox(&frame(32, 18, (4, 2)), 32, 18), Some(2));
        assert_eq!(letterbox(&frame(32, 18, (0, 0)), 32, 18), Some(0));
        assert_eq!(letterbox(&vec![0; 32 * 18 * 4], 32, 18), None);
        assert_eq!(letterbox(&[0; 8], 32, 18), None);
    }

    #[test]
    fn crops_cover_the_card_with_the_picture() {
        // 16:9 video, no bars, into a 16:9 card: drawn exactly over it.
        assert_eq!(
            crop((480.0, 270.0), 0.0, (320.0, 180.0)),
            (0.0, 0.0, 320.0, 180.0)
        );
        // A 2.39:1 picture letterboxed in 16:9 (34 rows above and below): scaled so the
        // picture fills the card's height, with the bars pushed out above and below it.
        let (left, top, width, height) = crop((480.0, 270.0), 34.0, (320.0, 180.0));
        let scale = 180.0 / 202.0;
        let close = |a: f32, b: f32| (a - b).abs() < 0.01;
        assert!(close(width, 480.0 * scale) && close(height, 270.0 * scale));
        assert!(close(left, (320.0 - 480.0 * scale) / 2.0));
        assert!(close(top, -34.0 * scale));
    }
}
