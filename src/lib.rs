//! # haph
//!
//! Hasher-agnostic static hashmaps

#![cfg_attr(not(test), no_std)]

mod generate;

extern crate alloc;

use alloc::vec::Vec;
use core::{borrow::Borrow, hash::Hash};
use foldhash::{HashSet, HashSetExt};

#[derive(Clone, Debug, Default)]
pub struct Map<K, V> {
    seed: u64,
    displacements: Vec<(u16, u16)>,
    entries: Vec<(K, V)>,
}

impl<K, V> Map<K, V> {
    pub fn new(entries: Vec<(K, V)>) -> Self
    where
        K: Eq + Hash,
    {
        assert!(
            entries.len() <= u16::MAX.into(),
            "cannot have more entries than possible hash values"
        );

        let keys: Vec<_> = entries.iter().map(|entry| &entry.0).collect();

        assert!(!has_duplicates(&keys), "duplicate key present");

        let (seed, state) = generate::generate(&keys);

        let mut entries = entries;
        sort_by_indices(&mut entries, state.indices);

        Self {
            seed,
            displacements: state.displacements,
            entries,
        }
    }
}

#[inline]
fn has_duplicates<T: Eq + Hash>(items: &[T]) -> bool {
    let mut set = HashSet::with_capacity(items.len());

    for item in items {
        if !set.insert(item) {
            return true;
        }
    }

    false
}

#[inline]
fn sort_by_indices<T>(data: &mut [T], mut indices: Vec<usize>) {
    for idx in 0..data.len() {
        if indices[idx] != idx {
            let mut current_idx = idx;
            loop {
                let target_idx = indices[current_idx];
                indices[current_idx] = current_idx;
                if indices[target_idx] == target_idx {
                    break;
                }
                data.swap(current_idx, target_idx);
                current_idx = target_idx;
            }
        }
    }
}

impl<K, V> Map<K, V> {
    pub fn get_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        if self.displacements.is_empty() {
            return None;
        }

        let hashes = generate::hash(key, self.seed);
        let (d1, d2) = self.displacements[hashes.0 as usize % self.displacements.len()];
        let index = generate::displace(hashes.1, hashes.2, d1, d2) as usize % self.entries.len();
        let entry = &self.entries[index];

        if entry.0.borrow() == key {
            Some((&entry.0, &entry.1))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::Map;

    #[test]
    fn empty() {
        type Key = u8;

        let map = Map::<Key, ()>::new(vec![]);

        for key in Key::MIN..=Key::MAX {
            assert!(map.get_entry(&key).is_none());
        }
    }

    #[test]
    fn single() {
        type Key = u8;

        let map = Map::<Key, &str>::new(vec![(Key::MAX, "foo")]);

        for key in Key::MIN..Key::MAX {
            assert!(map.get_entry(&key).is_none());
        }

        assert_eq!(map.get_entry(&Key::MAX), Some((&Key::MAX, &"foo")));
    }

    #[test]
    fn multiple() {
        type Key = u8;

        let entries = vec![(1, "foo"), (3, "bar"), (9, "baz")];
        let keys: Vec<_> = entries.clone().into_iter().map(|(k, _)| k).collect();

        let map = Map::<Key, &str>::new(entries);

        for key in Key::MIN..=Key::MAX {
            if !keys.contains(&key) {
                assert!(map.get_entry(&key).is_none());
            }
        }

        assert_eq!(map.get_entry(&1), Some((&1, &"foo")));
        assert_eq!(map.get_entry(&3), Some((&3, &"bar")));
        assert_eq!(map.get_entry(&9), Some((&9, &"baz")));
    }
}
