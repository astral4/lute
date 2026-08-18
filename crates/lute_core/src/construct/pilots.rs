//! The pilot strategy. Assigns keys to buckets, then searches each bucket for a pilot value that scrambles its keys onto free slots.
//! Keys land in `n` plus ~1% slack slots. The keys landing past `n` are remapped back into the free slots below `n`.

use super::{ConstructError, FIXED_SEED, MapState, Strategy};
use crate::kernel::{
    MAX_LEN, bucket, bucket_count, bucket_shift, pilot_mix, pilot_slot, pilot_step, shared_seed,
    slot_count, slot_of_mix,
};
use alloc::{vec, vec::Vec};
use core::iter::zip;
use core::mem::replace;
use foldhash::SharedSeed;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

/// The number of pilots per batch checked by the bucket search. Must divide 65536.
const PILOT_BATCH: u32 = 8;

const _: () = assert!(PILOT_BATCH.is_power_of_two() && PILOT_BATCH <= u64::BITS);

/// The number of seeds tried before giving up. Valid key sets should be hashed perfectly
/// within the first few seeds, so exhausting this budget probably means no perfect hash exists.
/// This can happen when two distinct keys hash identically under every seed (`Hash` impl inconsistent with `Eq` impl).
pub(super) const PILOT_SEED_BUDGET: usize = 1 << 8;

/// Sentinel marking a free slot. Real entry indices are less than `n`, which is at most `u16::MAX`, so `u16::MAX` is never an index.
const EMPTY: u16 = u16::MAX;

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

/// Rehashes the keys until a seed yields a complete set of pilots.
#[inline]
pub(super) fn generate_pilots<T>(
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
            Ok((strategy, order)) => {
                return Ok(MapState {
                    seed,
                    strategy,
                    order: Some(order),
                });
            }
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
/// and `num_buckets` is at most `MAX_LEN.div_ceil(LAMBDA).next_power_of_two()` = 32768.
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
        reason = "`num_buckets` is at most `MAX_LEN.div_ceil(LAMBDA).next_power_of_two()` = 32768"
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

/// Searches for a pilot per bucket under a fixed set of hashes. On success, returns the tables and the entries' placement order.
fn try_pilots(hashes: &[u64], n: usize) -> Result<(Strategy, Vec<u16>), ConstructError> {
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

    let (remap, order) = compact(&slot_entries, n);
    Ok((Strategy::Pilots { pilots, remap }, order))
}

/// Compacts the slot table, returning the remap table and the dense entry order.
fn compact(slot_entries: &[u16], n: usize) -> (Vec<u16>, Vec<u16>) {
    let mut remap = vec![0u16; slot_entries.len() - n];
    let mut order = slot_entries[..n].to_vec();
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
            order[hole] = overflow;
        }
        // Unoccupied overflow slots keep the filler value of 0. This is fine because a present key can't land on them
        // and a missing key that does is rejected by the final key comparison.
    }

    (remap, order)
}

#[cfg(test)]
mod test {
    use super::order_buckets_by_size;
    use std::cmp::Reverse;

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

            let mut expected: Vec<_> = (0..sizes.len())
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
}
