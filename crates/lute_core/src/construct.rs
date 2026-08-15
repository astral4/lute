//! Map and set construction.

use crate::kernel::{
    SCAN_MAX, bucket, displace, fastmod, fastmod_multiplier, fastrange, hash, split,
};
use crate::map::{CowSlice, Map};
use crate::set::Set;
use alloc::{vec, vec::Vec};
use core::hash::Hash;
use core::mem::replace;
use core::ptr::{read, write};
use hashbrown::HashSet;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// Average number of keys per bucket in the CHD strategy.
const LAMBDA: usize = 4;

/// Maps with at most this many entries try the displacement-free "direct" strategy: a single seed
/// under which the keys are already perfect. The search costs roughly `e^n / sqrt(n)` seed attempts
/// (each hashing every key), so this strategy is only faster than CHD at sufficiently small sizes.
const DIRECT_MAX: usize = 10;

const _: () = assert!(SCAN_MAX < DIRECT_MAX);
const _: () = assert!(DIRECT_MAX <= usize::BITS as usize);

/// Number of seeds tried for the direct strategy before falling back to CHD.
const DIRECT_BUDGET: usize = 1 << 16;

/// Number of seeds tried for the CHD strategy before giving up. Valid key sets should be hashed perfectly
/// within the first few seeds, so exhausting this budget probably means no perfect hash exists.
/// This can happen when two distinct keys hash identically under every seed (`Hash` impl inconsistent with `Eq` impl).
const CHD_BUDGET: usize = 1 << 8;

/// The maximum number of entries.
#[doc(hidden)]
pub const MAX_LEN: usize = u16::MAX as usize;

/// Perfect hash function construction result containing the hash seed, the CHD displacement table,
/// and the permutation mapping each output slot to the index of its source key.
#[doc(hidden)]
#[derive(Debug)]
pub struct MapState {
    pub seed: u64,
    pub displacements: Vec<(u16, u16)>,
    pub indices: Vec<usize>,
}

/// The CHD displacement table plus the permutation mapping each slot to the original entry index.
type ChdTables = (Vec<(u16, u16)>, Vec<usize>);

#[inline]
fn generate<T>(entries: &[T]) -> Option<MapState>
where
    T: Hash,
{
    let n = entries.len();

    if n <= SCAN_MAX {
        Some(MapState {
            seed: 0,
            displacements: Vec::new(),
            indices: (0..n).collect(),
        })
    } else if n <= DIRECT_MAX
        && let Some(state) = generate_direct(entries, n)
    {
        Some(state)
    } else {
        generate_chd(entries, n)
    }
}

/// Searches for a single seed such that `fastrange(hash(key), n)` is a bijection over the keys.
/// Lookups then need only one hash and one multiply-shift.
fn generate_direct<T>(entries: &[T], n: usize) -> Option<MapState>
where
    T: Hash,
{
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);
    let mut slot_to_orig = vec![0usize; n];

    'seeds: for _ in 0..DIRECT_BUDGET {
        let seed = rng.next_u64();
        // Bit `s` marks slot `s` being taken on this attempt.
        let mut taken = 0usize;

        for (i, entry) in entries.iter().enumerate() {
            let slot = fastrange(hash(entry, seed), n);
            let bit = 1 << slot;
            if taken & bit != 0 {
                continue 'seeds;
            }
            taken |= bit;
            slot_to_orig[slot] = i;
        }

        return Some(MapState {
            seed,
            displacements: Vec::new(),
            indices: slot_to_orig,
        });
    }

    None
}

/// CHD (compress, hash, displace): assign keys to buckets,
/// then find per-bucket displacements packing every key into a distinct slot.
fn generate_chd<T>(entries: &[T], n: usize) -> Option<MapState>
where
    T: Hash,
{
    let mut hashes: Vec<_> = Vec::with_capacity(n);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);

    for _ in 0..CHD_BUDGET {
        let seed = rng.next_u64();
        hashes.clear();
        hashes.extend(entries.iter().map(|entry| hash(entry, seed)));

        if let Some((displacements, indices)) = try_chd(&hashes, n) {
            return Some(MapState {
                seed,
                displacements,
                indices,
            });
        }
    }

    None
}

/// Returns bucket indices ordered by descending size, with ties broken by ascending index.
///
/// Equivalent to `sort_unstable_by_key(|&b| (Reverse(size(b)), b))`,
fn order_buckets_by_size(starts: &[u16]) -> Vec<u16> {
    // `starts` is the CSR prefix sum, so bucket `b`'s size is  `starts[b + 1] - starts[b]`.
    let num_buckets = u16::try_from(starts.len() - 1).expect("num_buckets fits in u16");
    let size = |b: u16| starts[usize::from(b) + 1] - starts[usize::from(b)];
    let max = usize::from((0..num_buckets).map(size).max().unwrap_or(0));

    // Count buckets per size, then turn those counts into each size's starting output slot in place.
    let mut slots = vec![0u16; max + 1];
    for b in 0..num_buckets {
        slots[usize::from(size(b))] += 1;
    }
    let mut offset = 0u16;
    for slot in slots.iter_mut().rev() {
        offset += replace(slot, offset);
    }

    // Scatter buckets in ascending index order, so within one size they keep index order for breaking ties.
    let mut order = vec![0u16; usize::from(num_buckets)];
    for b in 0..num_buckets {
        let slot = &mut slots[usize::from(size(b))];
        order[usize::from(*slot)] = b;
        *slot += 1;
    }
    order
}

#[inline]
#[expect(
    clippy::cast_possible_truncation,
    reason = "indices, counts, and offsets fit within u16 since table_len <= u16::MAX"
)]
fn try_chd(hashes: &[u64], table_len: usize) -> Option<ChdTables> {
    // `map` slots hold the index of the key placed there. `EMPTY` is a sentinel value marking a free slot.
    // Real indices are less than `table_len`, which is at most `u16::MAX`, so `usize::MAX` is never a valid index.
    const EMPTY: usize = usize::MAX;

    debug_assert!(table_len <= MAX_LEN, "table_len must fit in u16");

    let num_buckets = table_len.div_ceil(LAMBDA);
    // Lookups distinguish the direct strategy from CHD based on whether
    // the displacement table is empty, so CHD must produce at least one bucket.
    debug_assert!(
        num_buckets >= 1,
        "CHD must emit a non-empty displacement table"
    );

    // Bucket keys with a CSR layout: one packed key array plus offsets instead of a `Vec<usize>` per bucket.
    // `u16` suffices since every index, count, and offset is at most `table_len`, which is itself at most `u16::MAX`.
    // After the prefix sum, `starts[b]..starts[b + 1]` is bucket `b`'s slice.
    let buckets: Vec<u16> = hashes
        .iter()
        .map(|&h| bucket(h, num_buckets) as u16)
        .collect();
    let mut starts = vec![0u16; num_buckets + 1];
    for &b in &buckets {
        starts[usize::from(b) + 1] += 1;
    }
    for b in 0..num_buckets {
        starts[b + 1] += starts[b];
    }

    // Scatter each key index into its bucket's slice.
    let mut bucket_keys = vec![0u16; table_len];
    let mut cursor = starts.clone();
    for (i, &b) in buckets.iter().enumerate() {
        bucket_keys[usize::from(cursor[usize::from(b)])] = i as u16;
        cursor[usize::from(b)] += 1;
    }

    // Process the largest buckets first while the table is mostly empty.
    let order = order_buckets_by_size(&starts);

    let bound = table_len as u16;
    let multiplier = fastmod_multiplier(table_len);
    let splits: Vec<_> = hashes.iter().map(|&h| split(h)).collect();
    // `values_to_add` only ever holds one bucket's keys at a time.
    // The largest bucket is processed first, so sizing to it avoids reallocating during the search.
    let max_bucket = order.first().map_or(0, |&b| {
        usize::from(starts[b as usize + 1] - starts[b as usize])
    });
    let mut values_to_add = Vec::with_capacity(max_bucket);
    let mut map = vec![EMPTY; table_len];
    let mut displacements = vec![(0u16, 0u16); num_buckets];

    'buckets: for &b in &order {
        let keys = &bucket_keys[starts[b as usize] as usize..starts[b as usize + 1] as usize];

        // Two keys in this bucket with the same split will map to the same slot under every displacement,
        // so the bucket can never be placed. We immediately bail to a reseed
        // instead of trying all `bound * bound` displacements in vain.
        for (i, &k1) in keys.iter().enumerate() {
            for &k2 in &keys[i + 1..] {
                if splits[k1 as usize] == splits[k2 as usize] {
                    return None;
                }
            }
        }

        for d1 in 0..bound {
            'disps: for d2 in 0..bound {
                values_to_add.clear();

                for &key in keys {
                    let key = key as usize;
                    let (f1, f2) = splits[key];
                    let index = fastmod(
                        displace(f1, f2, u32::from(d1), u32::from(d2)),
                        multiplier,
                        table_len,
                    );

                    // Reject if the slot is taken by a placed bucket (`map`)
                    // or an earlier key of this bucket under the current displacement (`values_to_add`).
                    if map[index] != EMPTY || values_to_add.iter().any(|&(idx, _)| idx == index) {
                        continue 'disps;
                    }

                    values_to_add.push((index, key));
                }

                displacements[b as usize] = (d1, d2);
                for &(index, key) in &values_to_add {
                    map[index] = key;
                }
                continue 'buckets;
            }
        }
        return None;
    }

    Some((displacements, map))
}

#[inline]
fn has_duplicates<T: Eq + Hash>(items: &[T]) -> bool {
    let mut set = HashSet::with_capacity(items.len());
    !items.iter().all(|item| set.insert(item))
}

/// Permutes `data` such that `data[indices[i]]` is moved to `data[i]`.
/// Clobbers `indices` as scratch space.
///
/// # Safety
///
/// `indices` must be a permutation of `0..data.len()`.
#[inline]
unsafe fn apply_permutation<T>(data: &mut [T], indices: &mut [usize]) {
    debug_assert_eq!(data.len(), indices.len());
    debug_assert!(is_permutation(indices));

    for start in 0..data.len() {
        unsafe {
            if *indices.get_unchecked(start) == start {
                continue;
            }

            let tmp = read(data.get_unchecked(start));
            let mut i = start;
            loop {
                let src = *indices.get_unchecked(i);
                *indices.get_unchecked_mut(i) = i;
                if src == start {
                    write(data.get_unchecked_mut(i), tmp);
                    break;
                }
                let moved = read(data.get_unchecked(src));
                write(data.get_unchecked_mut(i), moved);
                i = src;
            }
        }
    }
}

fn is_permutation(indices: &[usize]) -> bool {
    let n = indices.len();
    let mut seen = vec![false; n];

    indices
        .iter()
        .all(|&i| i < n && !replace(&mut seen[i], true))
}

/// Constructs a perfect hash function over `keys`, returning the resulting [`MapState`],
/// or `None` if no perfect hash function can be found.
#[doc(hidden)]
#[must_use]
pub fn construct<T>(keys: &[T]) -> Option<MapState>
where
    T: Hash,
{
    generate(keys)
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

        let keys: Vec<_> = entries.iter().map(|entry| &entry.0).collect();

        assert!(!has_duplicates(&keys), "duplicate key present");

        let mut state = generate(&keys).unwrap_or_else(|| {
            panic!(
                "could not find a perfect hash function for the given keys after {CHD_BUDGET} attempts; \
                 two distinct keys could be hashing identically (is `Hash` consistent with `Eq`?)"
            )
        });

        let mut entries = entries;
        unsafe {
            apply_permutation(&mut entries, &mut state.indices);
        }

        Self {
            seed: state.seed,
            fastmod_multiplier: fastmod_multiplier(entries.len()),
            displacements: CowSlice::Owned(state.displacements),
            entries: CowSlice::Owned(entries),
        }
    }
}

#[cfg(feature = "construct")]
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

#[cfg(feature = "construct")]
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

#[cfg(feature = "construct")]
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

#[cfg(feature = "construct")]
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
    use super::{DIRECT_MAX, MAX_LEN, apply_permutation, order_buckets_by_size};
    use crate::kernel::SCAN_MAX;
    use crate::map::Map;
    use std::cmp::Reverse;
    use std::collections::HashSet;

    #[test]
    fn apply_permutation_gather() {
        let mut data = ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec();
        let mut indices = vec![2, 0, 1, 3, 5, 4];

        // SAFETY: `indices` is a permutation of `0..data.len()`.
        unsafe { apply_permutation(&mut data, &mut indices) };

        assert_eq!(data, ["c", "a", "b", "d", "f", "e"]);
    }

    #[test]
    fn counting_sort() {
        let cases: &[&[u16]] = &[
            &[3],
            &[1, 1, 1, 1],
            &[0, 5, 2, 5, 1, 2, 5],
            &[1, 2, 3, 4, 5],
            &[5, 4, 3, 2, 1],
            &[10, 0, 0, 0, 0, 0, 0],
            &[2, 0, 2, 0, 2, 0, 2],
        ];

        for &sizes in cases {
            let mut starts = vec![0u16; sizes.len() + 1];
            for (b, &s) in sizes.iter().enumerate() {
                starts[b + 1] = starts[b] + s;
            }

            let mut expected: Vec<u16> = (0..sizes.len())
                .map(|b| u16::try_from(b).unwrap())
                .collect();
            expected.sort_unstable_by_key(|&b| (Reverse(sizes[usize::from(b)]), b));

            assert_eq!(
                order_buckets_by_size(&starts),
                expected,
                "sizes = {sizes:?}"
            );
        }
    }

    #[test]
    #[cfg_attr(miri, ignore)]
    fn strategies_across_sizes() {
        // 49152 and 50000 land in the `2^15..2^16` non-power-of-two range that previously made CHD
        // construction thrash; keeping them here exercises construct+lookup correctness in that zone.
        let sizes = (0u32..=20).chain([50, 100, 256, 1000, 49152, 50000]);
        let (mut saw_scan, mut saw_direct, mut saw_chd) = (false, false, false);

        for n in sizes {
            // `2_654_435_769` is `floor(2^32 / phi)`; its multiples scatter `0..n` into distinct keys.
            let entries: Vec<_> = (0..n).map(|k| (k.wrapping_mul(2_654_435_769), k)).collect();
            let present: HashSet<_> = entries.iter().map(|&(k, _)| k).collect();

            let map: Map<_, _> = entries.clone().into_iter().collect();

            let count = usize::try_from(n).unwrap();
            if count <= SCAN_MAX {
                assert!(
                    map.displacements.is_empty(),
                    "scan n={n} should have no displacements"
                );
                saw_scan = true;
            } else if map.displacements.is_empty() {
                saw_direct = true;
            } else {
                saw_chd = true;
            }
            if count > DIRECT_MAX {
                assert!(
                    !map.displacements.is_empty(),
                    "n={n} above DIRECT_MAX should use CHD"
                );
            }

            for &(k, v) in &entries {
                assert_eq!(map.get(&k), Some(&v), "present n={n} key={k}");
                assert_eq!(map.get_entry(&k), Some((&k, &v)), "present n={n} key={k}");
            }

            let mut checked = 0;
            for k in 0u32.. {
                if checked >= 500 {
                    break;
                }
                if !present.contains(&k) {
                    assert!(map.get(&k).is_none(), "absent n={n} key={k}");
                    checked += 1;
                }
            }
        }

        assert!(saw_scan, "scan strategy never used");
        assert!(saw_direct, "direct strategy never used");
        assert!(saw_chd, "CHD strategy never used");
    }

    #[test]
    #[cfg_attr(miri, ignore)]
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
