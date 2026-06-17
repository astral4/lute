//! Map and set construction.

use crate::kernel::{SCAN_MAX, bucket, displace, fastrange, hash, split};
use crate::map::{CowSlice, Map};
use crate::set::Set;
use alloc::{vec, vec::Vec};
use core::cmp::Reverse;
use core::hash::Hash;
use hashbrown::HashSet;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// Average number of keys per bucket in the CHD strategy.
const LAMBDA: usize = 5;

/// Maps with at most this many entries try the displacement-free "direct" strategy: a single seed
/// under which the keys are already perfect. The search costs roughly `e^n / sqrt(n)` seed attempts
/// (each hashing every key), so this strategy is only faster than CHD at sufficiently small sizes.
pub(crate) const DIRECT_MAX: usize = 10;

const _: () = assert!(SCAN_MAX < DIRECT_MAX);

/// Number of seeds tried for the direct strategy before falling back to CHD.
const DIRECT_BUDGET: usize = 1 << 16;

/// Number of seeds tried for the CHD strategy before giving up. Valid key sets should be hashed perfectly
/// within the first few seeds, so exhausting this budget probably means no perfect hash exists.
/// This can happen when two distinct keys hash identically under every seed (`Hash` impl inconsistent with `Eq` impl).
const CHD_BUDGET: usize = 1 << 8;

struct MapState {
    seed: u64,
    displacements: Vec<(u16, u16)>,
    indices: Vec<usize>,
}

/// The CHD displacement table plus the permutation mapping each slot to the original entry index.
type ChdTables = (Vec<(u16, u16)>, Vec<usize>);

/// A reusable set of occupied slots.
struct SlotSet {
    stamp: Vec<u64>,
    generation: u64,
}

impl SlotSet {
    #[inline]
    fn new(len: usize) -> Self {
        Self {
            stamp: vec![0; len],
            generation: 0,
        }
    }

    /// Begins a new round, vacating every slot.
    #[inline]
    fn bump(&mut self) {
        self.generation += 1;
    }

    /// Marks `slot` occupied for this round, returning `false` if it was already taken and `true` if it was free.
    #[inline]
    fn insert(&mut self, slot: usize) -> bool {
        if self.stamp[slot] == self.generation {
            false
        } else {
            self.stamp[slot] = self.generation;
            true
        }
    }
}

#[inline]
fn generate<T>(entries: &[T]) -> MapState
where
    T: Hash,
{
    let n = entries.len();

    if n <= SCAN_MAX {
        MapState {
            seed: 0,
            displacements: Vec::new(),
            indices: (0..n).collect(),
        }
    } else if n <= DIRECT_MAX
        && let Some(state) = generate_direct(entries, n)
    {
        state
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
    let mut taken = SlotSet::new(n);
    let mut slot_to_orig = vec![0usize; n];

    'seeds: for _ in 0..DIRECT_BUDGET {
        let seed = rng.next_u64();
        taken.bump();

        for (i, entry) in entries.iter().enumerate() {
            let slot = fastrange(hash(entry, seed), n);
            if !taken.insert(slot) {
                continue 'seeds;
            }
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
fn generate_chd<T>(entries: &[T], n: usize) -> MapState
where
    T: Hash,
{
    let mut hashes: Vec<u64> = Vec::with_capacity(n);
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);

    for _ in 0..CHD_BUDGET {
        let seed = rng.next_u64();
        hashes.clear();
        hashes.extend(entries.iter().map(|entry| hash(entry, seed)));

        if let Some((displacements, indices)) = try_chd(&hashes, n) {
            return MapState {
                seed,
                displacements,
                indices,
            };
        }
    }

    panic!(
        "could not find a perfect hash function for the given keys after {CHD_BUDGET} attempts; \
         two distinct keys could be hashing identically (is `Hash` consistent with `Eq`?)"
    );
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
    let mut starts = vec![0u16; num_buckets + 1];
    for &h in hashes {
        starts[bucket(h, num_buckets) + 1] += 1;
    }
    for b in 0..num_buckets {
        starts[b + 1] += starts[b];
    }

    // Scatter each key index into its bucket's slice.
    let mut bucket_keys = vec![0u16; table_len];
    let mut cursor = starts.clone();
    for (i, &h) in hashes.iter().enumerate() {
        let b = bucket(h, num_buckets);
        bucket_keys[cursor[b] as usize] = i as u16;
        cursor[b] += 1;
    }

    // Process the largest buckets first while the table is mostly empty.
    // This is a total order, so the sort results and output data are reproducible even with unstable sorting.
    let mut order: Vec<u16> = (0..num_buckets as u16).collect();
    order.sort_unstable_by_key(|&b| (Reverse(starts[b as usize + 1] - starts[b as usize]), b));

    let bound = table_len as u16;
    let splits: Vec<(u16, u16)> = hashes.iter().map(|&h| split(h)).collect();
    // `values_to_add` only ever holds one bucket's keys at a time.
    // The largest bucket is processed first, so sizing to it avoids reallocating during the search.
    let max_bucket = order.first().map_or(0, |&b| {
        usize::from(starts[b as usize + 1] - starts[b as usize])
    });
    let mut values_to_add = Vec::with_capacity(max_bucket);
    let mut taken = SlotSet::new(table_len);
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
                taken.bump();

                for &key in keys {
                    let key = key as usize;
                    let (f1, f2) = splits[key];
                    let index = displace(f1, f2, d1, d2) as usize % table_len;

                    if map[index] != EMPTY || !taken.insert(index) {
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
    /// Constructs a `Map` from a vector of key-value entries.
    ///
    /// # Panics
    ///
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
    #[must_use]
    #[inline]
    fn from_vec(entries: Vec<(K, V)>) -> Self
    where
        K: Eq + Hash,
    {
        assert!(
            entries.len() <= u16::MAX.into(),
            "cannot have more than 65535 entries"
        );

        let keys: Vec<_> = entries.iter().map(|entry| &entry.0).collect();

        assert!(!has_duplicates(&keys), "duplicate key present");

        let state = generate(&keys);

        let mut entries = entries;
        sort_by_indices(&mut entries, state.indices);

        Self {
            seed: state.seed,
            displacements: CowSlice::Owned(state.displacements),
            entries: CowSlice::Owned(entries),
        }
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for Map<K, V>
where
    K: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
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
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
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
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
    #[must_use]
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
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
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
    /// Panics if there are more than 65535 entries or if any keys are duplicated.
    #[inline]
    fn from_iter<I: IntoIterator<Item = T>>(iter: I) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}
