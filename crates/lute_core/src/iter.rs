//! Iterators over the contents of a [`Map`](crate::Map) or [`Set`](crate::Set).

use core::iter::FusedIterator;
use core::slice::Iter;

/// An iterator over the key-value pairs of a [`Map`](crate::Map).
///
/// Created by [`Map::entries`](crate::Map::entries).
#[derive(Clone, Debug)]
pub struct MapEntries<'a, K, V> {
    pub(crate) inner: Iter<'a, (K, V)>,
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

/// An iterator over the keys of a [`Map`](crate::Map).
///
/// Created by [`Map::keys`](crate::Map::keys).
#[derive(Clone, Debug)]
pub struct MapKeys<'a, K, V> {
    pub(crate) inner: Iter<'a, (K, V)>,
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

/// An iterator over the values of a [`Map`](crate::Map).
///
/// Created by [`Map::values`](crate::Map::values).
#[derive(Clone, Debug)]
pub struct MapValues<'a, K, V> {
    pub(crate) inner: Iter<'a, (K, V)>,
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

/// An iterator over the values of a [`Set`](crate::Set).
///
/// Created by [`Set::entries`](crate::Set::entries).
#[derive(Clone, Debug)]
pub struct SetEntries<'a, T> {
    pub(crate) inner: MapKeys<'a, T, ()>,
}

impl<'a, T> Iterator for SetEntries<'a, T> {
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

impl<T> ExactSizeIterator for SetEntries<'_, T> {}
impl<T> FusedIterator for SetEntries<'_, T> {}
