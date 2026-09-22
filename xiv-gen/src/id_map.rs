//! A sorted-slice map keyed by one of the `#[define_id]` row-id newtypes.
//!
//! `Data`'s tables used to be `HashMap`s, which cost the browser twice.
//!
//! * **Size.** rkyv writes an `ArchivedHashMap`'s entries in hash-bucket
//!   order, so consecutive rows of a sheet land far apart in the archive.
//!   The sheets are overwhelmingly runs of near-identical rows — an item's
//!   name, description and stat block look a lot like its neighbours' — and
//!   scattering them puts those runs out of zlib's window. Keeping the rows
//!   in id order instead cut the shipped English pack's `items` table from
//!   2.89 MB to 1.79 MB compressed, and `recipes` from 330 KB to 173 KB, for
//!   no change in content.
//! * **Code.** `ArchivedHashMap`'s validation and deserialization are
//!   monomorphized per key/value pair — 37 instantiations for `Data`, all of
//!   them shipped in the wasm bundle. A slice of pairs reuses `Vec`'s.
//!
//! Lookup is a binary search rather than a hash, which for tables this size is
//! a handful of comparisons over a cache-friendly run. Nothing in the app does
//! enough per-row lookups for that to be the bottleneck; the common heavy
//! paths iterate whole tables, which is strictly faster here.
//!
//! The map is always sorted by [`RowId::row_id`] and holds at most one entry
//! per key, so iteration order is the sheet's own row order — stable between
//! the server's render and the browser's hydration without a sort at the call
//! site.

use rkyv::{Archive, Deserialize as RkyvDeserialize, Serialize as RkyvSerialize};
use serde::{Deserialize, Serialize};

/// The `i32` a row-id newtype wraps. Implemented by every `define_id!` type.
pub trait RowId: Copy {
    fn row_id(&self) -> i32;
}

/// A map from a row id to a row, stored as a sorted `Vec` of pairs.
///
/// The API is the subset of `HashMap`'s that the game data needs, with the
/// same signatures, so call sites read the same.
#[derive(
    Debug, Clone, PartialEq, Serialize, Deserialize, Archive, RkyvDeserialize, RkyvSerialize,
)]
#[archive(check_bytes)]
pub struct IdMap<K, V> {
    /// Sorted by `K::row_id()`, one entry per key.
    entries: Vec<(K, V)>,
}

// Hand-written rather than derived: `#[derive(Default)]` would bound `K` and
// `V` on `Default`, which the row types have no reason to implement.
impl<K, V> Default for IdMap<K, V> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
        }
    }
}

impl<K: RowId, V> IdMap<K, V> {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Builds from pairs that are **already** sorted by row id and unique.
    ///
    /// Debug builds assert that; release builds trust it, because the pack
    /// generator is the only caller that can get it wrong and its output is
    /// checked by `game-data-pack`'s tests.
    pub fn from_sorted(entries: Vec<(K, V)>) -> Self {
        debug_assert!(
            entries
                .windows(2)
                .all(|w| w[0].0.row_id() < w[1].0.row_id()),
            "IdMap::from_sorted got unsorted or duplicate keys"
        );
        Self { entries }
    }

    fn index_of(&self, key: &K) -> Result<usize, usize> {
        self.entries
            .binary_search_by_key(&key.row_id(), |(k, _)| k.row_id())
    }

    pub fn get(&self, key: &K) -> Option<&V> {
        self.index_of(key).ok().map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        match self.index_of(key) {
            Ok(i) => Some(&mut self.entries[i].1),
            Err(_) => None,
        }
    }

    pub fn contains_key(&self, key: &K) -> bool {
        self.index_of(key).is_ok()
    }

    /// Inserts `value`, returning the row it replaced. O(n) on a new key —
    /// this is here for the pack generator's fixups, not for hot paths.
    pub fn insert(&mut self, key: K, value: V) -> Option<V> {
        match self.index_of(&key) {
            Ok(i) => Some(std::mem::replace(&mut self.entries[i].1, value)),
            Err(i) => {
                self.entries.insert(i, (key, value));
                None
            }
        }
    }

    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        self.entries.retain_mut(|(k, v)| keep(k, v));
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Row ids in ascending order.
    pub fn keys(&self) -> impl Iterator<Item = &K> + Clone {
        self.entries.iter().map(|(k, _)| k)
    }

    /// Rows in ascending row-id order.
    pub fn values(&self) -> impl Iterator<Item = &V> + Clone {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut V> {
        self.entries.iter_mut().map(|(_, v)| v)
    }

    /// Pairs in ascending row-id order.
    pub fn iter(&self) -> impl Iterator<Item = (&K, &V)> + Clone {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&K, &mut V)> {
        self.entries.iter_mut().map(|(k, v)| (&*k, v))
    }
}

impl<K: RowId, V> FromIterator<(K, V)> for IdMap<K, V> {
    /// Sorts by row id; on a duplicate key the last pair wins, matching
    /// `HashMap`'s `collect`.
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut entries: Vec<(K, V)> = iter.into_iter().collect();
        // Stable so that "last one wins" survives the sort, then keep the last
        // of each run of equal keys.
        entries.sort_by_key(|(k, _)| k.row_id());
        entries.reverse();
        entries.dedup_by_key(|(k, _)| k.row_id());
        entries.reverse();
        Self { entries }
    }
}

impl<K, V> IntoIterator for IdMap<K, V> {
    type Item = (K, V);
    type IntoIter = std::vec::IntoIter<(K, V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.into_iter()
    }
}

impl<'a, K, V> IntoIterator for &'a IdMap<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = std::iter::Map<std::slice::Iter<'a, (K, V)>, fn(&'a (K, V)) -> (&'a K, &'a V)>;

    fn into_iter(self) -> Self::IntoIter {
        self.entries.iter().map(|(k, v)| (k, v))
    }
}

impl<K: RowId, V> std::ops::Index<&K> for IdMap<K, V> {
    type Output = V;

    fn index(&self, key: &K) -> &V {
        self.get(key).expect("no entry found for row id")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A real row-id newtype, so the rkyv round-trip exercises the same
    // archived shape the pack uses.
    use crate::ItemId as Id;

    fn map(pairs: &[(i32, &str)]) -> IdMap<Id, String> {
        pairs.iter().map(|(k, v)| (Id(*k), v.to_string())).collect()
    }

    #[test]
    fn collect_sorts_by_row_id() {
        let m = map(&[(7, "seven"), (1, "one"), (3, "three")]);
        assert_eq!(
            m.keys().map(|k| k.0).collect::<Vec<_>>(),
            vec![1, 3, 7],
            "iteration must follow row id, not insertion order"
        );
    }

    #[test]
    fn collect_keeps_the_last_of_a_duplicate_key() {
        // `HashMap`'s `collect` does the same, and the pack generator relies
        // on it when a sheet repeats a row id.
        let m = map(&[(1, "first"), (1, "second")]);
        assert_eq!(m.len(), 1);
        assert_eq!(m.get(&Id(1)).unwrap(), "second");
    }

    #[test]
    fn get_finds_present_keys_and_misses_absent_ones() {
        let m = map(&[(1, "one"), (100, "hundred"), (50, "fifty")]);
        assert_eq!(m.get(&Id(50)).map(String::as_str), Some("fifty"));
        assert_eq!(m.get(&Id(1)).map(String::as_str), Some("one"));
        assert_eq!(m.get(&Id(100)).map(String::as_str), Some("hundred"));
        assert_eq!(m.get(&Id(51)), None);
        assert_eq!(m.get(&Id(-1)), None);
        assert!(m.contains_key(&Id(1)));
        assert!(!m.contains_key(&Id(2)));
    }

    #[test]
    fn negative_row_ids_sort_before_positive_ones() {
        // Row ids are `i32` and a few sheets use -1 for "none"; a binary
        // search keyed on an unsigned reading of the id would miss them.
        let m = map(&[(5, "five"), (-1, "none"), (0, "zero")]);
        assert_eq!(m.keys().map(|k| k.0).collect::<Vec<_>>(), vec![-1, 0, 5]);
        assert_eq!(m.get(&Id(-1)).map(String::as_str), Some("none"));
    }

    #[test]
    fn insert_replaces_in_place_and_keeps_the_order() {
        let mut m = map(&[(1, "one"), (9, "nine")]);
        assert_eq!(m.insert(Id(5), "five".into()), None);
        assert_eq!(m.insert(Id(9), "NINE".into()), Some("nine".to_string()));
        assert_eq!(m.keys().map(|k| k.0).collect::<Vec<_>>(), vec![1, 5, 9]);
        assert_eq!(m.get(&Id(9)).map(String::as_str), Some("NINE"));
    }

    #[test]
    fn retain_drops_rows_and_leaves_the_rest_searchable() {
        let mut m = map(&[(1, "one"), (2, "two"), (3, "three")]);
        m.retain(|k, _| k.0 != 2);
        assert_eq!(m.len(), 2);
        assert_eq!(m.get(&Id(2)), None);
        assert_eq!(m.get(&Id(3)).map(String::as_str), Some("three"));
    }

    #[test]
    fn round_trips_through_rkyv() {
        let m = map(&[(3, "three"), (1, "one"), (2, "two")]);
        let bytes = rkyv::to_bytes::<_, 256>(&m).unwrap();
        let back: IdMap<Id, String> = rkyv::from_bytes(&bytes).unwrap();
        assert_eq!(back, m);
    }
}
