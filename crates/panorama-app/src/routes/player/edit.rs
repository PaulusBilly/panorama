//! The value editor of a stepper (`PlayerStepperControl`) and the custom colour input.
use crate::playback::subtitle_prefs::Field;

/// What is being edited.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Target {
    /// A stepper's value.
    Field(Field),
    /// The custom text colour (`#rrggbb`).
    Hex,
}

/// A draft of the field's text. A fresh draft is fully selected, so the first character replaces it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Edit {
    /// What the draft will become.
    pub target: Target,
    /// The typed text.
    pub draft: String,
    /// Everything is selected (the input selects its text on focus).
    pub fresh: bool,
}

/// What a key did to the editor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Outcome {
    /// The draft changed.
    Changed,
    /// Enter: commit.
    Commit,
    /// Escape: cancel.
    Cancel,
    /// Not an editing key.
    Ignored,
}

/// `String(number)` for a stepper value.
pub fn number_text(value: f64) -> String {
    value.to_string()
}

/// Parse a draft and clamp it: `None` for empty or non-numeric text, which cancels.
pub fn parse_commit(draft: &str, min: f64, max: f64) -> Option<f64> {
    let draft = draft.trim();
    if draft.is_empty() {
        return None;
    }
    let value: f64 = draft.parse().ok()?;
    value.is_finite().then(|| value.clamp(min, max))
}

impl Edit {
    /// Start editing a stepper's current value.
    pub fn field(field: Field, value: f64) -> Self {
        Self {
            target: Target::Field(field),
            draft: number_text(value),
            fresh: true,
        }
    }

    /// Start editing the custom colour.
    pub fn hex(current: &str) -> Self {
        Self {
            target: Target::Hex,
            draft: current.to_owned(),
            fresh: true,
        }
    }

    fn accepts(&self, c: char) -> bool {
        match self.target {
            Target::Field(_) => c.is_ascii_digit() || matches!(c, '.' | '-'),
            Target::Hex => c.is_ascii_hexdigit() || c == '#',
        }
    }

    /// Type text: it replaces a fresh selection, and characters the input does not take are dropped.
    pub fn insert(&mut self, text: &str) -> bool {
        let typed: String = text.chars().filter(|c| self.accepts(*c)).collect();
        if typed.is_empty() {
            return false;
        }
        if self.fresh {
            self.draft.clear();
            self.fresh = false;
        }
        let limit = match self.target {
            Target::Field(_) => 8,
            Target::Hex => 9,
        };
        for c in typed.chars() {
            if self.draft.chars().count() < limit {
                self.draft.push(c);
            }
        }
        true
    }

    /// Backspace deletes the selection, or the last character.
    pub fn backspace(&mut self) {
        if self.fresh {
            self.draft.clear();
            self.fresh = false;
        } else {
            self.draft.pop();
        }
    }

    /// Route one key.
    pub fn key(&mut self, key: &str, key_char: Option<&str>) -> Outcome {
        match key {
            "enter" => Outcome::Commit,
            "escape" => Outcome::Cancel,
            "backspace" | "delete" => {
                self.backspace();
                Outcome::Changed
            }
            _ => match key_char {
                Some(text) if self.insert(text) => Outcome::Changed,
                _ => Outcome::Ignored,
            },
        }
    }

    /// The committed value of a stepper draft, clamped to its limits.
    pub fn committed(&self) -> Option<f64> {
        match self.target {
            Target::Field(field) => {
                let (min, max) = field.range();
                parse_commit(&self.draft, min, max)
            }
            Target::Hex => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_into(edit: &mut Edit, text: &str) {
        for c in text.chars() {
            edit.key(&c.to_string(), Some(&c.to_string()));
        }
    }

    #[test]
    fn a_draft_starts_as_the_value_with_everything_selected() {
        let mut edit = Edit::field(Field::FontSize, 38.0);
        assert_eq!(edit.draft, "38");
        assert!(edit.fresh);
        type_into(&mut edit, "52");
        assert_eq!(edit.draft, "52");
        assert!(!edit.fresh);
        assert_eq!(edit.committed(), Some(52.0));
        assert_eq!(number_text(1.24), "1.24");
        assert_eq!(number_text(-1.5), "-1.5");
    }

    #[test]
    fn commit_clamps_to_the_limits() {
        for (draft, field, expected) in [
            ("500", Field::FontSize, Some(96.0)),
            ("3", Field::FontSize, Some(12.0)),
            ("-4", Field::PaddingX, Some(0.0)),
            ("1.6", Field::LineHeight, Some(1.6)),
            ("9", Field::LineHeight, Some(2.0)),
            ("101", Field::BoxOpacity, Some(100.0)),
            ("", Field::FontSize, None),
            ("   ", Field::FontSize, None),
            ("-", Field::FontSize, None),
            ("1.2.3", Field::FontSize, None),
            ("12.5", Field::PaddingX, Some(12.5)),
        ] {
            let edit = Edit {
                target: Target::Field(field),
                draft: draft.into(),
                fresh: false,
            };
            assert_eq!(edit.committed(), expected, "{draft:?}");
        }
        assert_eq!(parse_commit("1e3", 0.0, 10.0), Some(10.0));
        assert_eq!(parse_commit("NaN", 0.0, 10.0), None);
        assert_eq!(parse_commit("inf", 0.0, 10.0), None);
    }

    #[test]
    fn enter_commits_and_escape_cancels() {
        let mut edit = Edit::field(Field::PaddingY, 4.0);
        assert_eq!(edit.key("enter", None), Outcome::Commit);
        assert_eq!(edit.key("escape", None), Outcome::Cancel);
        assert_eq!(
            edit.draft, "4",
            "keys other than editing keys leave the draft"
        );
        assert_eq!(edit.key("tab", None), Outcome::Ignored);
        assert_eq!(
            edit.key("a", Some("a")),
            Outcome::Ignored,
            "letters are not numeric"
        );
    }

    #[test]
    fn backspace_clears_a_selection_then_trims() {
        let mut edit = Edit::field(Field::FontSize, 38.0);
        edit.backspace();
        assert_eq!(edit.draft, "");
        type_into(&mut edit, "123");
        edit.backspace();
        assert_eq!(edit.draft, "12");
        assert_eq!(edit.key("backspace", None), Outcome::Changed);
        assert_eq!(edit.draft, "1");
    }

    #[test]
    fn the_hex_input_takes_only_hex_digits_and_a_hash() {
        let mut edit = Edit::hex("#ffe066");
        type_into(&mut edit, "#12zz34");
        assert_eq!(edit.draft, "#1234");
        type_into(&mut edit, "5678ab");
        assert_eq!(edit.draft.chars().count(), 9, "capped");
        assert_eq!(
            edit.committed(),
            None,
            "colours commit through the hex parser"
        );
    }

    #[test]
    fn long_drafts_are_capped() {
        let mut edit = Edit::field(Field::FontSize, 1.0);
        type_into(&mut edit, "1234567890123");
        assert_eq!(edit.draft.chars().count(), 8);
    }
}
