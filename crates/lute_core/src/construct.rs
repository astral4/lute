//! Map and set construction.

use crate::kernel::{
    DIRECT_MAX, SCAN_MAX, bucket, bucket_count, fastrange, hash, pilot_slot, shared_seed,
    slot_count,
};
use crate::map::{CowSlice, Map};
use crate::set::Set;
use alloc::{vec, vec::Vec};
use core::hash::Hash;
use core::iter::zip;
use core::mem::replace;
use core::ptr::{read, write};
use hashbrown::HashSet;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// The number of seeds tried for the direct strategy before falling back to the pilot strategy.
const DIRECT_BUDGET: usize = 1 << 16;

/// The number of seeds tried for the pilot strategy before giving up. Valid key sets should be hashed perfectly
/// within the first few seeds, so exhausting this budget probably means no perfect hash exists.
/// This can happen when two distinct keys hash identically under every seed (`Hash` impl inconsistent with `Eq` impl).
const SEED_BUDGET: usize = 1 << 8;

/// The maximum number of entries.
#[doc(hidden)]
pub const MAX_LEN: usize = u16::MAX as usize;

/// A perfect hash function construction result.
#[doc(hidden)]
#[derive(Debug)]
pub struct MapState {
    /// The hash seed.
    pub seed: u64,
    /// One pilot per bucket. Empty for the scan and direct strategies.
    pub pilots: Vec<u16>,
    /// The overflow-slot remap table.
    pub remap: Vec<u16>,
    /// For each final position, the index of the caller's entry that belongs there. This is a permutation of `0..len`.
    pub indices: Vec<usize>,
}

#[inline]
fn generate<T>(entries: &[T]) -> Option<MapState>
where
    T: Hash,
{
    let n = entries.len();

    if n <= SCAN_MAX {
        Some(MapState {
            seed: 0,
            pilots: Vec::new(),
            remap: Vec::new(),
            indices: (0..n).collect(),
        })
    } else if n <= DIRECT_MAX
        && let Some(state) = generate_direct(entries, n)
    {
        Some(state)
    } else {
        generate_pilots(entries, n)
    }
}

/// Searches for a single seed such that `fastrange(hash(key), n)` is a bijection over the keys.
/// Lookups then need only one hash and one multiply-shift.
fn generate_direct<T>(entries: &[T], n: usize) -> Option<MapState>
where
    T: Hash,
{
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);
    let mut slot_entries = vec![0usize; n];

    'seeds: for _ in 0..DIRECT_BUDGET {
        let seed = rng.next_u64();
        let shared = shared_seed(seed);
        // Bit `s` marks slot `s` being taken on this attempt.
        let mut taken = 0usize;

        for (i, entry) in entries.iter().enumerate() {
            let slot = fastrange(hash(entry, seed, &shared), n);
            let bit = 1 << slot;
            if taken & bit != 0 {
                continue 'seeds;
            }
            taken |= bit;
            slot_entries[slot] = i;
        }

        return Some(MapState {
            seed,
            pilots: Vec::new(),
            remap: Vec::new(),
            indices: slot_entries,
        });
    }

    None
}

/// Pilot strategy: Assign keys to buckets, then search each bucket for a pilot value that scrambles its keys onto free slots.
/// Keys land in `n` plus ~1% slack slots; the keys landing past `n` are remapped back into the free slots below `n`.
fn generate_pilots<T>(entries: &[T], n: usize) -> Option<MapState>
where
    T: Hash,
{
    let mut hashes: Vec<_> = Vec::with_capacity(n);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);

    for _ in 0..SEED_BUDGET {
        let seed = rng.next_u64();
        let shared = shared_seed(seed);
        hashes.clear();
        hashes.extend(entries.iter().map(|entry| hash(entry, seed, &shared)));

        if let Some((pilots, remap, indices)) = try_pilots(&hashes, n) {
            return Some(MapState {
                seed,
                pilots,
                remap,
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
    let size = |b| {
        let b = usize::from(b);
        starts[b + 1] - starts[b]
    };

    let num_buckets = u16::try_from(starts.len() - 1).expect("num_buckets fits in u16");

    // Count buckets per size, then turn those counts into each size's starting output position in place.
    let max = usize::from((0..num_buckets).map(size).max().unwrap_or(0));
    let mut offsets = vec![0u16; max + 1];
    for b in 0..num_buckets {
        offsets[usize::from(size(b))] += 1;
    }
    let mut running = 0u16;
    for offset in offsets.iter_mut().rev() {
        running += replace(offset, running);
    }

    // Scatter buckets in ascending index order, so within one size they keep index order for breaking ties.
    let mut order = vec![0u16; usize::from(num_buckets)];
    for b in 0..num_buckets {
        let offset = &mut offsets[usize::from(size(b))];
        order[usize::from(*offset)] = b;
        *offset += 1;
    }
    order
}

#[inline]
fn try_pilots(hashes: &[u64], n: usize) -> Option<(Vec<u16>, Vec<u16>, Vec<usize>)> {
    // Sentinel marking a free slot. Real entry indices are less than `n`, which is at most `u16::MAX`, so `u16::MAX` is never an index.
    const EMPTY: u16 = u16::MAX;

    debug_assert!(n <= MAX_LEN, "entry indices must fit in u16");

    let slots = slot_count(n);
    let num_buckets = bucket_count(n);

    // We bucket keys with a CSR layout (one packed entry array plus offsets) instead of a `Vec` per bucket.
    // `u16` suffices since every entry index, count, and offset is at most `n`, which is itself at most `u16::MAX`,
    // and `num_buckets` is at most `MAX_LEN.div_ceil(LAMBDA).next_power_of_two()` = 16384.
    // After the prefix sum, `starts[b]..starts[b + 1]` is bucket `b`'s slice.
    #[expect(clippy::cast_possible_truncation)]
    let key_buckets: Vec<_> = hashes
        .iter()
        .map(|&h| bucket(h, num_buckets) as u16)
        .collect();
    let mut starts = vec![0u16; num_buckets + 1];
    for &b in &key_buckets {
        starts[usize::from(b) + 1] += 1;
    }
    for b in 0..num_buckets {
        starts[b + 1] += starts[b];
    }

    let mut bucket_entries = vec![0u16; n];
    let mut bucket_hashes = vec![0u64; n];
    let mut cursor = starts.clone();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`i` indexes `hashes`, so it is less than `n`, which is <= `u16::MAX`"
    )]
    for (i, &b) in key_buckets.iter().enumerate() {
        let b = usize::from(b);
        let pos = usize::from(cursor[b]);
        bucket_entries[pos] = i as u16;
        bucket_hashes[pos] = hashes[i];
        cursor[b] += 1;
    }

    // Process the largest buckets first while the table is mostly empty.
    let order = order_buckets_by_size(&starts);

    let mut placements = match order.first() {
        Some(&b) => {
            let b = usize::from(b);
            // `placements` only ever holds one bucket's entries at a time.
            // The largest bucket is processed first, so sizing to it avoids reallocating during the search.
            Vec::with_capacity(usize::from(starts[b + 1] - starts[b]))
        }
        None => Vec::new(),
    };
    let mut taken = vec![0u64; slots.div_ceil(64)];
    let mut slot_entries = vec![EMPTY; slots];
    let mut pilots = vec![0u16; num_buckets];

    'buckets: for &b in &order {
        let b = usize::from(b);
        let lo = usize::from(starts[b]);
        let hi = usize::from(starts[b + 1]);
        let b_entries = &bucket_entries[lo..hi];
        let b_hashes = &bucket_hashes[lo..hi];

        for (i, &h1) in b_hashes.iter().enumerate() {
            for &h2 in &b_hashes[i + 1..] {
                // `kernel::fastrange` only consumes the mixed pilot's low 32 bits, and the mixing itself preserves equality in the low 32 bits.
                // So, if these two hashes match in the low 32 bits, they will land on the same slot for each pilot and we have to reseed.
                #[expect(clippy::cast_possible_truncation)]
                if h1 as u32 == h2 as u32 {
                    return None;
                }
            }
        }

        'pilots: for pilot in 0..=u16::MAX {
            placements.clear();

            for (&hash, &entry) in zip(b_hashes, b_entries) {
                let slot = pilot_slot(hash, pilot, slots);
                // Reject if the slot is taken by a placed bucket or an earlier key of this bucket under the current pilot.
                if taken[slot >> 6] & (1 << (slot & 63)) != 0
                    || placements.iter().any(|&(s, _)| s == slot)
                {
                    continue 'pilots;
                }
                placements.push((slot, entry));
            }

            pilots[b] = pilot;

            for &(slot, entry) in &placements {
                taken[slot >> 6] |= 1 << (slot & 63);
                slot_entries[slot] = entry;
            }

            continue 'buckets;
        }

        return None;
    }

    // Compact the slot table into dense entry indices. A slot below `n` is its own entry index.
    // Each occupied overflow slot is remapped to a free slot below `n`.
    let mut indices: Vec<_> = slot_entries[..n].iter().map(|&e| usize::from(e)).collect();
    let mut remap = vec![0u16; slots - n];
    let mut free = (0..n).filter(|&slot| slot_entries[slot] == EMPTY);
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`hole` is a free slot below `n`, which is <= `u16::MAX`"
    )]
    for overflow in n..slots {
        if slot_entries[overflow] != EMPTY {
            let hole = free.next().expect("a free slot per occupied overflow slot");
            remap[overflow - n] = hole as u16;
            indices[hole] = usize::from(slot_entries[overflow]);
        }
        // Unoccupied overflow slots keep the filler value of 0. This is fine because a present key can't land on them
        // and a missing key that does is rejected by the final key comparison.
    }

    Some((pilots, remap, indices))
}

#[inline]
fn has_duplicates<T: Eq + Hash>(items: &[T]) -> bool {
    let mut set = HashSet::with_capacity(items.len());
    !items.iter().all(|item| set.insert(item))
}

/// Permutes `data` such that `data[indices[i]]` is moved to `data[i]`. Clobbers `indices` as scratch space.
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

/// Constructs a perfect hash function over `keys`, returning the resulting [`MapState`], or `None` if no perfect hash function can be found.
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
                "could not find a perfect hash function for the given keys after {SEED_BUDGET} attempts; \
                 two distinct keys could be hashing identically (is `Hash` consistent with `Eq`?)"
            )
        });

        let mut entries = entries;
        // SAFETY: `state.indices` is a permutation of `0..entries.len()`. Every strategy places each of the `n` keys
        // in exactly one slot, and the pilot strategy pairs each occupied overflow slot with a distinct free slot below `n`.
        unsafe {
            apply_permutation(&mut entries, &mut state.indices);
        }

        Self {
            seed: state.seed,
            shared_seed: shared_seed(state.seed),
            pilots: CowSlice::Owned(state.pilots),
            remap: CowSlice::Owned(state.remap),
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
    use crate::kernel::{SCAN_MAX, slot_count};
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
        let (mut saw_scan, mut saw_direct, mut saw_pilots) = (false, false, false);

        for n in sizes {
            // `2_654_435_769` is the closest odd number to `2^32 / phi`; its multiples evenly scatter `0..n` into distinct keys.
            let entries: Vec<_> = (0..n).map(|k| (k.wrapping_mul(2_654_435_769), k)).collect();
            let map: Map<_, _> = entries.clone().into_iter().collect();

            let count = usize::try_from(n).unwrap();
            if count <= SCAN_MAX {
                assert!(map.pilots.is_empty(), "scan n={n} should have no pilots");
                saw_scan = true;
            } else if map.pilots.is_empty() {
                saw_direct = true;
            } else {
                saw_pilots = true;
            }
            if count > DIRECT_MAX {
                assert!(
                    !map.pilots.is_empty(),
                    "n={n} above DIRECT_MAX should use the pilot strategy"
                );
                assert_eq!(
                    map.remap.len(),
                    slot_count(count) - count,
                    "n={n} remap table should cover the slack slots"
                );
            }

            check_lookups(&map, &entries, n, 500);
        }

        assert!(saw_scan, "scan strategy never used");
        assert!(saw_direct, "direct strategy never used");
        assert!(saw_pilots, "pilot strategy never used");
    }

    #[test]
    fn direct_strategy_structured_keys() {
        fn assert_direct(name: &str, keys: impl Iterator<Item = u64>) {
            let map: Map<_, _> = keys.map(|k| (k, k)).collect();
            assert!(
                map.pilots.is_empty(),
                "{name} should use the direct strategy"
            );
            for (k, v) in map.entries() {
                assert_eq!(map.get(k), Some(v), "{name} key={k}");
            }
        }

        assert_direct("consecutive", 0..8);
        assert_direct("stride16", (0..8).map(|k| k * 16));
        assert_direct("shift16", (0..8).map(|k| k << 16));
        assert_direct("shift48", (0..8).map(|k| k << 48));
        assert_direct("big_offset", (0..8).map(|k| (1u64 << 40) + k));
        assert_direct("interleave", (0..8).map(|k| k | (k << 32)));
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
