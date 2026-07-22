//! Multi-key sorting of query result rows for the result grid.
//!
//! The UI lets a user sort the result grid by up to a few columns at once
//! (a primary key, then a secondary, then a tertiary). This module owns the
//! *ordering* logic so it stays out of the UI event handlers (architecture
//! rule: no business logic in the presentation layer) and can be tested in
//! isolation. It computes a permutation of row indices rather than moving
//! rows, so the caller's row indices (used for selection and inline editing)
//! keep pointing at the same underlying rows.

use std::cmp::Ordering;

use crate::{Row, Value};

/// One level of a sort: which result column to order by, and the direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SortKey {
    /// Zero-based index into the result's columns.
    pub column: usize,
    /// `true` = ascending, `false` = descending.
    pub ascending: bool,
}

/// Impose a total order over [`Value`] for display sorting.
///
/// SQL leaves cross-type and NULL ordering largely engine-defined, but the
/// grid just needs a *stable, predictable* order — not SQL semantics. So we
/// fix one total order: NULLs first, then numbers (Integer/Real compared by
/// magnitude), then text, then blobs. Within a bucket the natural order
/// applies; across buckets a bucket rank breaks the tie. Unlike a raw `f64`
/// comparison this never panics (it uses `f64::total_cmp`, which also gives
/// `NaN` a defined position).
#[must_use]
pub fn compare_values(a: &Value, b: &Value) -> Ordering {
    fn rank(v: &Value) -> u8 {
        match v {
            Value::Null => 0,
            Value::Integer(_) | Value::Real(_) => 1,
            Value::Text(_) => 2,
            Value::Blob(_) => 3,
        }
    }

    match (a, b) {
        (Value::Null, Value::Null) => Ordering::Equal,
        (Value::Integer(x), Value::Integer(y)) => x.cmp(y),
        (Value::Real(x), Value::Real(y)) => x.total_cmp(y),
        // Mixed number types compare by f64 magnitude so 2 and 2.5 interleave.
        (Value::Integer(x), Value::Real(y)) => cmp_int_real(*x, *y),
        (Value::Real(x), Value::Integer(y)) => cmp_int_real(*y, *x).reverse(),
        (Value::Text(x), Value::Text(y)) => x.cmp(y),
        (Value::Blob(x), Value::Blob(y)) => x.cmp(y),
        // Different buckets: order by bucket rank.
        _ => rank(a).cmp(&rank(b)),
    }
}

/// Order an integer against a real for the mixed-numeric case. A DB column is
/// normally uniformly typed, so interleaving the two storage classes is a
/// display-sort nicety; the `as f64` widening is acceptable here because at
/// worst it mis-orders a pair where a >2^53 integer lands within float
/// rounding distance of the real — a hair-splitting tie that never affects
/// the grid's correctness, only which of two near-equal rows shows first.
#[allow(clippy::cast_precision_loss)]
fn cmp_int_real(n: i64, x: f64) -> Ordering {
    (n as f64).total_cmp(&x)
}

/// Return a stable permutation of `0..rows.len()` ordering the rows by the
/// given keys, primary first.
///
/// An empty `keys` yields the identity order, so a caller can always render
/// through the returned indices. Rows equal on every key keep their original
/// relative order (the sort is stable), which makes the natural row order the
/// implicit final tiebreak — so a "third" sort level is really just the row's
/// original position once two explicit levels are exhausted.
#[must_use]
pub fn sorted_row_order(rows: &[Row], keys: &[SortKey]) -> Vec<usize> {
    let mut order: Vec<usize> = (0..rows.len()).collect();
    if keys.is_empty() {
        return order;
    }

    order.sort_by(|&a, &b| {
        for key in keys {
            let ord = match (rows[a].get(key.column), rows[b].get(key.column)) {
                (Some(x), Some(y)) => compare_values(x, y),
                // A ragged row missing this column sorts before a present
                // value, so short rows stay grouped rather than scattering.
                (None, Some(_)) => Ordering::Less,
                (Some(_), None) => Ordering::Greater,
                (None, None) => Ordering::Equal,
            };
            let ord = if key.ascending { ord } else { ord.reverse() };
            if ord != Ordering::Equal {
                return ord;
            }
        }
        Ordering::Equal
    });
    order
}

#[cfg(test)]
mod tests {
    use super::{compare_values, sorted_row_order, SortKey};
    use crate::{Row, Value};
    use std::cmp::Ordering;

    fn asc(column: usize) -> SortKey {
        SortKey {
            column,
            ascending: true,
        }
    }

    fn desc(column: usize) -> SortKey {
        SortKey {
            column,
            ascending: false,
        }
    }

    #[test]
    fn integers_compare_numerically_not_lexically() {
        // 9 vs 10: a string compare would put "10" before "9".
        assert_eq!(
            compare_values(&Value::Integer(9), &Value::Integer(10)),
            Ordering::Less
        );
    }

    #[test]
    fn integer_and_real_interleave_by_magnitude() {
        assert_eq!(
            compare_values(&Value::Integer(2), &Value::Real(2.5)),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Real(2.5), &Value::Integer(3)),
            Ordering::Less
        );
    }

    #[test]
    fn null_sorts_before_every_other_type() {
        for other in [
            Value::Integer(-1),
            Value::Real(-1.0),
            Value::Text(String::new()),
            Value::Blob(Vec::new()),
        ] {
            assert_eq!(
                compare_values(&Value::Null, &other),
                Ordering::Less,
                "NULL should precede {other:?}"
            );
        }
    }

    #[test]
    fn cross_type_order_is_number_then_text_then_blob() {
        assert_eq!(
            compare_values(&Value::Integer(999), &Value::Text("a".into())),
            Ordering::Less
        );
        assert_eq!(
            compare_values(&Value::Text("z".into()), &Value::Blob(vec![0])),
            Ordering::Less
        );
    }

    #[test]
    fn empty_keys_yields_identity_order() {
        let rows = vec![
            Row::new(vec![Value::Integer(3)]),
            Row::new(vec![Value::Integer(1)]),
            Row::new(vec![Value::Integer(2)]),
        ];
        assert_eq!(sorted_row_order(&rows, &[]), vec![0, 1, 2]);
    }

    #[test]
    fn single_key_ascending_and_descending() {
        let rows = vec![
            Row::new(vec![Value::Integer(3)]),
            Row::new(vec![Value::Integer(1)]),
            Row::new(vec![Value::Integer(2)]),
        ];
        assert_eq!(sorted_row_order(&rows, &[asc(0)]), vec![1, 2, 0]);
        assert_eq!(sorted_row_order(&rows, &[desc(0)]), vec![0, 2, 1]);
    }

    #[test]
    fn secondary_key_breaks_primary_ties() {
        // Column 0 groups; column 1 orders within a group.
        let rows = vec![
            Row::new(vec![Value::Text("b".into()), Value::Integer(1)]),
            Row::new(vec![Value::Text("a".into()), Value::Integer(2)]),
            Row::new(vec![Value::Text("a".into()), Value::Integer(1)]),
        ];
        // Primary a-then-b; within "a", 1 before 2.
        assert_eq!(sorted_row_order(&rows, &[asc(0), asc(1)]), vec![2, 1, 0]);
        // Mixed direction: primary asc, secondary desc → within "a", 2 before 1.
        assert_eq!(sorted_row_order(&rows, &[asc(0), desc(1)]), vec![1, 2, 0]);
    }

    #[test]
    fn tertiary_key_breaks_the_first_two_ties() {
        let rows = vec![
            Row::new(vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(9),
            ]),
            Row::new(vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(5),
            ]),
            Row::new(vec![
                Value::Integer(1),
                Value::Integer(1),
                Value::Integer(7),
            ]),
        ];
        assert_eq!(
            sorted_row_order(&rows, &[asc(0), asc(1), asc(2)]),
            vec![1, 2, 0]
        );
    }

    #[test]
    fn equal_rows_keep_original_order_stable() {
        // Every row equal on the sort key → the permutation is the identity,
        // proving the sort is stable (the natural order is the final tiebreak).
        let rows = vec![
            Row::new(vec![Value::Integer(1)]),
            Row::new(vec![Value::Integer(1)]),
            Row::new(vec![Value::Integer(1)]),
        ];
        assert_eq!(sorted_row_order(&rows, &[asc(0)]), vec![0, 1, 2]);
    }

    #[test]
    fn nulls_group_ahead_when_ascending() {
        let rows = vec![
            Row::new(vec![Value::Integer(2)]),
            Row::new(vec![Value::Null]),
            Row::new(vec![Value::Integer(1)]),
        ];
        assert_eq!(sorted_row_order(&rows, &[asc(0)]), vec![1, 2, 0]);
    }
}
