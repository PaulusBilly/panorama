//! The lift card's trailer. Once a preview has finished opening, its film's trailer is looked up
//! and played muted and looping inside the artwork, fading in over it at the first frame. The
//! artwork stays underneath, so a film without a trailer, or one YouTube refuses, keeps the card
//! exactly as it was.
use crate::{
    services::{Job, Services},
    theme::motion::{DURATION_DELIBERATE, EASE_OUT},
    transition::Tween,
};
use gpui::{Context, Div, RenderImage, Task, Window, div, img, prelude::*, px};
use panorama_core::addons::FilmDetails;
use panorama_mpv::{Clip, ClipState};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

/// How long a trailer plays before the card goes back to its artwork.
const PREVIEW_FOR: Duration = Duration::from_secs(30);
/// Frames with picture in them that decide how much letterbox to crop.
const BAR_SAMPLES: u8 = 8;
/// A channel this dark or darker counts as letterbox black (video black sits near 16).
const BLACK: u8 = 28;

/// The trailer for one opened preview.
pub struct TrailerPreview {
    /// The grid index of the card it belongs to.
    pub index: usize,
    film: String,
    lookup: Option<Task<()>>,
    clip: Option<Clip>,
    image: Option<Arc<RenderImage>>,
    fade: Tween,
    /// Rows of letterbox baked into the top and bottom of the video, smallest seen so far, and
    /// how many frames with picture have been measured. Shown once `BAR_SAMPLES` agree, so the
    /// crop never jumps.
    bars: (u32, u8),
    /// When the trailer gives the card back to its artwork; set as it fades in, cleared once
    /// it has ended, so an open card plays it only once.
    ends: Option<Instant>,
}

impl TrailerPreview {
    /// Looks up `film`'s trailer for a card whose open artwork is `size` points, and plays it
    /// once found. `found` hands the result back to whichever view owns this preview.
    pub fn start<V: 'static>(
        index: usize,
        film: &FilmDetails,
        size: (f32, f32),
        services: &Arc<Services>,
        window: &Window,
        cx: &mut Context<V>,
        found: fn(&mut V) -> Option<&mut TrailerPreview>,
    ) -> Self {
        let scale = window.scale_factor();
        let pixels = (
            (size.0 * scale).round() as u32,
            (size.1 * scale).round() as u32,
        );
        let mut job: Job<_> = services.trailer(film.kind, film.id.clone(), pixels.1);
        let id = film.id.clone();
        let lookup = cx.spawn(async move |view, cx| {
            let Some((playlist, library)) = (&mut job.0).await.ok().flatten() else {
                return;
            };
            let _ = view.update(cx, |view, cx| {
                if let Some(preview) = found(view)
                    && preview.index == index
                    && preview.film == id
                    && preview.clip.is_none()
                {
                    preview.clip = library.clip(playlist.as_str(), pixels.0, pixels.1).ok();
                    cx.notify();
                }
            });
        });
        Self {
            index,
            film: film.id.clone(),
            lookup: Some(lookup),
            clip: None,
            image: None,
            fade: Tween::fixed(0.0),
            bars: (u32::MAX, 0),
            ends: None,
        }
    }

    pub fn is_for(&self, index: usize, film: &FilmDetails) -> bool {
        self.index == index && self.film == film.id
    }

    /// Takes the clip's newest frame, if any. The frame it replaces leaves the window's atlas
    /// after the next frame is drawn, once nothing paints it any more.
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
            self.bars = (self.bars.0.min(bar), self.bars.1 + 1);
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

    /// Stops playback, keeping the last frame so the card can settle with it.
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

    /// Frames are needed while the clip plays or its first frame fades in.
    pub fn moving(&self, reduced: bool) -> bool {
        self.clip
            .as_ref()
            .is_some_and(|clip| clip.state() != ClipState::Failed)
            || self.fade.moving(Instant::now(), reduced)
    }

    /// The trailer over the artwork, `bounds` points and covering it; nothing before a frame.
    /// It fades with the card's `lift`, so a settling card takes its trailer down with it.
    pub fn layer(&self, bounds: (f32, f32), lift: f32, reduced: bool) -> Option<Div> {
        if self.bars.1 < BAR_SAMPLES {
            return None;
        }
        let image = self.image.clone()?;
        let size = image.size(0);
        let (left, top, width, height) = crop(
            (size.width.0 as f32, size.height.0 as f32),
            self.bars.0 as f32,
            bounds,
        );
        let shown = self.fade.value(Instant::now(), reduced) * lift;
        Some(
            div()
                .absolute()
                .inset_0()
                .overflow_hidden()
                .opacity(shown)
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
