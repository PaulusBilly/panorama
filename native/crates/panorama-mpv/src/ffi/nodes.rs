use crate::events::Node;
use std::ffi::{CStr, c_char, c_int};

#[repr(C)]
pub(super) union Value {
    pub string: *const c_char,
    pub flag: c_int,
    pub integer: i64,
    pub double: f64,
    pub list: *const List,
}

#[repr(C)]
pub(super) struct RawNode {
    pub value: Value,
    pub format: c_int,
}

#[repr(C)]
pub(super) struct List {
    pub count: c_int,
    pub values: *const RawNode,
    pub keys: *const *const c_char,
}

pub(super) unsafe fn string(pointer: *const c_char) -> Option<String> {
    if pointer.is_null() {
        return None;
    }
    // SAFETY: Caller guarantees this is a live NUL-terminated mpv string.
    Some(
        unsafe { CStr::from_ptr(pointer) }
            .to_string_lossy()
            .into_owned(),
    )
}

pub(super) unsafe fn copy(node: &RawNode, depth: usize) -> Node {
    if depth > 16 {
        return Node::None;
    }
    // SAFETY: Caller guarantees a live node tree from mpv; each union arm is gated
    // by its C format tag, and all values are copied before the next wait_event.
    unsafe {
        match node.format {
            1 => string(node.value.string)
                .map(Node::String)
                .unwrap_or(Node::None),
            3 => Node::Flag(node.value.flag != 0),
            4 => Node::Integer(node.value.integer),
            5 => Node::Double(node.value.double),
            7 | 8 => {
                let Some(list) = node.value.list.as_ref() else {
                    return Node::None;
                };
                if list.count < 0 || list.count > 100_000 {
                    return Node::None;
                }
                if list.count == 0 {
                    return if node.format == 7 {
                        Node::Array(Vec::new())
                    } else {
                        Node::Map(Vec::new())
                    };
                }
                if list.values.is_null() || (node.format == 8 && list.keys.is_null()) {
                    return Node::None;
                }
                let entries = std::slice::from_raw_parts(list.values, list.count as usize);
                if node.format == 7 {
                    Node::Array(entries.iter().map(|n| copy(n, depth + 1)).collect())
                } else {
                    let keys = std::slice::from_raw_parts(list.keys, list.count as usize);
                    Node::Map(
                        keys.iter()
                            .zip(entries)
                            .filter_map(|(&key, value)| {
                                Some((string(key)?, copy(value, depth + 1)))
                            })
                            .collect(),
                    )
                }
            }
            _ => Node::None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_node_tree_is_copied_before_its_storage_is_released() {
        let copied = {
            let fields = [
                RawNode {
                    value: Value { integer: 3 },
                    format: 4,
                },
                RawNode {
                    value: Value {
                        string: c"video".as_ptr(),
                    },
                    format: 1,
                },
                RawNode {
                    value: Value { flag: 1 },
                    format: 3,
                },
            ];
            let keys = [c"id".as_ptr(), c"type".as_ptr(), c"selected".as_ptr()];
            let map = List {
                count: 3,
                values: fields.as_ptr(),
                keys: keys.as_ptr(),
            };
            let track = RawNode {
                value: Value { list: &map },
                format: 8,
            };
            let list = List {
                count: 1,
                values: &track,
                keys: std::ptr::null(),
            };
            let root = RawNode {
                value: Value { list: &list },
                format: 7,
            };
            // SAFETY: Every pointer refers to the live local arrays/lists above;
            // their tags match the initialized union fields and strings are static.
            unsafe { copy(&root, 0) }
        };
        let tracks = crate::events::tracks(&copied);
        assert_eq!(tracks.len(), 1);
        assert_eq!(tracks[0].id, 3);
        assert_eq!(tracks[0].kind, crate::TrackKind::Video);
        assert!(tracks[0].selected);
    }

    #[test]
    fn empty_lists_do_not_require_nonnull_storage() {
        let list = List {
            count: 0,
            values: std::ptr::null(),
            keys: std::ptr::null(),
        };
        let root = RawNode {
            value: Value { list: &list },
            format: 8,
        };
        // SAFETY: Live list with zero elements; no storage pointer may be read.
        assert_eq!(unsafe { copy(&root, 0) }, Node::Map(Vec::new()));
    }
}
