//! Map and set construction.

use crate::kernel::{
    MAX_LEN, PACKED_MAX, PACKED_SHIFTS, PACKED_SLOTS, SCAN_MAX, bucket, bucket_count, bucket_shift,
    hash, packed_slot, pilot_mix, pilot_slot, pilot_step, shared_seed, slot_count, slot_of_mix,
};
use crate::map::{CowSlice, Map, pilot_shift, slot_total};
use crate::set::Set;
use alloc::{vec, vec::Vec};
use core::hash::Hash;
use core::iter::zip;
use core::mem::replace;
use core::ptr::{read, write};
use foldhash::SharedSeed;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

/// The number of seeds tried for the packed strategy before falling back to the pilot strategy.
const PACKED_SEED_BUDGET: usize = 16;

/// The number of pilots per batch checked by the bucket search. Must divide 65536.
const PILOT_BATCH: u32 = 8;

const _: () = assert!(PILOT_BATCH.is_power_of_two() && PILOT_BATCH <= u64::BITS);

/// The number of seeds tried for the pilot strategy before giving up. Valid key sets should be hashed perfectly
/// within the first few seeds, so exhausting this budget probably means no perfect hash exists.
/// This can happen when two distinct keys hash identically under every seed (`Hash` impl inconsistent with `Eq` impl).
const PILOT_SEED_BUDGET: usize = 1 << 8;

/// Sentinel marking a free slot in the pilot strategy. Real entry indices are less than `n`, which is at most `u16::MAX`,
/// so `u16::MAX` is never an index.
const EMPTY: u16 = u16::MAX;

/// A perfect hash function construction result.
#[doc(hidden)]
#[derive(Debug)]
pub struct MapState {
    /// The hash seed.
    pub seed: u64,
    pub strategy: Strategy,
}

/// Tables produced by a construction strategy. See [`crate::BakedStrategy`] for the form used by generated code.
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
        /// For each final position, the index of the caller's entry that belongs there. This is a permutation of `0..len`.
        indices: Vec<u16>,
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

/// Tracks keys claiming slots. One bit is used per slot.
struct SlotBitmap {
    words: Vec<u64>,
    slots: usize,
}

impl SlotBitmap {
    fn new(slots: usize) -> Self {
        Self {
            words: vec![0; slots.div_ceil(64)],
            slots,
        }
    }

    /// Returns whether `slot` is currently free.
    #[inline]
    fn is_free(&self, slot: usize) -> bool {
        debug_assert!(slot < self.slots);
        // SAFETY: `slot < self.slots <= self.words.len() * 64`.
        unsafe { *self.words.get_unchecked(slot >> 6) & (1 << (slot & 63)) == 0 }
    }

    /// Claims `slot` if currently free, or releases `slot` if currently claimed.
    #[inline]
    fn flip(&mut self, slot: usize) {
        debug_assert!(slot < self.slots);
        // SAFETY: `slot < self.slots <= self.words.len() * 64`.
        unsafe { *self.words.get_unchecked_mut(slot >> 6) ^= 1 << (slot & 63) };
    }
}

/// Builds a perfect hash function over `items`, hashing each one with `hash_item`.
#[inline]
fn generate<T>(
    items: &[T],
    hash_item: impl Fn(&T, u64, &SharedSeed) -> u64,
) -> Result<MapState, ConstructError> {
    let n = items.len();

    if n <= SCAN_MAX {
        Ok(MapState {
            seed: 0,
            strategy: Strategy::Packed {
                table: [0; PACKED_SLOTS],
                shift: 0,
            },
        })
    } else if n <= PACKED_MAX
        && let Some(state) = generate_packed(items, n, &hash_item)
    {
        Ok(state)
    } else {
        generate_pilots(items, n, &hash_item)
    }
}

/// Searches for a bit window of the hash that separates the keys into distinct slots, then records the resulting slot-to-entry table.
#[inline]
fn generate_packed<T>(
    items: &[T],
    n: usize,
    hash_item: &impl Fn(&T, u64, &SharedSeed) -> u64,
) -> Option<MapState> {
    debug_assert!(n <= PACKED_MAX);

    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);
    let mut hashes = [0u64; PACKED_MAX];
    let hashes = &mut hashes[..n];

    for _ in 0..PACKED_SEED_BUDGET {
        let seed = rng.next_u64();
        let shared = shared_seed(seed);
        for (out, item) in zip(&mut *hashes, items) {
            *out = hash_item(item, seed, &shared);
        }

        for shift in 0..PACKED_SHIFTS {
            let mut taken = 0u16;
            for &hash in &*hashes {
                taken |= 1 << packed_slot(hash, shift);
            }
            if taken.count_ones() as usize != n {
                continue;
            }

            let mut table = [0u8; PACKED_SLOTS];
            #[expect(
                clippy::cast_possible_truncation,
                reason = "`i` indexes at most `PACKED_MAX` <= 16 entries"
            )]
            for (i, &hash) in hashes.iter().enumerate() {
                table[packed_slot(hash, shift) as usize] = i as u8;
            }

            return Some(MapState {
                seed,
                strategy: Strategy::Packed { table, shift },
            });
        }
    }

    None
}

/// Assigns keys to buckets, then searches each bucket for a pilot value that scrambles its keys onto free slots.
/// Keys land in `n` plus ~1% slack slots. The keys landing past `n` are remapped back into the free slots below `n`.
#[inline]
fn generate_pilots<T>(
    items: &[T],
    n: usize,
    hash_item: &impl Fn(&T, u64, &SharedSeed) -> u64,
) -> Result<MapState, ConstructError> {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED);
    let mut hashes = Vec::with_capacity(n);

    for _ in 0..PILOT_SEED_BUDGET {
        let seed = rng.next_u64();
        let shared = shared_seed(seed);
        hashes.clear();
        hashes.extend(items.iter().map(|item| hash_item(item, seed, &shared)));

        match try_pilots(&hashes, n) {
            Ok(strategy) => return Ok(MapState { seed, strategy }),
            Err(e @ ConstructError::Identical(..)) => return Err(e),
            Err(ConstructError::Exhausted) => {}
        }
    }

    Err(ConstructError::Exhausted)
}

/// Groups the keys by bucket into a CSR layout. Returns the starting offsets of each bucket, a packed array of entry indices,
/// and a packed array of hashes. For example, `starts[b]..starts[b + 1]` is bucket `b`'s slice of the packed arrays.
///
/// `u16` suffices for the offsets since every entry index, count, and offset is at most `n`, which is at most [`MAX_LEN`],
/// and `num_buckets` is at most `MAX_LEN.div_ceil(LAMBDA).next_power_of_two()` = 16384.
fn bucket_keys(hashes: &[u64], num_buckets: usize, shift: u32) -> (Vec<u16>, Vec<u16>, Vec<u64>) {
    let mut starts = vec![0u16; num_buckets + 1];
    for &h in hashes {
        starts[bucket(h, shift) + 1] += 1;
    }
    for b in 0..num_buckets {
        starts[b + 1] += starts[b];
    }

    let mut bucket_entries = vec![0u16; hashes.len()];
    let mut bucket_hashes = vec![0u64; hashes.len()];
    let mut cursor = starts.clone();
    #[expect(
        clippy::cast_possible_truncation,
        reason = "`i` indexes `hashes`, so `i` < `n` <= `u16::MAX`"
    )]
    for (i, &h) in hashes.iter().enumerate() {
        let b = bucket(h, shift);
        let pos = usize::from(cursor[b]);
        bucket_entries[pos] = i as u16;
        bucket_hashes[pos] = h;
        cursor[b] += 1;
    }

    (starts, bucket_entries, bucket_hashes)
}

/// Returns bucket indices ordered by descending size, with ties broken by ascending index.
///
/// Equivalent to `sort_unstable_by_key(|&b| (Reverse(size(b)), b))`.
fn order_buckets_by_size(starts: &[u16]) -> Vec<u16> {
    // `starts` is the CSR prefix sum, so bucket `b`'s size is  `starts[b + 1] - starts[b]`.
    let size = |b| {
        let b = usize::from(b);
        starts[b + 1] - starts[b]
    };

    #[expect(
        clippy::cast_possible_truncation,
        reason = "`num_buckets` is at most `MAX_LEN.div_ceil(LAMBDA).next_power_of_two()` = 16384"
    )]
    let num_buckets = (starts.len() - 1) as u16;

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

fn try_pilots(hashes: &[u64], n: usize) -> Result<Strategy, ConstructError> {
    debug_assert!(n <= MAX_LEN, "entry indices must fit in u16");

    let slots = slot_count(n);
    let num_buckets = bucket_count(n);
    let shift = bucket_shift(num_buckets);

    let (starts, bucket_entries, bucket_hashes) = bucket_keys(hashes, num_buckets, shift);

    // Process the largest buckets first while the table is mostly empty.
    let order = order_buckets_by_size(&starts);

    let mut taken = SlotBitmap::new(slots);
    let mut slot_entries = vec![EMPTY; slots];
    let mut pilots = vec![0u16; num_buckets];

    'buckets: for &b in &order {
        let b = usize::from(b);
        let lo = usize::from(starts[b]);
        let hi = usize::from(starts[b + 1]);
        let b_entries = &bucket_entries[lo..hi];
        let b_hashes = &bucket_hashes[lo..hi];

        // An empty bucket is satisfied by any pilot.
        let Some((&first, rest_hashes)) = b_hashes.split_first() else {
            continue;
        };

        // Most pilots fail on the bucket's first key once the table fills up, so we check pilots in batches.
        // `kernel::pilot_mix` is affine in the pilot, so we can advance the pilot sweep via addition.
        let step = pilot_step(first);
        let mut mixed = pilot_mix(first, 0);

        for base in (0..u32::from(u16::MAX)).step_by(PILOT_BATCH as usize) {
            let mut survivors = 0u64;
            for j in 0..PILOT_BATCH {
                let slot = slot_of_mix(mixed.wrapping_add(step.wrapping_mul(j)), slots);
                survivors |= u64::from(taken.is_free(slot)) << j;
            }
            mixed = mixed.wrapping_add(step.wrapping_mul(PILOT_BATCH));

            while survivors != 0 {
                let j = survivors.trailing_zeros();
                survivors &= survivors - 1;
                #[expect(
                    clippy::cast_possible_truncation,
                    reason = "`base + j` stays within `u16`, and `PILOT_BATCH` is at most 64"
                )]
                let pilot = (base + j) as u16;

                // Claim each slot as it's accepted so the bitmap rejects a key colliding with an earlier key in the same bucket.
                taken.flip(pilot_slot(first, pilot, slots));
                let mut claimed = 0;
                while let Some(&hash) = rest_hashes.get(claimed) {
                    let slot = pilot_slot(hash, pilot, slots);
                    if !taken.is_free(slot) {
                        break;
                    }
                    taken.flip(slot);
                    claimed += 1;
                }

                if claimed == rest_hashes.len() {
                    pilots[b] = pilot;
                    for (&hash, &entry) in zip(b_hashes, b_entries) {
                        slot_entries[pilot_slot(hash, pilot, slots)] = entry;
                    }
                    continue 'buckets;
                }

                // Release a pilot's claims if it didn't work out.
                taken.flip(pilot_slot(first, pilot, slots));
                for &hash in &rest_hashes[..claimed] {
                    taken.flip(pilot_slot(hash, pilot, slots));
                }
            }
        }

        for (i, &h1) in b_hashes.iter().enumerate() {
            for (j, &h2) in b_hashes.iter().enumerate().skip(i + 1) {
                if h1 == h2 {
                    return Err(ConstructError::Identical(
                        usize::from(b_entries[i]),
                        usize::from(b_entries[j]),
                    ));
                }
            }
        }

        return Err(ConstructError::Exhausted);
    }

    let (remap, indices) = compact(&slot_entries, n);
    Ok(Strategy::Pilots {
        pilots,
        remap,
        indices,
    })
}

/// Compacts the slot table, returning the remap table and the dense entry order.
fn compact(slot_entries: &[u16], n: usize) -> (Vec<u16>, Vec<u16>) {
    let mut remap = vec![0u16; slot_entries.len() - n];
    let mut indices = slot_entries[..n].to_vec();
    let mut free = (0..n).filter(|&slot| slot_entries[slot] == EMPTY);

    #[expect(
        clippy::cast_possible_truncation,
        reason = "`hole` is a free slot below `n` <= `u16::MAX`"
    )]
    for (&overflow, target) in zip(&slot_entries[n..], &mut remap) {
        if overflow != EMPTY {
            // SAFETY: The pilot search places all `n` keys in distinct slots or fails before reaching here.
            // If `k` overflow slots are occupied, then `n − k` slots below `n` are occupied, leaving exactly `k` free.
            let hole = unsafe { free.next().unwrap_unchecked() };
            *target = hole as u16;
            indices[hole] = overflow;
        }
        // Unoccupied overflow slots keep the filler value of 0. This is fine because a present key can't land on them
        // and a missing key that does is rejected by the final key comparison.
    }

    (remap, indices)
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

/// Constructs a perfect hash function over `keys`, returning the resulting [`MapState`] or why none was found.
#[doc(hidden)]
pub fn construct<T>(keys: &[T]) -> Result<MapState, ConstructError>
where
    T: Hash,
{
    generate(keys, |key, seed, shared| hash(key, seed, shared))
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

        let state = generate(&entries, |entry, seed, shared| hash(&entry.0, seed, shared))
            .unwrap_or_else(|err| match err {
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
            });

        let (entries, packed, packed_shift, pilots, remap) = match state.strategy {
            Strategy::Packed { table, shift } => (entries, table, shift, Vec::new(), Vec::new()),
            Strategy::Pilots {
                pilots,
                remap,
                indices,
            } => {
                // SAFETY: `indices` is a permutation of `0..entries.len()`. The search places each of the `n` keys in exactly one slot
                // and pairs each occupied overflow slot with a distinct free slot below `n`.
                let entries = unsafe { gather(entries, &indices) };
                (entries, [0; PACKED_SLOTS], 0, pilots, remap)
            }
        };

        Self {
            seed: state.seed,
            shared_seed: shared_seed(state.seed),
            packed,
            packed_shift,
            bucket_shift: pilot_shift(&pilots),
            slots: slot_total(entries.len(), remap.len()),
            pilots: CowSlice::Owned(pilots),
            remap: CowSlice::Owned(remap),
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
    use super::{MAX_LEN, gather, order_buckets_by_size};
    use crate::kernel::{PACKED_MAX, SCAN_MAX, slot_count};
    use crate::map::Map;
    use std::cmp::Reverse;
    use std::collections::HashSet;

    #[test]
    fn gather_permutation() {
        let data = ["a", "b", "c", "d", "e", "f"].map(String::from).to_vec();
        // SAFETY: The indices are a permutation of `0..data.len()`.
        let gathered = unsafe { gather(data, &[2, 0, 1, 3, 5, 4]) };
        assert_eq!(gathered, ["c", "a", "b", "d", "f", "e"]);
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
        let (mut saw_scan, mut saw_packed, mut saw_pilots) = (false, false, false);

        for n in sizes {
            // `2_654_435_769` is the closest odd number to `2^32 / phi`; its multiples evenly scatter `0..n` into distinct keys.
            let entries: Vec<_> = (0..n).map(|k| (k.wrapping_mul(2_654_435_769), k)).collect();
            let map: Map<_, _> = entries.clone().into_iter().collect();

            let count = usize::try_from(n).unwrap();
            if count <= SCAN_MAX {
                assert!(map.pilots.is_empty(), "scan n={n} should have no pilots");
                saw_scan = true;
            } else if map.pilots.is_empty() {
                saw_packed = true;
            } else {
                saw_pilots = true;
            }
            if count > PACKED_MAX {
                assert!(
                    !map.pilots.is_empty(),
                    "n={n} above PACKED_MAX should use the pilot strategy"
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
        assert!(saw_packed, "packed strategy never used");
        assert!(saw_pilots, "pilot strategy never used");
    }

    #[test]
    fn packed_strategy_structured_keys() {
        fn assert_packed(name: &str, keys: impl Iterator<Item = u64>) {
            let map: Map<_, _> = keys.map(|k| (k, k)).collect();
            assert!(
                map.pilots.is_empty(),
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
