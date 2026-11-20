//! The in-progress composition: raw keys plus partial selections.

/// A candidate the user has already chosen for the front of the composition.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// Raw bytes of the composition this selection replaced.
    pub raw: String,
    pub text: String,
    /// Whether this selection should be learned when committed.
    pub learnable: bool,
    pub confirmed: bool,
    pub input_mode: Option<senti_core::InputMode>,
}

/// Raw input (e.g. `nihaoshijie`) and the selections already made for its
/// leading part (e.g. `你好` for `nihao`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Composition {
    raw: String,
    consumed: usize,
    selections: Vec<Selection>,
}

impl Composition {
    pub fn is_empty(&self) -> bool {
        self.raw.is_empty()
    }

    pub fn raw(&self) -> &str {
        &self.raw
    }

    pub fn len(&self) -> usize {
        self.raw.len()
    }

    /// The part still waiting for conversion.
    pub fn remaining(&self) -> &str {
        &self.raw[self.consumed..]
    }

    pub fn selections(&self) -> &[Selection] {
        &self.selections
    }

    /// Text already selected for the front of the composition.
    pub fn selected_text(&self) -> String {
        self.selections.iter().map(|s| s.text.as_str()).collect()
    }

    /// What the user sees: selected text followed by the unconverted keys.
    pub fn preedit(&self) -> String {
        format!("{}{}", self.selected_text(), self.remaining())
    }

    /// Selected text + raw remainder; what `Enter` commits.
    pub fn literal(&self) -> String {
        self.preedit()
    }

    pub fn push(&mut self, c: char) {
        self.raw.push(c);
    }

    /// Replace the last unconverted key without undoing a partial selection.
    pub fn replace_last(&mut self, c: char) {
        if !self.remaining().is_empty() {
            self.raw.pop();
            self.raw.push(c);
        }
    }

    /// Remove the last raw character. If that empties the unconverted part,
    /// the most recent selection is undone so the user can re-choose it.
    pub fn pop(&mut self) {
        self.raw.pop();
        while self.consumed > 0 && self.raw.len() <= self.consumed {
            self.undo_selection();
        }
    }

    /// Convert the first `consumed` bytes of [`Self::remaining`] to `text`.
    pub fn select(&mut self, consumed: usize, text: String, learnable: bool) {
        self.select_with_mode(consumed, text, learnable, None);
    }

    pub fn select_with_mode(
        &mut self,
        consumed: usize,
        text: String,
        learnable: bool,
        input_mode: Option<senti_core::InputMode>,
    ) {
        let consumed = consumed.clamp(1, self.remaining().len().max(1));
        let end = (self.consumed + consumed).min(self.raw.len());
        self.selections.push(Selection {
            raw: self.raw[self.consumed..end].to_owned(),
            text,
            learnable,
            confirmed: false,
            input_mode,
        });
        self.consumed = end;
    }

    /// Refresh only automatic Chinese prefixes. Explicit selections are anchors.
    pub fn revise_automatic_text(&mut self, start: usize, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        // Look-ahead may revise only the latest word-sized span. Older text
        // stays stable even though it is still available as language context.
        let editable_start = chars.len().saturating_sub(6);
        let mut at = 0;
        let mut changed = false;
        for selection in self.selections.iter_mut().skip(start) {
            if selection.confirmed || selection.input_mode != Some(senti_core::InputMode::Zhuyin) {
                return false;
            }
            let Some(syllables) = senti_core::zhuyin::parse_tolerant(&selection.raw) else {
                return false;
            };
            let end = at + syllables.len();
            if end > chars.len() {
                return false;
            }
            let replacement: String = selection
                .text
                .chars()
                .zip(&chars[at..end])
                .enumerate()
                .map(|(i, (old, new))| if at + i < editable_start { old } else { *new })
                .collect();
            changed |= selection.text != replacement;
            selection.text = replacement;
            at = end;
        }
        changed
    }

    pub fn confirm_last_selection(&mut self) {
        if let Some(selection) = self.selections.last_mut() {
            selection.confirmed = true;
        }
    }

    /// `true` once every raw key has been converted.
    pub fn is_fully_converted(&self) -> bool {
        !self.raw.is_empty() && self.consumed >= self.raw.len()
    }

    fn undo_selection(&mut self) {
        if let Some(last) = self.selections.pop() {
            self.consumed -= last.raw.len();
        }
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn typed(s: &str) -> Composition {
        let mut c = Composition::default();
        s.chars().for_each(|ch| c.push(ch));
        c
    }

    #[test]
    fn lookahead_changes_only_the_latest_six_characters() {
        let raw = "su3".repeat(10);
        let mut c = typed(&raw);
        c.select_with_mode(
            raw.len(),
            "你".repeat(10),
            true,
            Some(senti_core::InputMode::Zhuyin),
        );
        assert!(c.revise_automatic_text(0, &"妳".repeat(10)));
        assert_eq!(
            c.selected_text(),
            format!("{}{}", "你".repeat(4), "妳".repeat(6))
        );
        c.confirm_last_selection();
        assert!(!c.revise_automatic_text(0, &"你".repeat(10)));
        assert_eq!(
            c.selected_text(),
            format!("{}{}", "你".repeat(4), "妳".repeat(6))
        );
    }

    #[test]
    fn partial_selection_flow() {
        let mut c = typed("nihaoshijie");
        c.select(5, "你好".into(), true);
        assert_eq!(c.remaining(), "shijie");
        assert_eq!(c.preedit(), "你好shijie");
        assert!(!c.is_fully_converted());
        c.select(6, "世界".into(), true);
        assert!(c.is_fully_converted());
        assert_eq!(c.selected_text(), "你好世界");
    }

    #[test]
    fn backspace_edits_remaining_then_undoes_selection() {
        let mut c = typed("nihaoma");
        c.select(5, "你好".into(), true);
        c.pop(); // remove "a"
        assert_eq!(c.preedit(), "你好m");
        c.pop(); // remaining would be empty → undo 你好
        assert_eq!(c.preedit(), "nihao");
        assert!(c.selections().is_empty());
    }

    #[test]
    fn select_clamps_consumed() {
        let mut c = typed("ni");
        c.select(99, "你".into(), true);
        assert!(c.is_fully_converted());
        assert_eq!(c.selections()[0].raw, "ni");
    }
}
