//! The packed strategy. Scatters a few keys over [`PACKED_SLOTS`] slots with one bit window of their hashes,
//! then stores the slot-to-entry table inline in the map.

use super::{FIXED_SEED, MapState, Strategy};
use crate::kernel::{PACKED_MAX, PACKED_SHIFTS, PACKED_SLOTS, packed_slot, shared_seed};
use core::iter::zip;
use foldhash::SharedSeed;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

/// The number of seeds tried before falling back to the pilot strategy.
const PACKED_SEED_BUDGET: usize = 16;

/// Searches for a bit window of the hash that separates the keys into distinct slots, then records the resulting slot-to-entry table.
#[inline]
pub(super) fn generate_packed<T>(
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
                order: None,
            });
        }
    }

    None
}
