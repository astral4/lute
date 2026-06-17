use alloc::{vec, vec::Vec};
use core::cmp::Reverse;
use core::hash::{Hash, Hasher};
use foldhash::SharedSeed;
use foldhash::fast::FoldHasher;
use rand::distr::StandardUniform;
use rand::{RngExt, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// Average number of keys per bucket in the CHD strategy.
const LAMBDA: usize = 5;

/// `floor(2^64 / phi)`. Used in bucket hashing because its multiples evenly distribute keys across buckets,
/// keeping minimal-load construction fast. See also: splitmix64 and Fibonacci hashing.
const BUCKET_MUL: u64 = 11_400_714_819_323_198_485;

/// Maps with at most this many entries use linear scanning for lookups; no hashing or auxiliary data involved.
pub(crate) const SCAN_MAX: usize = 1;

/// Maps with at most this many entries try the displacement-free "direct" strategy:
/// a single seed under which the keys are already perfect. The search costs ~`e^n / sqrt(n)` seed attempts
/// (each hashing every key), so this strategy is only faster than CHD at sufficiently small sizes.
pub(crate) const DIRECT_MAX: usize = 10;

/// Number of seeds tried for the direct strategy before falling back to CHD.
const DIRECT_BUDGET: usize = 1 << 20;

const _: () = assert!(SCAN_MAX < DIRECT_MAX);

pub(crate) struct MapState {
    pub(crate) seed: u64,
    pub(crate) displacements: Vec<(u16, u16)>,
    pub(crate) indices: Vec<usize>,
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
pub(crate) fn generate<T>(entries: &[T]) -> MapState
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
    let mut seen = SlotSet::new(n);
    let mut slot_to_orig = vec![0usize; n];

    'seeds: for seed in Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED)
        .sample_iter(StandardUniform)
        .take(DIRECT_BUDGET)
    {
        seen.bump();

        for (i, entry) in entries.iter().enumerate() {
            let slot = fastrange(hash(entry, seed), n);
            if !seen.insert(slot) {
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

    for seed in Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED).sample_iter(StandardUniform) {
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

    unreachable!("the seed iterator is infinite")
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
    let mut values_to_add = Vec::with_capacity(2 * LAMBDA);
    let mut taken = SlotSet::new(table_len);
    let mut map = vec![EMPTY; table_len];
    let mut displacements = vec![(0u16, 0u16); num_buckets];

    'buckets: for &b in &order {
        let keys = &bucket_keys[starts[b as usize] as usize..starts[b as usize + 1] as usize];
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
pub(crate) fn hash<T>(x: T, seed: u64) -> u64
where
    T: Hash,
{
    let mut hasher = FoldHasher::with_seed(seed, SharedSeed::global_fixed());
    x.hash(&mut hasher);
    hasher.finish()
}

/// Produces the two displacement values from a 64-bit hash using its low 32 bits, which `foldhash` mixes best.
#[expect(
    clippy::cast_possible_truncation,
    reason = "deliberately narrowing to u16"
)]
#[inline]
pub(crate) fn split(hash: u64) -> (u16, u16) {
    ((hash >> 16) as u16, hash as u16)
}

/// Reduces a 64-bit hash into `[0, len)` without division using its low 32 bits, which `foldhash` mixes best.
#[expect(
    clippy::cast_possible_truncation,
    reason = "deliberately taking the low 32 hash bits; result is < len so it fits usize"
)]
#[inline]
pub(crate) fn fastrange(hash: u64, len: usize) -> usize {
    ((u64::from(hash as u32) * len as u64) >> 32) as usize
}

/// The CHD bucket index for a hash.
///
/// [`split`] consumes the low 32 hash bits, so the bucket must draw on a different region.
/// Otherwise, two keys colliding in the low 32 bits would share both a bucket and their displacement inputs,
/// becoming impossible to separate and forcing a reseed. Taking the high 32 bits of `hash * BUCKET_MUL`
/// keeps the bucket disjoint from `split` while still mixing in the whole hash,
/// since the multiplication propagates the well-distributed low bits upward.
#[inline]
pub(crate) fn bucket(hash: u64, num_buckets: usize) -> usize {
    fastrange(hash.wrapping_mul(BUCKET_MUL) >> 32, num_buckets)
}

#[inline]
pub(crate) fn displace(f1: u16, f2: u16, d1: u16, d2: u16) -> u16 {
    f1.wrapping_mul(d1).wrapping_add(f2).wrapping_add(d2)
}
