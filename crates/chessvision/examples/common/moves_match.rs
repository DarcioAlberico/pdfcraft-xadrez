//! Moves matched across two runs of the chess reader (plan A9, Q2): the one rule `cargo xtask
//! chess-bench` and the `walk_bench` and `reference_bench` examples judge a run by against a
//! baseline, so a move lost counts the same in all three. Only the standard library: the
//! examples and `xtask` include this file by `#[path]` (no product crate depends on `xtask`).

use std::collections::BTreeMap;

/// A move's identity across runs: its page (0-based), its place (the left and bottom of its box,
/// rounded to a point) and its text.
pub type Key = (u64, i64, i64, String);

/// How far, in points, a move's place may move between runs and still be the same move.
pub const SAME_PLACE: i64 = 4;

/// The moves `good` in `now` and not in `then`: gained when `now` is the newer run, lost when it is
/// the older one.
///
/// A move is the same move in both runs when its page and text are and its place differs by at
/// most [`SAME_PLACE`] points either way: a change to the reader can move a move's box a little
/// (OCR measures its glyphs against another text scale), and that is not a move lost. A move
/// matched nowhere near counts only when its page has more `good` moves with its text in `now`
/// than in `then`: its box was wrong in one run (a line inflated by a frame read as a glyph) and
/// right in the other, and the move is the same.
pub fn newly<'a>(now: &'a BTreeMap<Key, String>, then: &BTreeMap<Key, String>, good: impl Fn(&str) -> bool) -> Vec<&'a Key> {
    let good_on_page = |map: &BTreeMap<Key, String>, k: &Key| page_of(map, k.0).filter(|(o, st)| good(st) && o.3 == k.3).count();
    now.iter()
        .filter(|(k, s)| {
            good(s)
                && !page_of(then, k.0).any(|(o, st)| {
                    good(st) && o.3 == k.3 && o.1.abs_diff(k.1) <= SAME_PLACE.unsigned_abs() && o.2.abs_diff(k.2) <= SAME_PLACE.unsigned_abs()
                })
                && good_on_page(now, k) > good_on_page(then, k)
        })
        .map(|(k, _)| k)
        .collect()
}

/// The moves of `map` on `page`.
fn page_of(map: &BTreeMap<Key, String>, page: u64) -> impl Iterator<Item = (&Key, &String)> + '_ {
    map.range((page, i64::MIN, i64::MIN, String::new())..).take_while(move |(o, _)| o.0 == page)
}

/// A run against its baseline: the moves gained and the moves lost (`good` in the baseline and not
/// now), by [`newly`].
pub fn compare<'a>(now: &'a BTreeMap<Key, String>, then: &'a BTreeMap<Key, String>, good: impl Fn(&str) -> bool + Copy) -> (usize, Vec<&'a Key>) {
    (newly(now, then, good).len(), newly(then, now, good))
}
