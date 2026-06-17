use crate::Map;
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::iter::FusedIterator;

/// An immutable set.
///
/// Construct one with [`Set::from_vec`], [`From`], or [`FromIterator`].
#[derive(Clone)]
pub struct Set<T: 'static> {
    #[doc(hidden)]
    pub map: Map<T, ()>,
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
