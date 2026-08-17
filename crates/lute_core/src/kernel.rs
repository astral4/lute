//! Perfect hashing operations.

use core::hash::{Hash, Hasher};
use foldhash::SharedSeed;
use foldhash::fast::FoldHasher;

/// The maximum number of entries.
#[doc(hidden)]
pub const MAX_LEN: usize = u16::MAX as usize;

/// Maps with at most this many entries use linear scanning for lookups; no hashing or auxiliary data involved.
pub(crate) const SCAN_MAX: usize = 1;

/// Maps with at most this many entries use the packed strategy, which scatters the keys over [`PACKED_SLOTS`] slots
/// and stores the resulting slot-to-entry table inline in the map.
pub(crate) const PACKED_MAX: usize = 12;

/// The number of slots to scatter keys into in the packed strategy.
pub(crate) const PACKED_SLOTS: usize = 16;

/// The number of bit windows to choose between in the packed strategy.
pub(crate) const PACKED_SHIFTS: u32 = 65 - PACKED_SLOTS.trailing_zeros();

const _: () = assert!(SCAN_MAX < PACKED_MAX);
const _: () = assert!(PACKED_MAX <= PACKED_SLOTS);
const _: () = assert!(PACKED_SLOTS.is_power_of_two() && PACKED_SLOTS <= u16::BITS as usize);
const _: () = assert!(bucket_count(MAX_LEN) <= u16::MAX as usize);

/// The average number of keys per bucket in the pilot strategy. Higher = faster construction but more space usage.
const LAMBDA: usize = 4;

/// The closest odd number to `2^32 / pi`. Used because its bits are well-dispersed. See also: FxHash, PTHash, and PtrHash.
const PILOT_MUL: u32 = 1_367_130_551;

/// Expands a map's seed into the [`SharedSeed`] used for hashing.
#[inline]
pub(crate) const fn shared_seed(seed: u64) -> SharedSeed {
    SharedSeed::from_u64(seed)
}

#[inline]
pub(crate) fn hash<T>(x: T, seed: u64, shared: &SharedSeed) -> u64
where
    T: Hash,
{
    let mut hasher = FoldHasher::with_seed(seed, shared);
    x.hash(&mut hasher);
    hasher.finish()
}

/// Returns the slot in `0..PACKED_SLOTS` that the window at `shift` puts `hash` in.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the mask keeps the result below `PACKED_SLOTS`"
)]
#[inline]
pub(crate) fn packed_slot(hash: u64, shift: u32) -> u32 {
    debug_assert!(shift < PACKED_SHIFTS);
    (hash >> shift) as u32 & (PACKED_SLOTS as u32 - 1)
}

/// Returns the entry index that the packed table `packed` assigns to `hash`.
#[inline]
pub(crate) fn packed_index(hash: u64, shift: u32, packed: &[u8; PACKED_SLOTS]) -> usize {
    // `packed_slot` is bounded by the table length, so no bounds check is needed.
    usize::from(packed[packed_slot(hash, shift) as usize])
}

/// Returns the number of pilot buckets for `n` entries.
#[inline]
pub(crate) const fn bucket_count(n: usize) -> usize {
    // Bucket selection (see `bucket`) bit-shifts instead of reducing, so a power-of-two count is needed. Rounding up keeps the average
    // bucket size in `(LAMBDA / 2, LAMBDA]` at the cost of the pilot table doubling whenever `n` crosses a `LAMBDA * 2^k` boundary.
    let buckets = n.div_ceil(LAMBDA).next_power_of_two();
    // A single bucket would mean a shift by 64.
    if buckets < 2 { 2 } else { buckets }
}

/// Returns the right shift that [`bucket`] applies for `num_buckets` buckets.
#[inline]
pub(crate) const fn bucket_shift(num_buckets: usize) -> u32 {
    debug_assert!(
        num_buckets >= 2 && num_buckets.is_power_of_two(),
        "bucket selection requires a power-of-two bucket count"
    );
    64 - num_buckets.trailing_zeros()
}

/// Returns the bucket index for a hash, given the shift from [`bucket_shift`].
#[expect(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn bucket(hash: u64, shift: u32) -> usize {
    debug_assert!(
        (50..64).contains(&shift),
        "shift must select 2..=16384 buckets"
    );
    // The half left alone by `slot_input` selects the bucket with a single shift.
    ((hash as u32) >> (shift - 32)) as usize
}

/// Returns the number of slots to scatter `n` keys into in the pilot strategy,
#[inline]
pub(crate) const fn slot_count(n: usize) -> usize {
    // Without the ~1% slack, the last buckets must hit exactly the few remaining free slots, which often needs pilots beyond the `u16` range.
    n + n.div_ceil(100)
}

/// Returns the half of a hash used for slot selection.
#[inline]
fn slot_input(hash: u64) -> u32 {
    (hash >> 32) as u32
}

/// Returns how much [`pilot_mix`] advances when the pilot goes up by one.
#[cfg(feature = "construct")]
#[inline]
pub(crate) fn pilot_step(hash: u64) -> u32 {
    slot_input(hash)
}

/// Returns the value to be reduced by [`pilot_slot`].
///
/// This is affine in the pilot; i.e. `pilot_mix(h, p + d) == pilot_mix(h, p) + d * pilot_step(h)` for `p + d <= u16::MAX`.
#[inline]
pub(crate) fn pilot_mix(hash: u64, pilot: u16) -> u32 {
    slot_input(hash).wrapping_mul(PILOT_MUL.wrapping_add(u32::from(pilot)))
}

/// Reduces the result of [`pilot_mix`] to a slot in `[0, slots)`.
#[inline]
pub(crate) fn slot_of_mix(mixed: u32, slots: usize) -> usize {
    ((u64::from(mixed) * slots as u64) >> 32) as usize
}

/// Returns the slot for a hash based on the provided pilot.
#[inline]
pub(crate) fn pilot_slot(hash: u64, pilot: u16, slots: usize) -> usize {
    slot_of_mix(pilot_mix(hash, pilot), slots)
}
