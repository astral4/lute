//! Iterators over the contents of a [`Map`](crate::Map) or [`Set`](crate::Set).

use core::iter::FusedIterator;
use core::slice::Iter;

/// Defines an iterator that yields `$project` applied to each element of an inner iterator.
macro_rules! projecting_iter {
    (
        $(#[$doc:meta])*
        $name:ident<$lt:lifetime $(, $param:ident)*>($inner:ty) -> $item:ty,
        $project:expr
    ) => {
        $(#[$doc])*
        #[derive(Clone, Debug)]
        pub struct $name<$lt $(, $param)*> {
            pub(crate) inner: $inner,
        }

        impl<$lt $(, $param)*> Iterator for $name<$lt $(, $param)*> {
            type Item = $item;

            #[inline]
            fn next(&mut self) -> Option<Self::Item> {
                self.inner.next().map($project)
            }

            #[inline]
            fn size_hint(&self) -> (usize, Option<usize>) {
                self.inner.size_hint()
            }

            #[inline]
            fn count(self) -> usize {
                self.inner.count()
            }

            #[inline]
            fn last(self) -> Option<Self::Item> {
                self.inner.last().map($project)
            }

            #[inline]
            fn nth(&mut self, n: usize) -> Option<Self::Item> {
                self.inner.nth(n).map($project)
            }

            #[inline]
            fn fold<B, F>(self, init: B, f: F) -> B
            where
                F: FnMut(B, Self::Item) -> B,
            {
                self.inner.map($project).fold(init, f)
            }
        }

        impl<$lt $(, $param)*> DoubleEndedIterator for $name<$lt $(, $param)*> {
            #[inline]
            fn next_back(&mut self) -> Option<Self::Item> {
                self.inner.next_back().map($project)
            }

            #[inline]
            fn nth_back(&mut self, n: usize) -> Option<Self::Item> {
                self.inner.nth_back(n).map($project)
            }

            #[inline]
            fn rfold<B, F>(self, init: B, f: F) -> B
            where
                F: FnMut(B, Self::Item) -> B,
            {
                self.inner.map($project).rfold(init, f)
            }
        }

        impl<$lt $(, $param)*> ExactSizeIterator for $name<$lt $(, $param)*> {
            #[inline]
            fn len(&self) -> usize {
                self.inner.len()
            }
        }

        impl<$lt $(, $param)*> FusedIterator for $name<$lt $(, $param)*> {}
    };
}

projecting_iter! {
    /// An iterator over the key-value pairs of a [`Map`](crate::Map).
    ///
    /// Created by [`Map::entries`](crate::Map::entries).
    MapEntries<'a, K, V>(Iter<'a, (K, V)>) -> (&'a K, &'a V),
    |(k, v)| (k, v)
}

projecting_iter! {
    /// An iterator over the keys of a [`Map`](crate::Map).
    ///
    /// Created by [`Map::keys`](crate::Map::keys).
    MapKeys<'a, K, V>(Iter<'a, (K, V)>) -> &'a K,
    |(k, _)| k
}

projecting_iter! {
    /// An iterator over the values of a [`Map`](crate::Map).
    ///
    /// Created by [`Map::values`](crate::Map::values).
    MapValues<'a, K, V>(Iter<'a, (K, V)>) -> &'a V,
    |(_, v)| v
}

projecting_iter! {
    /// An iterator over the values of a [`Set`](crate::Set).
    ///
    /// Created by [`Set::entries`](crate::Set::entries).
    SetEntries<'a, T>(MapKeys<'a, T, ()>) -> &'a T,
    |value| value
}
