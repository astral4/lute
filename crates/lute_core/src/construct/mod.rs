//! Map and set construction.

mod packed;
mod pilots;

use packed::generate_packed;
use pilots::{PILOT_SEED_BUDGET, generate_pilots};

use crate::cow::CowSlice;
use crate::kernel::{MAX_LEN, PACKED_MAX, PACKED_SLOTS, SCAN_MAX, hash};
use crate::map::Map;
use crate::set::Set;
use crate::strategy::Tables;
use alloc::{vec, vec::Vec};
use core::hash::Hash;
use core::mem::replace;
use core::ptr::{read, write};
use foldhash::SharedSeed;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// A perfect hash function construction result.
#[doc(hidden)]
#[derive(Debug)]
pub struct MapState {
    /// The hash seed.
    pub seed: u64,
    /// The tables that map a hash to an entry index.
    pub strategy: Strategy,
    /// For each final entry position, the index of the caller's entry that belongs there.
    /// This is a permutation of `0..len`, or `None` when the entries are already in their final order.
    pub order: Option<Vec<u16>>,
}

impl MapState {
    /// The state of a map small enough to be searched by scanning: no seed, no tables, no reordering.
    const fn scan() -> Self {
        Self {
            seed: 0,
            strategy: Strategy::Packed {
                table: [0; PACKED_SLOTS],
                shift: 0,
            },
            order: None,
        }
    }
}

/// Tables produced by a construction strategy. See [`BakedStrategy`](crate::BakedStrategy) for the form used by generated code.
#[doc(hidden)]
#[derive(Debug)]
pub enum Strategy {
    /// A bit window of the hash indexes `table`.
    Packed {
        /// One entry index per slot.
        table: [u8; PACKED_SLOTS],
        /// Which bit window of the hash selects the slot.
        shift: u32,
    },
    /// The hash selects a bucket and the bucket's pilot selects a slot. Slots past the entries are remapped back.
    Pilots {
        /// One pilot per bucket.
        pilots: Vec<u16>,
        /// Entry indices for the overflow slots.
        remap: Vec<u16>,
    },
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub enum ConstructError {
    /// These two items hashed identically.
    Identical(usize, usize),
    /// No perfect hash function was found.
    Exhausted,
}

/// Builds a perfect hash function over `items`, hashing each one with `hash_item`.
#[inline]
fn generate<T>(
    items: &[T],
    hash_item: impl Fn(&T, u64, &SharedSeed) -> u64,
) -> Result<MapState, ConstructError> {
    let n = items.len();

    if n <= SCAN_MAX {
        Ok(MapState::scan())
    } else if n <= PACKED_MAX
        && let Some(state) = generate_packed(items, n, &hash_item)
    {
        Ok(state)
    } else {
        generate_pilots(items, n, &hash_item)
    }
}

/// Constructs a perfect hash function over `keys`, returning the resulting [`MapState`] or why none was found.
#[doc(hidden)]
pub fn construct<T>(keys: &[T]) -> Result<MapState, ConstructError>
where
    T: Hash,
{
    generate(keys, |key, seed, shared| hash(key, seed, shared))
}

/// Returns a vector whose `i`-th element is `data[indices[i]]`.
///
/// # Safety
///
/// `indices` must be a permutation of `0..data.len()`.
#[inline]
unsafe fn gather<T>(data: Vec<T>, indices: &[u16]) -> Vec<T> {
    debug_assert_eq!(data.len(), indices.len());
    debug_assert!(is_permutation(indices));

    let mut out: Vec<T> = Vec::with_capacity(indices.len());

    let src = data.as_ptr();
    let dst = out.as_mut_ptr();
    for (i, &from) in indices.iter().enumerate() {
        unsafe { write(dst.add(i), read(src.add(from as usize))) };
    }
    unsafe { out.set_len(indices.len()) };
    // Every element was moved into `out`, so the source must not drop them.
    let mut data = data;
    unsafe { data.set_len(0) };

    out
}

fn is_permutation(indices: &[u16]) -> bool {
    let n = indices.len();
    let mut seen = vec![false; n];

    indices.iter().all(|&i| {
        let i = i as usize;
        i < n && !replace(&mut seen[i], true)
    })
}

impl<K, V> Map<K, V> {
    /// Constructs a `Map` from a vector of key-value entries.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 entries
    /// - any duplicate keys are present
    /// - no perfect hash function can be found for the keys
    #[inline]
    fn from_vec(entries: Vec<(K, V)>) -> Self
    where
        K: Eq + Hash,
    {
        assert!(
            entries.len() <= MAX_LEN,
            "cannot have more than {MAX_LEN} entries"
        );

        let MapState {
            seed,
            strategy,
            order,
        } = generate(&entries, |entry, seed, shared| hash(&entry.0, seed, shared)).unwrap_or_else(
            |err| match err {
                ConstructError::Identical(i, j) => {
                    assert!(entries[i].0 != entries[j].0, "duplicate key present");
                    panic!(
                        "could not find a perfect hash function for the given keys; \
                        two distinct keys hash identically under every seed (is `Hash` consistent with `Eq`?)"
                    )
                }
                ConstructError::Exhausted => panic!(
                    "could not find a perfect hash function for the given keys after {PILOT_SEED_BUDGET} attempts"
                ),
            },
        );

        let entries = match order {
            // SAFETY: `order` is a permutation of `0..entries.len()`. The search places each of the `n` keys in exactly one slot
            // and pairs each occupied overflow slot with a distinct free slot below `n`.
            Some(order) => unsafe { gather(entries, &order) },
            None => entries,
        };

        let tables = match strategy {
            Strategy::Packed { table, shift } => Tables::packed(table, shift),
            Strategy::Pilots { pilots, remap } => Tables::pilots(
                CowSlice::Owned(pilots),
                CowSlice::Owned(remap),
                entries.len(),
            ),
        };

        Self::new(seed, CowSlice::Owned(entries), tables)
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for Map<K, V>
where
    K: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 entries
    /// - any duplicate keys are present
    /// - no perfect hash function can be found for the keys
    #[inline]
    fn from(entries: [(K, V); N]) -> Self {
        Self::from_vec(Vec::from(entries))
    }
}

impl<K, V> FromIterator<(K, V)> for Map<K, V>
where
    K: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 entries
    /// - any duplicate keys are present
    /// - no perfect hash function can be found for the keys
    #[inline]
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

impl<T> Set<T> {
    /// Constructs a `Set` from a vector of values.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 values
    /// - any duplicate values are present
    /// - no perfect hash function can be found for the values
    #[inline]
    fn from_vec(values: Vec<T>) -> Self
    where
        T: Eq + Hash,
    {
        Self {
            map: Map::from_vec(values.into_iter().map(|v| (v, ())).collect()),
        }
    }
}

impl<T, const N: usize> From<[T; N]> for Set<T>
where
    T: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 values
    /// - any duplicate values are present
    /// - no perfect hash function can be found for the values
    #[inline]
    fn from(values: [T; N]) -> Self {
        Self::from_vec(Vec::from(values))
    }
}

impl<T> FromIterator<T> for Set<T>
where
    T: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if:
    /// - there are more than 65535 values
    /// - any duplicate values are present
    /// - no perfect hash function can be found for the values
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

#[cfg(test)]
mod test {
    use super::{MAX_LEN, gather};
    use crate::kernel::{PACKED_MAX, SCAN_MAX, slot_count};
    use crate::map::Map;
    use std::collections::HashSet;

    #[test]
    fn gather_permutation() {
        let data = ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec();
        // SAFETY: The indices are a permutation of `0..data.len()`.
        let gathered = unsafe { gather(data, &[2, 0, 1, 3, 5, 4]) };
        assert_eq!(gathered, ["c", "a", "b", "d", "f", "e"]);
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn strategies_across_sizes() {
        /// Asserts that every entry is present and the first `absent_limit` non-entry keys are absent.
        fn check_lookups(map: &Map<u32, u32>, entries: &[(u32, u32)], n: u32, absent_limit: usize) {
            for &(k, v) in entries {
                assert_eq!(map.get(&k), Some(&v), "present n={n} key={k}");
                assert_eq!(map.get_entry(&k), Some((&k, &v)), "present n={n} key={k}");
            }

            let present: HashSet<_> = entries.iter().map(|&(k, _)| k).collect();
            let mut checked = 0;
            for k in 0u32.. {
                if checked >= absent_limit {
                    break;
                }
                if !present.contains(&k) {
                    assert_eq!(map.get(&k), None, "absent n={n} key={k}");
                    checked += 1;
                }
            }
        }

        let sizes = (0u32..=20)
            .chain([50, 100, 256, 1000, 49152, 50000])
            .chain([253, 1013, 64887]); // `n` where `kernel::slot_count(n)` is a power of 2
        let (mut saw_scan, mut saw_packed, mut saw_pilots) = (false, false, false);

        for n in sizes {
            // `2_654_435_769` is the closest odd number to `2^32 / phi`; its multiples evenly scatter `0..n` into distinct keys.
            let entries: Vec<_> = (0..n).map(|k| (k.wrapping_mul(2_654_435_769), k)).collect();
            let map: Map<_, _> = entries.clone().into_iter().collect();

            let count = usize::try_from(n).unwrap();
            if count <= SCAN_MAX {
                assert!(
                    map.tables.is_packed(),
                    "scan n={n} should have no pilot table"
                );
                saw_scan = true;
            } else if map.tables.is_packed() {
                saw_packed = true;
            } else {
                saw_pilots = true;
            }
            if count > PACKED_MAX {
                assert!(
                    !map.tables.is_packed(),
                    "n={n} above PACKED_MAX should use the pilot strategy"
                );
                assert_eq!(
                    map.tables.remap.len(),
                    slot_count(count) - count,
                    "n={n} remap table should cover the slack slots"
                );
            }

            check_lookups(&map, &entries, n, 500);
        }

        assert!(saw_scan, "scan strategy never used");
        assert!(saw_packed, "packed strategy never used");
        assert!(saw_pilots, "pilot strategy never used");
    }

    #[test]
    fn packed_strategy_structured_keys() {
        fn assert_packed(name: &str, keys: impl Iterator<Item = u64>) {
            let map: Map<_, _> = keys.map(|k| (k, k)).collect();
            assert!(
                map.tables.is_packed(),
                "{name} should use the packed strategy"
            );
            for (k, v) in map.entries() {
                assert_eq!(map.get(k), Some(v), "{name} key={k}");
            }
        }

        assert_packed("consecutive", 0..8);
        assert_packed("stride16", (0..8).map(|k| k * 16));
        assert_packed("shift16", (0..8).map(|k| k << 16));
        assert_packed("shift48", (0..8).map(|k| k << 48));
        assert_packed("big_offset", (0..8).map(|k| (1u64 << 40) + k));
        assert_packed("interleave", (0..8).map(|k| k | (k << 32)));
    }

    #[cfg_attr(miri, ignore)]
    #[test]
    fn construct_at_max_len() {
        let n = u32::try_from(MAX_LEN).expect("MAX_LEN fits in u32");
        let map: Map<_, _> = (0..n).map(|k| (k, k)).collect();

        assert_eq!(map.len(), MAX_LEN);
        for k in 0..n {
            assert_eq!(map.get(&k), Some(&k), "missing key {k}");
        }
        assert_eq!(map.get(&n), None);
    }

    #[test]
    #[should_panic = "cannot have more than"]
    fn construct_above_max_len_panics() {
        let n = u32::try_from(MAX_LEN).expect("MAX_LEN fits in u32");
        drop((0..=n).map(|k| (k, ())).collect::<Map<_, _>>());
    }
}
