use crate::Map;
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::iter::FusedIterator;

/// An immutable set.
///
/// Construct one with [`From`] or [`FromIterator`].
#[derive(Clone)]
pub struct Set<T: 'static> {
    pub(crate) map: Map<T, ()>,
}

impl<T: Debug> Debug for Set<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        f.debug_set().entries(self.entries()).finish()
    }
}

impl<T> Default for Set<T> {
    #[inline]
    fn default() -> Self {
        Self {
            map: Map::default(),
        }
    }
}

impl<T> Set<T> {
    /// Reconstructs a `Set` from a baked [`Map`].
    ///
    /// This is an implementation detail used by generated code; it is intentionally hidden from the public API.
    #[doc(hidden)]
    #[must_use]
    pub const fn from_baked_map(map: Map<T, ()>) -> Self {
        Self { map }
    }

    /// Returns the value in the set equal to the given value, if present.
    #[inline]
    pub fn get<Q>(&self, value: &Q) -> Option<&T>
    where
        Q: Hash + Eq + ?Sized,
        T: Borrow<Q>,
    {
        self.map.get_entry(value).map(|(v, ())| v)
    }

    /// Returns `true` if the set contains the given value.
    #[inline]
    pub fn contains<Q>(&self, value: &Q) -> bool
    where
        Q: Hash + Eq + ?Sized,
        T: Borrow<Q>,
    {
        self.map.contains_key(value)
    }

    /// Returns the number of values in the set.
    #[must_use]
    #[inline]
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// Returns `true` if the set contains no values.
    #[must_use]
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Returns an iterator over the values of the set in an unspecified order.
    #[must_use]
    #[inline]
    pub fn entries(&self) -> Entries<'_, T> {
        Entries {
            inner: self.map.keys(),
        }
    }
}

impl<T> PartialEq for Set<T>
where
    T: Eq + Hash,
{
    fn eq(&self, other: &Self) -> bool {
        self.map == other.map
    }
}

impl<T> Eq for Set<T> where T: Eq + Hash {}

/// An iterator over the values of a [`Set`].
///
/// Created by [`Set::entries`].
#[derive(Clone, Debug)]
pub struct Entries<'a, T> {
    inner: crate::Keys<'a, T, ()>,
}

impl<'a, T> Iterator for Entries<'a, T> {
    type Item = &'a T;

    #[inline]
    fn next(&mut self) -> Option<Self::Item> {
        self.inner.next()
    }

    #[inline]
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.inner.size_hint()
    }
}

impl<T> ExactSizeIterator for Entries<'_, T> {}
impl<T> FusedIterator for Entries<'_, T> {}

#[expect(
    clippy::into_iter_without_iter,
    reason = "the by-reference iterator is `Set::entries`"
)]
impl<'a, T> IntoIterator for &'a Set<T> {
    type Item = &'a T;
    type IntoIter = Entries<'a, T>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.entries()
    }
}

#[cfg(all(test, feature = "construct"))]
mod test {
    use super::Set;
    use std::collections::HashSet;

    type Value = u8;

    #[test]
    fn empty() {
        let set: Set<Value> = Set::from([]);

        assert_eq!(set, Set::default());

        assert_eq!(set.len(), 0);
        assert!(set.is_empty());

        for value in Value::MIN..=Value::MAX {
            assert!(set.get(&value).is_none());
            assert!(!set.contains(&value));
        }
    }

    #[test]
    fn single() {
        let set = Set::from([Value::MAX]);

        assert_eq!(set.len(), 1);
        assert!(!set.is_empty());

        for value in Value::MIN..Value::MAX {
            assert!(set.get(&value).is_none());
            assert!(!set.contains(&value));
        }

        assert_eq!(set.get(&Value::MAX), Some(&Value::MAX));
        assert!(set.contains(&Value::MAX));
    }

    #[test]
    fn multiple() {
        let values: Vec<_> = vec![1, 3, 9];
        let present: HashSet<_> = values.iter().copied().collect();

        let set: Set<_> = values.into_iter().collect();

        assert_eq!(set.len(), 3);
        assert!(!set.is_empty());

        for value in Value::MIN..=Value::MAX {
            if present.contains(&value) {
                assert_eq!(set.get(&value), Some(&value));
                assert!(set.contains(&value));
            } else {
                assert!(set.get(&value).is_none());
                assert!(!set.contains(&value));
            }
        }
    }

    #[test]
    fn set_iterators() {
        let set = Set::from([1u8, 2, 3]);

        assert_eq!(set.entries().len(), 3);

        let mut values: Vec<_> = set.entries().copied().collect();
        values.sort_unstable();
        assert_eq!(values, [1, 2, 3]);

        let mut by_ref: Vec<_> = (&set).into_iter().copied().collect();
        by_ref.sort_unstable();
        assert_eq!(by_ref, values);
    }

    #[test]
    fn equality() {
        let a = Set::from([1u8, 2, 3]);
        let b = Set::from([3u8, 2, 1]);
        let differs = Set::from([1u8, 2, 4]);

        assert_eq!(a, b);
        assert_ne!(a, differs);
    }

    #[test]
    fn borrow_str_lookup() {
        let set: Set<_> = ["alpha", "beta"].into_iter().map(str::to_owned).collect();

        assert!(set.contains("alpha"));
        assert_eq!(set.get("alpha").map(String::as_str), Some("alpha"));
        assert!(set.contains("beta"));
        assert_eq!(set.get("beta").map(String::as_str), Some("beta"));
        assert!(!set.contains("gamma"));
        assert_eq!(set.get("gamma"), None);
    }

    #[test]
    #[should_panic = "duplicate key present"]
    fn panic_duplicate_value() {
        drop(Set::from([Value::MAX, Value::MAX]));
    }
}
