use crate::cow::CowSlice;
use crate::iter::{MapEntries, MapKeys, MapValues};
use crate::kernel::{
    MAX_LEN, PACKED_SLOTS, SCAN_MAX, bucket, hash, packed_index, pilot_slot, shared_seed,
};
use crate::strategy::{BakedStrategy, NO_PILOTS, Tables};
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::ops::Index;
use foldhash::SharedSeed;

/// An immutable map.
///
/// Construct one with [`From`] or [`FromIterator`].
#[derive(Clone)]
pub struct Map<K: 'static, V: 'static> {
    pub(crate) seed: u64,
    /// The expansion of `seed` used for hashing.
    pub(crate) shared_seed: SharedSeed,
    pub(crate) entries: CowSlice<(K, V)>,
    pub(crate) tables: Tables,
}

impl<K, V> Map<K, V> {
    /// Assembles a map from entries that are already in their final order and tables for addressing them.
    pub(crate) const fn new(seed: u64, entries: CowSlice<(K, V)>, tables: Tables) -> Self {
        Self {
            seed,
            shared_seed: shared_seed(seed),
            entries,
            tables,
        }
    }

    /// Reconstructs a `Map` from its serialized parts.
    ///
    /// This is an implementation detail used by generated code; it is intentionally hidden from the public API.
    /// The parts must come from an actual construction.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_baked_parts(
        seed: u64,
        entries: &'static [(K, V)],
        strategy: BakedStrategy,
    ) -> Self {
        assert!(entries.len() <= MAX_LEN, "too many entries");

        Self::new(
            seed,
            CowSlice::Borrowed(entries),
            Tables::from_baked(strategy, entries.len()),
        )
    }

    /// Returns the key-value entry corresponding to the given key, if present.
    #[inline]
    pub fn get_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        let entries: &[(K, V)] = &self.entries;
        let n = entries.len();

        if n <= SCAN_MAX {
            // Linear scanning
            return entries
                .iter()
                .find(|(k, _)| k.borrow() == key)
                .map(|(k, v)| (k, v));
        }

        let hash = hash(key, self.seed, &self.shared_seed);
        let tables = &self.tables;
        let shift = tables.bucket_shift;
        let index = if shift == NO_PILOTS {
            packed_index(hash, tables.packed_shift, &tables.packed)
        } else {
            let slot = {
                // SAFETY: `kernel::bucket` returns a value less than `pilots.len()` for any hash
                // because `shift` is `kernel::bucket_shift(pilots.len())` and `pilots.len()` is a power of 2 and at least 2.
                let pilot = *unsafe { tables.pilots.get_unchecked(bucket(hash, shift)) };
                pilot_slot(hash, pilot, tables.slots as usize)
            };
            if slot < n {
                slot
            } else {
                // SAFETY: `slot` is less than `slots` = `n + remap.len()`, so `slot - n` is a valid index into `remap`.
                usize::from(*unsafe { tables.remap.get_unchecked(slot - n) })
            }
        };

        // SAFETY: `index` is a valid entry index. Packed table values are entry indices recorded during the search,
        // non-remapped slots satisfy `slot < n`, and both tables are validated at construction.
        let (k, v) = unsafe { entries.get_unchecked(index) };

        if k.borrow() == key {
            Some((k, v))
        } else {
            None
        }
    }

    /// Returns a reference to the value corresponding to the given key, if present.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        self.get_entry(key).map(|(_, v)| v)
    }

    /// Returns `true` if the map contains an entry for the given key.
    #[inline]
    pub fn contains_key<Q>(&self, key: &Q) -> bool
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        self.get_entry(key).is_some()
    }

    /// Returns the number of entries in the map.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the map contains no entries.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns an iterator over the key-value pairs of the map in an unspecified order.
    #[must_use]
    #[inline]
    pub fn entries(&self) -> MapEntries<'_, K, V> {
        MapEntries {
            inner: self.entries.iter(),
        }
    }

    /// Returns an iterator over the keys of the map in an unspecified order.
    #[must_use]
    #[inline]
    pub fn keys(&self) -> MapKeys<'_, K, V> {
        MapKeys {
            inner: self.entries.iter(),
        }
    }

    /// Returns an iterator over the values of the map in an unspecified order.
    #[must_use]
    #[inline]
    pub fn values(&self) -> MapValues<'_, K, V> {
        MapValues {
            inner: self.entries.iter(),
        }
    }
}

impl<K: Debug, V: Debug> Debug for Map<K, V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_map().entries(self.entries()).finish()
    }
}

impl<K, V> Default for Map<K, V> {
    #[inline]
    fn default() -> Self {
        Self::from_baked_parts(
            0,
            &[],
            BakedStrategy::Packed {
                table: [0; PACKED_SLOTS],
                shift: 0,
            },
        )
    }
}

impl<Q, K, V> Index<&Q> for Map<K, V>
where
    Q: Hash + Eq + ?Sized,
    K: Borrow<Q>,
{
    type Output = V;

    /// # Panics
    ///
    /// Panics if there is no entry for the given key.
    #[inline]
    fn index(&self, index: &Q) -> &Self::Output {
        self.get(index).expect("no entry found for key")
    }
}

impl<K, V> PartialEq for Map<K, V>
where
    K: Eq + Hash,
    V: PartialEq,
{
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.entries().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl<K, V> Eq for Map<K, V>
where
    K: Eq + Hash,
    V: Eq,
{
}

#[expect(
    clippy::into_iter_without_iter,
    reason = "the by-reference iterator is `Map::entries`"
)]
impl<'a, K, V> IntoIterator for &'a Map<K, V> {
    type Item = (&'a K, &'a V);
    type IntoIter = MapEntries<'a, K, V>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.entries()
    }
}

#[cfg(test)]
mod baked_parts_test {
    use super::{BakedStrategy, MAX_LEN, Map};
    use crate::kernel::PACKED_SHIFTS;

    const fn packed(table: [u8; 16], shift: u32) -> BakedStrategy {
        BakedStrategy::Packed { table, shift }
    }

    const fn pilots(pilots: &'static [u16], remap: &'static [u16]) -> BakedStrategy {
        BakedStrategy::Pilots { pilots, remap }
    }

    #[test]
    fn empty_map_no_packed_table() {
        // A map this small never consults the packed table, so its contents are not constrained.
        let map: Map<u16, u16> = Map::from_baked_parts(0, &[], packed([7; 16], 0));
        assert_eq!(map.get(&0), None);
    }

    #[test]
    #[should_panic = "packed strategy used for an entry count that requires a pilot table"]
    fn packed_beyond_range() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 20],
            packed([0; 16], 0),
        ));
    }

    #[test]
    #[should_panic = "too many entries"]
    fn too_many_baked_entries() {
        const ENTRIES: &[(u16, u16)] = &[(0, 0); MAX_LEN + 1];

        drop(Map::from_baked_parts(0, ENTRIES, packed([0; 16], 0)));
    }

    #[test]
    #[should_panic = "packed window shift out of range"]
    fn packed_shift_out_of_range() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 4],
            packed([0; 16], PACKED_SHIFTS),
        ));
    }

    #[test]
    #[should_panic = "packed value out of range"]
    fn single_entry_packed_table() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16)],
            packed([7; 16], 0),
        ));
    }

    #[test]
    #[should_panic = "packed value out of range"]
    fn packed_value_out_of_range() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 4],
            packed([200; 16], 0),
        ));
    }

    #[test]
    #[should_panic = "pilot strategy without a pilot table"]
    fn pilots_empty() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 4],
            pilots(&[], &[0]),
        ));
    }

    #[test]
    #[should_panic = "pilot table length must match the bucket count"]
    fn pilot_length_mismatch() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 20],
            pilots(&[0; 4], &[0]),
        ));
    }

    #[test]
    #[should_panic = "remap length must match the slot slack"]
    fn remap_length_mismatch() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16); 20],
            pilots(&[0; 8], &[]),
        ));
    }

    #[test]
    #[should_panic = "remap value out of range"]
    fn remap_value_out_of_range() {
        drop(Map::from_baked_parts(
            0,
            &[(0u16, 0u16), (1, 0), (2, 0), (3, 0)],
            pilots(&[0, 0], &[4]),
        ));
    }
}

#[cfg(all(test, feature = "construct"))]
mod test {
    use super::Map;
    use core::hash::{Hash, Hasher};
    use std::collections::HashSet;

    type Key = u8;

    #[test]
    fn empty() {
        let map: Map<Key, ()> = Map::from([]);

        assert_eq!(map, Map::default());

        assert_eq!(map.len(), 0);
        assert!(map.is_empty());

        for key in Key::MIN..=Key::MAX {
            assert_eq!(map.get_entry(&key), None);
            assert_eq!(map.get(&key), None);
            assert!(!map.contains_key(&key));
        }
    }

    #[test]
    fn single() {
        let map = Map::from([(Key::MAX, "foo")]);

        assert_eq!(map.len(), 1);
        assert!(!map.is_empty());

        for key in Key::MIN..Key::MAX {
            assert_eq!(map.get_entry(&key), None);
            assert_eq!(map.get(&key), None);
            assert!(!map.contains_key(&key));
        }

        assert_eq!(map.get_entry(&Key::MAX), Some((&Key::MAX, &"foo")));
        assert_eq!(map.get(&Key::MAX), Some(&"foo"));
        assert_eq!(map[&Key::MAX], "foo");
        assert!(map.contains_key(&Key::MAX));
    }

    #[test]
    fn multiple() {
        let entries = vec![(1, "foo"), (3, "bar"), (9, "baz")];
        let keys: HashSet<_> = entries.clone().into_iter().map(|(k, _)| k).collect();

        let map: Map<_, _> = entries.into_iter().collect();

        assert_eq!(map.len(), 3);
        assert!(!map.is_empty());

        for key in Key::MIN..=Key::MAX {
            if !keys.contains(&key) {
                assert_eq!(map.get_entry(&key), None);
                assert_eq!(map.get(&key), None);
                assert!(!map.contains_key(&key));
            }
        }

        assert_eq!(map.get_entry(&1), Some((&1, &"foo")));
        assert_eq!(map.get(&1), Some(&"foo"));
        assert_eq!(map[&1], "foo");
        assert!(map.contains_key(&1));

        assert_eq!(map.get_entry(&3), Some((&3, &"bar")));
        assert_eq!(map.get(&3), Some(&"bar"));
        assert_eq!(map[&3], "bar");
        assert!(map.contains_key(&3));

        assert_eq!(map.get_entry(&9), Some((&9, &"baz")));
        assert_eq!(map.get(&9), Some(&"baz"));
        assert_eq!(map[&9], "baz");
        assert!(map.contains_key(&9));
    }

    #[test]
    fn map_iterators() {
        let map = Map::from([(1u8, "a"), (2, "b"), (3, "c")]);

        assert_eq!(map.entries().len(), 3);

        let mut keys: Vec<_> = map.keys().copied().collect();
        keys.sort_unstable();
        assert_eq!(keys, [1, 2, 3]);

        let mut values: Vec<_> = map.values().copied().collect();
        values.sort_unstable();
        assert_eq!(values, ["a", "b", "c"]);

        let mut entries: Vec<_> = map.entries().map(|(&k, &v)| (k, v)).collect();
        entries.sort_unstable();
        assert_eq!(entries, [(1, "a"), (2, "b"), (3, "c")]);

        let mut by_ref: Vec<_> = (&map).into_iter().map(|(&k, &v)| (k, v)).collect();
        by_ref.sort_unstable();
        assert_eq!(by_ref, entries);
    }

    #[test]
    fn equality() {
        let a = Map::from([(1u8, "x"), (2, "y")]);
        let b = Map::from([(2u8, "y"), (1, "x")]);
        let differs_value = Map::from([(1u8, "x"), (2, "z")]);
        let differs_key = Map::from([(1u8, "x"), (9, "y")]);

        assert_eq!(a, b);
        assert_ne!(a, differs_value);
        assert_ne!(a, differs_key);
    }

    #[test]
    fn borrow_str_lookup() {
        let map: Map<_, _> = [("alpha", 1), ("beta", 2)]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect();

        assert_eq!(map.get("alpha"), Some(&1));
        assert_eq!(map["alpha"], 1);
        assert_eq!(map.get("beta"), Some(&2));
        assert_eq!(map["beta"], 2);
        assert_eq!(map.get("gamma"), None);
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[should_panic = "duplicate key present"]
    fn panic_duplicate_key() {
        drop(Map::from([(Key::MAX, "foo"), (Key::MAX, "bar")]));
    }

    #[test]
    #[should_panic = "no entry found for key"]
    fn panic_index() {
        let map = Map::from([(Key::MAX, "foo")]);
        let _ = map[&0];
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    #[should_panic = "could not find a perfect hash function"]
    fn panic_inconsistent_hash_eq() {
        #[derive(PartialEq, Eq)]
        struct Collide(u32, u32);

        impl Hash for Collide {
            fn hash<H: Hasher>(&self, state: &mut H) {
                self.0.hash(state);
            }
        }

        drop(Map::from([(Collide(1, 1), "a"), (Collide(1, 2), "b")]));
    }
}
