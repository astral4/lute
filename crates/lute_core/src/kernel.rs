//! Perfect hashing operations.

use core::hash::{Hash, Hasher};
use foldhash::SharedSeed;
use foldhash::fast::FoldHasher;

/// The maximum number of entries.
#[doc(hidden)]
pub const MAX_LEN: usize = u16::MAX as usize;

/// Maps with at most this many entries use linear scanning for lookups; no hashing or auxiliary data involved.
pub(crate) const SCAN_MAX: usize = 1;

/// Maps with at most this many entries use the table-free "direct" strategy, which finds a single seed under which the keys are already perfect.
/// The search costs roughly `e^n / sqrt(n)` seed attempts (each hashing every key), so this strategy is only viable at sufficiently small sizes.
pub(crate) const DIRECT_MAX: usize = 10;

const _: () = assert!(SCAN_MAX < DIRECT_MAX);
const _: () = assert!(DIRECT_MAX <= usize::BITS as usize);
const _: () = assert!(bucket_count(MAX_LEN) <= u16::MAX as usize);

/// The closest odd number to `2^64 / phi`. Used because its bits are well-dispersed. See also: splitmix64 and Fibonacci hashing.
const BUCKET_MUL: u64 = 11_400_714_819_323_198_485;

/// The closest odd number to `2^64 / pi`. Used because its bits are well-dispersed. See also: FxHash, PTHash, and PtrHash.
const PILOT_MUL: u64 = 5_871_781_006_564_002_453;

/// The average number of keys per bucket in the pilot strategy. Higher = faster construction but more space usage.
const LAMBDA: usize = 4;

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

/// Reduces a 64-bit hash into `[0, len)` without division using its low 32 bits, which `foldhash` mixes best.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the result is < `len`, so it fits `usize`"
)]
#[inline]
pub(crate) fn fastrange(hash: u64, len: usize) -> usize {
    ((u64::from(hash as u32) * len as u64) >> 32) as usize
}

/// Returns the bucket index for a hash.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the shift leaves at most `num_buckets.trailing_zeros()` bits, so the result fits `usize`"
)]
#[inline]
pub(crate) fn bucket(hash: u64, num_buckets: usize) -> usize {
    debug_assert!(
        num_buckets >= 2 && num_buckets.is_power_of_two(),
        "bucket selection requires a power-of-two bucket count"
    );
    // `num_buckets` is a power of 2 and at least 2, so the scrambled hash's top bits select the bucket with a single shift.
    (hash.wrapping_mul(BUCKET_MUL) >> (64 - num_buckets.trailing_zeros())) as usize
}

/// Returns the slot that a bucket's pilot sends a hash to.
#[inline]
pub(crate) fn pilot_slot(hash: u64, pilot: u16, slots: usize) -> usize {
    let mixed = (hash ^ PILOT_MUL.wrapping_mul(u64::from(pilot))).wrapping_mul(PILOT_MUL);
    fastrange(mixed, slots)
}

/// Returns the number of slots that the pilot strategy scatters `n` keys into.
#[inline]
pub(crate) const fn slot_count(n: usize) -> usize {
    // Without the ~1% slack, the last buckets must hit exactly the few remaining free slots, which often needs pilots beyond the `u16` range.
    n + n.div_ceil(100)
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
