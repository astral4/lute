use crate::kernel::{
    SCAN_MAX, bucket, displace, fastmod, fastmod_multiplier, fastrange, hash, split,
};
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::iter::FusedIterator;
use core::ops::{Deref, Index};
use core::slice::Iter;

pub(crate) enum CowSlice<T: 'static> {
    Borrowed(&'static [T]),
    Owned(Vec<T>),
}

impl<T> Deref for CowSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match *self {
            Self::Borrowed(borrowed) => borrowed,
            Self::Owned(ref owned) => owned.borrow(),
        }
    }
}

impl<T: Clone> Clone for CowSlice<T> {
    fn clone(&self) -> Self {
        match *self {
            Self::Borrowed(b) => Self::Borrowed(b),
            Self::Owned(ref o) => {
                let b: &[T] = o.borrow();
                Self::Owned(b.to_vec())
            }
        }
    }

    fn clone_from(&mut self, source: &Self) {
        match (self, source) {
            (&mut Self::Owned(ref mut dest), Self::Owned(o)) => {
                let b: &[T] = o.borrow();
                b.clone_into(dest);
            }
            (t, s) => *t = s.clone(),
        }
    }
}

impl<T: Debug> Debug for CowSlice<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Self::Borrowed(b) => Debug::fmt(b, f),
            Self::Owned(ref o) => Debug::fmt(o, f),
        }
    }
}

impl<T> Default for CowSlice<T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

/// An immutable map.
///
/// Construct one with [`From`] or [`FromIterator`].
#[derive(Clone)]
pub struct Map<K: 'static, V: 'static> {
    pub(crate) seed: u64,
    pub(crate) fastmod_multiplier: u64,
    pub(crate) displacements: CowSlice<(u16, u16)>,
    pub(crate) entries: CowSlice<(K, V)>,
}

impl<K: Debug, V: Debug> Debug for Map<K, V> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_map().entries(self.entries()).finish()
    }
}

impl<K, V> Default for Map<K, V> {
    #[inline]
    fn default() -> Self {
        Self {
            seed: 0,
            fastmod_multiplier: fastmod_multiplier(0),
            displacements: CowSlice::default(),
            entries: CowSlice::default(),
        }
    }
}

impl<K, V> Map<K, V> {
    /// Reconstructs a `Map` from its serialized parts.
    ///
    /// This is an implementation detail used by generated code; it is intentionally hidden from the public API.
    /// The parts must come from an actual construction.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_baked_parts(
        seed: u64,
        displacements: &'static [(u16, u16)],
        entries: &'static [(K, V)],
    ) -> Self {
        Self {
            seed,
            fastmod_multiplier: fastmod_multiplier(entries.len()),
            displacements: CowSlice::Borrowed(displacements),
            entries: CowSlice::Borrowed(entries),
        }
    }

    /// Returns the key-value entry corresponding to the given key, if present.
    #[inline]
    pub fn get_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        let entries = &self.entries;
        let n = entries.len();

        if n <= SCAN_MAX {
            // Linear scanning
            return entries
                .iter()
                .find(|(k, _)| k.borrow() == key)
                .map(|(k, v)| (k, v));
        }

        let disps = &self.displacements;
        let hash = hash(key, self.seed);

        // CHD always produces at least one bucket (hence a non-empty displacement table), while the direct strategy produces none.
        // The hash is minimal and perfect, so the number of slots equals the entry count `n`.
        let index = if disps.is_empty() {
            // Direct strategy
            fastrange(hash, n)
        } else {
            // CHD
            let (f1, f2) = split(hash);
            let (d1, d2) = disps[bucket(hash, disps.len())];
            debug_assert_eq!(self.fastmod_multiplier, fastmod_multiplier(n));
            fastmod(
                displace(f1, f2, u32::from(d1), u32::from(d2)),
                self.fastmod_multiplier,
                n,
            )
        };

        let (k, v) = &entries[index];

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

/// An iterator over the key-value pairs of a [`Map`].
///
/// Created by [`Map::entries`].
#[derive(Clone, Debug)]
pub struct MapEntries<'a, K, V> {
    inner: Iter<'a, (K, V)>,
}

impl<'a, K, V> Iterator for MapEntries<'a, K, V> {
    type Item = (&'a K, &'a V);

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, v)| (k, v))
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K, V> ExactSizeIterator for MapEntries<'_, K, V> {}
impl<K, V> FusedIterator for MapEntries<'_, K, V> {}

/// An iterator over the keys of a [`Map`].
///
/// Created by [`Map::keys`].
#[derive(Clone, Debug)]
pub struct MapKeys<'a, K, V> {
    inner: Iter<'a, (K, V)>,
}

impl<'a, K, V> Iterator for MapKeys<'a, K, V> {
    type Item = &'a K;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(k, _)| k)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K, V> ExactSizeIterator for MapKeys<'_, K, V> {}
impl<K, V> FusedIterator for MapKeys<'_, K, V> {}

/// An iterator over the values of a [`Map`].
///
/// Created by [`Map::values`].
#[derive(Clone, Debug)]
pub struct MapValues<'a, K, V> {
    inner: Iter<'a, (K, V)>,
}

impl<'a, K, V> Iterator for MapValues<'a, K, V> {
    type Item = &'a V;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next().map(|(_, v)| v)
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<K, V> ExactSizeIterator for MapValues<'_, K, V> {}
impl<K, V> FusedIterator for MapValues<'_, K, V> {}

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

    #[test]
    #[cfg_attr(miri, ignore)]
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
