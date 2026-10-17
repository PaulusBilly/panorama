use super::{Client, Handle, nodes};
use crate::{
    EndReason, MpvError, PlayerEvent,
    events::{Node, Properties},
};
use std::ffi::{c_char, c_int, c_void};

pub(crate) const OBSERVED: &[&str] = &[
    "time-pos",
    "duration",
    "pause",
    "paused-for-cache",
    "cache-buffering-state",
    "demuxer-cache-state",
    "demuxer-cache-duration",
    "demuxer-cache-time",
    "cache-speed",
    "speed",
    "hwdec-current",
    "track-list",
    "audio-device-list",
    "sub-text",
    "sub-start/full",
    "sub-end/full",
];

#[repr(C)]
pub(super) struct Event {
    id: c_int,
    error: c_int,
    userdata: u64,
    data: *const c_void,
}

#[repr(C)]
struct Property {
    name: *const c_char,
    format: c_int,
    data: *const c_void,
}

#[repr(C)]
struct EndFile {
    reason: c_int,
    error: c_int,
    playlist_entry_id: i64,
    playlist_insert_id: i64,
    playlist_insert_num_entries: c_int,
}

pub(crate) struct Received {
    pub events: Vec<PlayerEvent>,
    pub reply: bool,
    pub shutdown: bool,
}

impl Client {
    pub(crate) fn receive(&self, properties: &mut Properties) -> Received {
        let mut result = Received {
            events: Vec::new(),
            reply: false,
            shutdown: false,
        };
        // SAFETY: Only the dedicated event thread waits on this live handle. Event
        // and nested payloads remain valid until its next wait and are copied here.
        unsafe {
            let Some(event) = (self.api.wait)(self.handle as Handle, 0.05).as_ref() else {
                return result;
            };
            result.reply = matches!(event.id, 4 | 5);
            result.shutdown = event.id == 1;
            if event.error < 0 {
                result
                    .events
                    .push(PlayerEvent::Error(MpvError::from_code(event.error)));
            }
            if event.id == 24 {
                result.events.push(PlayerEvent::Error(MpvError::Runtime));
            }
            if event.id == 7
                && let Some(end) = event.data.cast::<EndFile>().as_ref()
            {
                result.events.push(PlayerEvent::EndFile {
                    reason: end_reason(end.reason, end.error),
                    playlist_entry_id: end.playlist_entry_id,
                });
            }
            if event.id == 22
                && let Some(property) = event.data.cast::<Property>().as_ref()
            {
                let Some(name) = nodes::string(property.name) else {
                    return result;
                };
                let value = if property.format == 6 {
                    property
                        .data
                        .cast::<nodes::RawNode>()
                        .as_ref()
                        .map(|n| nodes::copy(n, 0))
                        .unwrap_or(Node::None)
                } else {
                    Node::None
                };
                result.events.extend(properties.event(&name, value));
            }
        }
        result
    }
}

fn end_reason(reason: i32, error: i32) -> EndReason {
    match reason {
        0 => EndReason::Eof,
        2 => EndReason::Stop,
        3 => EndReason::Quit,
        4 => EndReason::Error(MpvError::from_code(error)),
        5 => EndReason::Redirect,
        _ => EndReason::Other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn end_reasons_preserve_sanitized_kinds() {
        assert_eq!(end_reason(4, -13), EndReason::Error(MpvError::Loading));
        assert_eq!(end_reason(0, 0), EndReason::Eof);
        assert_eq!(end_reason(2, 0), EndReason::Stop);
        assert_eq!(end_reason(1234, -1000), EndReason::Other);
    }
}
