use crate::{theme::Theme, transition::bezier};
use gpui::{Div, div, prelude::*, px, svg};

fn offset(elapsed: f32, index: usize, reduced: bool) -> f32 {
    if reduced || elapsed < index as f32 * 0.1 {
        return 0.0;
    }
    let phase = ((elapsed - index as f32 * 0.1) % 1.6) / 1.6;
    let t = if phase < 0.12 {
        return 0.0;
    } else if phase < 0.38 {
        (phase - 0.12) / 0.26
    } else if phase < 0.62 {
        return 20.0;
    } else if phase < 0.88 {
        return 20.0 * (1.0 - bezier((phase - 0.62) / 0.26, [0.22, 1.0, 0.36, 1.0]));
    } else {
        return 0.0;
    };
    20.0 * bezier(t, [0.22, 1.0, 0.36, 1.0])
}

pub fn render(elapsed: f32, reduced: bool, theme: Theme) -> Div {
    let mut loader = div().relative().w(px(192.0)).h(px(86.08)).overflow_hidden();
    for (index, (left, right)) in [(0.0, 69.26), (38.42, 38.52), (69.26, 15.98), (92.21, 0.0)]
        .into_iter()
        .enumerate()
    {
        let x = 160.0 * left / 100.0;
        loader = loader.child(
            div()
                .absolute()
                .top_0()
                .left(px(12.0 + x + offset(elapsed, index, reduced)))
                .w(px(160.0 * (100.0 - left - right) / 100.0))
                .h(px(86.08))
                .overflow_hidden()
                .child(
                    svg()
                        .path("icon.svg")
                        .absolute()
                        .left(px(-x))
                        .top_0()
                        .w(px(160.0))
                        .h(px(160.0 * 197.0 / 366.0))
                        .text_color(theme.player_ink),
                ),
        );
    }
    loader
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_css_keyframes_delays_and_reduced_motion() {
        for index in 0..4 {
            let delay = index as f32 * 0.1;
            assert!(offset(delay + 1.6 * 0.12, index, false).abs() < 0.001);
            assert!((offset(delay + 1.6 * 0.38, index, false) - 20.0).abs() < 0.001);
            assert!((offset(delay + 1.6 * 0.62, index, false) - 20.0).abs() < 0.001);
            assert!(offset(delay + 1.6 * 0.88, index, false).abs() < 0.001);
            assert_eq!(offset(0.6, index, true), 0.0);
        }
    }
}
