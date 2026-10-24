use super::model::PlaybackModel;

#[derive(Clone, PartialEq)]
pub struct ShellPlayerState {
    pub capture_ready: bool,
    pub fullscreen: bool,
}

impl ShellPlayerState {
    pub fn update(state: &mut Option<Self>, model: &PlaybackModel, ui_settled: bool) -> bool {
        let next = Self {
            capture_ready: ui_settled
                && model.has_reached_playable
                && model.time.is_some_and(|time| time >= 1.0),
            fullscreen: model.fullscreen,
        };
        if state.as_ref() == Some(&next) {
            return false;
        }
        *state = Some(next);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::playback::{model::HostEvent, selection::SourceRef};
    use panorama_mpv::PlayerEvent;

    #[test]
    fn shell_changes_only_for_capture_readiness_and_fullscreen() {
        let mut model = PlaybackModel::new("Film".into(), SourceRef::default());
        let mut state = None;
        model.apply(HostEvent::Loading);
        assert!(ShellPlayerState::update(&mut state, &model, true));
        model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(1.0))));
        assert!(ShellPlayerState::update(&mut state, &model, true));
        for seconds in [1.1, 2.0, 60.0] {
            model.apply(HostEvent::Mpv(PlayerEvent::TimePos(Some(seconds))));
            assert!(!ShellPlayerState::update(&mut state, &model, true));
        }
        model.apply(HostEvent::Volume(50));
        model.apply(HostEvent::Muted(true));
        model.audio.push(crate::playback::model::Track {
            id: 1,
            label: "English".into(),
            description: None,
            lang: None,
            codec: None,
            selected: true,
            external: false,
        });
        assert!(!ShellPlayerState::update(&mut state, &model, true));
        model.fullscreen = true;
        assert!(ShellPlayerState::update(&mut state, &model, true));
        assert!(!ShellPlayerState::update(&mut state, &model, true));
    }
}
