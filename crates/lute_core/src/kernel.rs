//! Perfect hashing operations.

use core::hash::{Hash, Hasher};
use foldhash::SharedSeed;
use foldhash::fast::FoldHasher;

/// `floor(2^64 / phi)`. Used in bucket hashing because its multiples evenly distribute keys across buckets,
/// keeping minimal-load construction fast. See also: splitmix64 and Fibonacci hashing.
const BUCKET_MUL: u64 = 11_400_714_819_323_198_485;

/// Maps with at most this many entries use linear scanning for lookups; no hashing or auxiliary data involved.
pub(crate) const SCAN_MAX: usize = 1;

#[inline]
pub(crate) fn hash<T>(x: T, seed: u64) -> u64
where
    T: Hash,
{
    let mut hasher = FoldHasher::with_seed(seed, SharedSeed::global_fixed());
    x.hash(&mut hasher);
    hasher.finish()
}

/// Splits a 64-bit hash into the two 32-bit displacement inputs `(f1, f2)` used by `displace`.
#[expect(
    clippy::cast_possible_truncation,
    reason = "deliberately taking 32-bit halves"
)]
#[inline]
pub(crate) fn split(hash: u64) -> (u32, u32) {
    (hash as u32, (hash >> 32) as u32)
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
/// `split` feeds the raw hash halves into the displacement, so the bucket must be a different function of the hash.
/// Otherwise, a key's bucket and its displacement inputs would be trivially correlated, making same-bucket keys harder to separate.
/// Decorrelation comes from using `hash * BUCKET_MUL` (Fibonacci hashing) for a bucket index.
#[inline]
pub(crate) fn bucket(hash: u64, num_buckets: usize) -> usize {
    fastrange(hash.wrapping_mul(BUCKET_MUL) >> 32, num_buckets)
}

/// Combines the hash-derived inputs `f1`, `f2` with a bucket's displacement `(d1, d2)` into a pre-reduction slot value.
#[inline]
pub(crate) fn displace(f1: u32, f2: u32, d1: u32, d2: u32) -> u32 {
    f1.wrapping_mul(d1).wrapping_add(f2).wrapping_add(d2)
}

/// Precomputes the multiplier `floor(2^64 / n) + 1` for `fastmod`.
#[inline]
pub(crate) const fn fastmod_multiplier(n: usize) -> u64 {
    if n != 0 {
        (u64::MAX / n as u64).wrapping_add(1)
    } else {
        0
    }
}

/// Computes `x % n` without division using the multiplier from `fastmod_multiplier`.
#[allow(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn fastmod(x: u32, multiplier: u64, n: usize) -> usize {
    let lowbits = multiplier.wrapping_mul(u64::from(x));
    ((u128::from(lowbits) * n as u128) >> 64) as usize
}
