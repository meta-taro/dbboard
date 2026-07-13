//! Result-grid row selection (ADR-0035 slice 2). A pure state machine for
//! click / Ctrl+click / Shift+click, kept egui-free so the selection rules
//! are unit-tested without a UI. Indices are positions into
//! [`dbboard_core::Row`] slices held by `QueryResult::rows`.

use std::collections::BTreeSet;

/// The rows the user has selected in the result grid, plus the anchor a
/// Shift+click ranges from. `BTreeSet` keeps the selection in ascending
/// index order, so an export of the subset preserves the grid's
/// top-to-bottom order for free.
#[derive(Debug, Default, Clone)]
pub struct ResultSelection {
    selected: BTreeSet<usize>,
    /// The last row a plain or Ctrl click touched. A Shift+click selects
    /// the inclusive range between it and the clicked row; it stays put
    /// across successive Shift+clicks so the range can be re-dragged from
    /// the same origin.
    anchor: Option<usize>,
}

/// The modifier keys the selection cares about, captured at click time.
/// A deliberately small mirror of egui's `Modifiers` so the state machine
/// below never sees an egui type. `ctrl` is the platform "command"
/// modifier (Ctrl on Windows/Linux, ⌘ on macOS).
#[derive(Debug, Default, Clone, Copy)]
pub struct ClickModifiers {
    pub ctrl: bool,
    pub shift: bool,
}

impl ResultSelection {
    /// Whether row `idx` is currently selected (drives the row highlight).
    #[must_use]
    pub fn is_selected(&self, idx: usize) -> bool {
        self.selected.contains(&idx)
    }

    /// Whether nothing is selected — gates the selected-row export actions.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.selected.is_empty()
    }

    /// How many rows are selected (for the "N rows selected" label).
    #[must_use]
    pub fn len(&self) -> usize {
        self.selected.len()
    }

    /// Selected indices in ascending order.
    pub fn iter(&self) -> impl Iterator<Item = usize> + '_ {
        self.selected.iter().copied()
    }

    /// Drop the whole selection (and its anchor). Used by the explicit
    /// "clear selection" button and when a new query result replaces the
    /// grid — the old indices no longer mean anything.
    pub fn clear(&mut self) {
        self.selected.clear();
        self.anchor = None;
    }

    /// Apply a click on row `idx` under `mods`, mutating the selection the
    /// way a file list or spreadsheet does:
    ///
    /// - **plain**: select only `idx`; anchor there.
    /// - **Ctrl**: toggle `idx` in/out of the selection; anchor there.
    /// - **Shift**: select the inclusive range from the anchor (or `idx`
    ///   itself when there is no anchor yet) to `idx`. Plain Shift
    ///   replaces the selection with that range; Ctrl+Shift adds the range
    ///   to what is already selected. The anchor is left untouched so the
    ///   next Shift+click re-ranges from the same origin.
    pub fn click(&mut self, idx: usize, mods: ClickModifiers) {
        if mods.shift {
            let anchor = self.anchor.unwrap_or(idx);
            let (lo, hi) = (anchor.min(idx), anchor.max(idx));
            if !mods.ctrl {
                self.selected.clear();
            }
            self.selected.extend(lo..=hi);
            return;
        }
        if mods.ctrl {
            if !self.selected.remove(&idx) {
                self.selected.insert(idx);
            }
        } else {
            self.selected.clear();
            self.selected.insert(idx);
        }
        self.anchor = Some(idx);
    }
}

#[cfg(test)]
mod tests {
    use super::{ClickModifiers, ResultSelection};

    fn plain() -> ClickModifiers {
        ClickModifiers::default()
    }
    fn ctrl() -> ClickModifiers {
        ClickModifiers {
            ctrl: true,
            shift: false,
        }
    }
    fn shift() -> ClickModifiers {
        ClickModifiers {
            ctrl: false,
            shift: true,
        }
    }
    fn ctrl_shift() -> ClickModifiers {
        ClickModifiers {
            ctrl: true,
            shift: true,
        }
    }

    fn selected(s: &ResultSelection) -> Vec<usize> {
        s.iter().collect()
    }

    #[test]
    fn plain_click_selects_exactly_one_row() {
        let mut s = ResultSelection::default();
        s.click(3, plain());
        assert_eq!(selected(&s), vec![3]);
        // A second plain click replaces, not adds.
        s.click(5, plain());
        assert_eq!(selected(&s), vec![5]);
    }

    #[test]
    fn ctrl_click_toggles_rows_independently() {
        let mut s = ResultSelection::default();
        s.click(1, ctrl());
        s.click(4, ctrl());
        assert_eq!(selected(&s), vec![1, 4]);
        // Ctrl-clicking a selected row removes just that row.
        s.click(1, ctrl());
        assert_eq!(selected(&s), vec![4]);
    }

    #[test]
    fn shift_click_selects_the_inclusive_range_from_the_anchor() {
        let mut s = ResultSelection::default();
        s.click(2, plain());
        s.click(5, shift());
        assert_eq!(selected(&s), vec![2, 3, 4, 5]);
    }

    #[test]
    fn shift_range_works_when_the_click_is_above_the_anchor() {
        let mut s = ResultSelection::default();
        s.click(5, plain());
        s.click(2, shift());
        assert_eq!(selected(&s), vec![2, 3, 4, 5]);
    }

    #[test]
    fn plain_shift_replaces_the_previous_range_re_dragging_from_the_anchor() {
        let mut s = ResultSelection::default();
        s.click(2, plain());
        s.click(6, shift());
        // Re-drag shorter: anchor stays at 2, so the range shrinks.
        s.click(4, shift());
        assert_eq!(selected(&s), vec![2, 3, 4]);
    }

    #[test]
    fn ctrl_shift_adds_the_range_to_the_existing_selection() {
        let mut s = ResultSelection::default();
        s.click(0, ctrl());
        s.click(2, plain()); // anchor -> 2, selection -> {2}
        s.click(4, ctrl_shift()); // add 2..=4 without clearing
        assert_eq!(selected(&s), vec![2, 3, 4]);
    }

    #[test]
    fn shift_without_a_prior_anchor_selects_just_the_clicked_row() {
        let mut s = ResultSelection::default();
        s.click(7, shift());
        assert_eq!(selected(&s), vec![7]);
    }

    #[test]
    fn clear_empties_the_selection_and_the_anchor() {
        let mut s = ResultSelection::default();
        s.click(1, plain());
        s.click(3, shift());
        assert!(!s.is_empty());
        s.clear();
        assert!(s.is_empty());
        assert_eq!(s.len(), 0);
        // With the anchor gone, a fresh Shift+click only takes its own row.
        s.click(9, shift());
        assert_eq!(selected(&s), vec![9]);
    }
}
