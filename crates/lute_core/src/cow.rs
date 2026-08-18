//! A slice that is either baked into source code or created during a construction at runtime.

use alloc::vec::Vec;
use core::ops::Deref;

pub(crate) enum CowSlice<T: 'static> {
    Borrowed(&'static [T]),
    Owned(Vec<T>),
}

impl<T> CowSlice<T> {
    /// Returns the number of elements.
    pub(crate) const fn len(&self) -> usize {
        match *self {
            Self::Borrowed(borrowed) => borrowed.len(),
            Self::Owned(ref owned) => owned.len(),
        }
    }
}

impl<T> Deref for CowSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match *self {
            Self::Borrowed(borrowed) => borrowed,
            Self::Owned(ref owned) => owned,
        }
    }
}

impl<T: Clone> Clone for CowSlice<T> {
    fn clone(&self) -> Self {
        match *self {
            Self::Borrowed(borrowed) => Self::Borrowed(borrowed),
            Self::Owned(ref owned) => Self::Owned(owned.clone()),
        }
    }
}
