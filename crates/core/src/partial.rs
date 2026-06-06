//! Partial-backup selection logic.
//!
//! When preflight reports a shortfall, the user picks a method and we
//! determine which sources to actually back up. All logic here is pure
//! (no I/O): it operates on sources already paired with their measured
//! sizes, so it can be tested exhaustively at the boundaries.

use crate::config::Source;

/// A source paired with its measured size in bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct SizedSource {
    pub source: Source,
    pub size_bytes: u64,
}

/// The result of a selection: which sources to back up, and the bytes that
/// selection totals.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    pub selected: Vec<SizedSource>,
    pub selected_bytes: u64,
}

/// SmallestFirst: include whole sources from smallest to largest, stopping
/// at the first that does not fit in the remaining free space.
///
/// Deterministic: sorts by size ascending with a stable sort, so equal
/// sizes preserve their original config order. Returns the selected subset
/// and its total size. May select nothing if even the smallest source does
/// not fit.
pub fn select_smallest_first(sources: &[SizedSource], free_bytes: u64) -> Selection {
    // Clone so we can sort without disturbing the caller's slice.
    let mut sorted: Vec<SizedSource> = sources.to_vec();
    // Stable sort by size ascending; equal sizes keep config order.
    sorted.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes));

    let mut selected: Vec<SizedSource> = Vec::new();
    let mut selected_bytes: u64 = 0;

    for item in sorted {
        // Stop at the first source that would not fit.
        if selected_bytes.saturating_add(item.size_bytes) > free_bytes {
            break;
        }
        selected_bytes = selected_bytes.saturating_add(item.size_bytes);
        selected.push(item);
    }

    Selection {
        selected,
        selected_bytes,
    }
}

/// The outcome of validating a user's hand-picked (Customization) selection.
#[derive(Debug, Clone, PartialEq)]
pub enum CustomValidation {
    /// The chosen sources fit; carries the total bytes they occupy.
    Fits { selected_bytes: u64 },
    /// The chosen sources do not fit; carries the total and the overflow.
    DoesNotFit {
        selected_bytes: u64,
        over_by_bytes: u64,
    },
}

/// Customization: validate a user-chosen subset against free space.
///
/// Does not modify the selection — it only reports whether it fits, and if
/// not, by how much. The front-end uses this to let the user re-pick.
pub fn validate_custom(chosen: &[SizedSource], free_bytes: u64) -> CustomValidation {
    let total: u64 = chosen
        .iter()
        .fold(0u64, |acc, item| acc.saturating_add(item.size_bytes));

    if total <= free_bytes {
        CustomValidation::Fits {
            selected_bytes: total,
        }
    } else {
        CustomValidation::DoesNotFit {
            selected_bytes: total,
            over_by_bytes: total - free_bytes,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    // Helper: build a SizedSource quickly.
    fn sized(name: &str, size: u64) -> SizedSource {
        SizedSource {
            source: Source {
                name: name.to_string(),
                path: PathBuf::from(format!("/data/{name}")),
            },
            size_bytes: size,
        }
    }

    #[test]
    fn smallest_first_selects_in_size_order_until_full() {
        // sizes 2,6,7 ; free 9 -> greedy smallest: 2 (=2), 6 (=8),
        // 7 would make 15 > 9 -> stop. Selected {2,6} = 8.
        let sources = vec![sized("A", 7), sized("B", 2), sized("C", 6)];
        let result = select_smallest_first(&sources, 9);
        assert_eq!(result.selected_bytes, 8);
        let names: Vec<&str> = result
            .selected
            .iter()
            .map(|s| s.source.name.as_str())
            .collect();
        assert_eq!(names, vec!["B", "C"]); // smallest-first order
    }

    #[test]
    fn smallest_first_all_fit() {
        let sources = vec![sized("A", 1), sized("B", 2), sized("C", 3)];
        let result = select_smallest_first(&sources, 100);
        assert_eq!(result.selected_bytes, 6);
        assert_eq!(result.selected.len(), 3);
    }

    #[test]
    fn smallest_first_none_fit() {
        // Even the smallest (5) exceeds free (3).
        let sources = vec![sized("A", 5), sized("B", 8)];
        let result = select_smallest_first(&sources, 3);
        assert_eq!(result.selected_bytes, 0);
        assert!(result.selected.is_empty());
    }

    #[test]
    fn smallest_first_exact_fit_boundary() {
        // 4 + 6 = 10, free exactly 10 -> both fit.
        let sources = vec![sized("A", 6), sized("B", 4)];
        let result = select_smallest_first(&sources, 10);
        assert_eq!(result.selected_bytes, 10);
        assert_eq!(result.selected.len(), 2);
    }

    #[test]
    fn smallest_first_stable_order_on_ties() {
        // Equal sizes must preserve config order: A then B (both size 5).
        let sources = vec![sized("A", 5), sized("B", 5), sized("C", 5)];
        let result = select_smallest_first(&sources, 10); // fits two
        let names: Vec<&str> = result
            .selected
            .iter()
            .map(|s| s.source.name.as_str())
            .collect();
        assert_eq!(names, vec!["A", "B"]); // config order preserved on ties
    }

    #[test]
    fn custom_fits() {
        let chosen = vec![sized("A", 3), sized("B", 4)];
        let result = validate_custom(&chosen, 10);
        assert_eq!(result, CustomValidation::Fits { selected_bytes: 7 });
    }

    #[test]
    fn custom_fits_exact_boundary() {
        let chosen = vec![sized("A", 5), sized("B", 5)];
        let result = validate_custom(&chosen, 10);
        assert_eq!(result, CustomValidation::Fits { selected_bytes: 10 });
    }

    #[test]
    fn custom_does_not_fit_reports_overflow() {
        let chosen = vec![sized("A", 8), sized("B", 5)];
        let result = validate_custom(&chosen, 10);
        assert_eq!(
            result,
            CustomValidation::DoesNotFit {
                selected_bytes: 13,
                over_by_bytes: 3
            }
        );
    }

    #[test]
    fn custom_empty_selection_fits() {
        let chosen: Vec<SizedSource> = vec![];
        let result = validate_custom(&chosen, 10);
        assert_eq!(result, CustomValidation::Fits { selected_bytes: 0 });
    }
}
