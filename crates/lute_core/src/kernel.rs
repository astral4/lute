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
/// `split` consumes the low 32 hash bits, so the bucket must draw on a different region.
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

#[cfg(target_pointer_width = "64")]
pub(crate) type FastmodMul = u64;
#[cfg(not(target_pointer_width = "64"))]
pub(crate) type FastmodMul = u32;

/// Precomputes the multiplier `floor(2^64 / n) + 1` for `fastmod`.
#[cfg(target_pointer_width = "64")]
#[inline]
pub(crate) const fn fastmod_multiplier(n: usize) -> FastmodMul {
    if n != 0 {
        (u64::MAX / n as u64).wrapping_add(1)
    } else {
        0
    }
}

/// Precomputes the multiplier `floor(2^32 / n) + 1` for `fastmod`.
#[cfg(not(target_pointer_width = "64"))]
#[expect(
    clippy::cast_possible_truncation,
    reason = "n is a table length <= MAX_LEN (u16::MAX), so it fits in u32"
)]
#[inline]
pub(crate) const fn fastmod_multiplier(n: usize) -> FastmodMul {
    if n != 0 {
        (u32::MAX / n as u32).wrapping_add(1)
    } else {
        0
    }
}

/// Computes `x % n` without division using the multiplier from `fastmod_constant`.
#[cfg(target_pointer_width = "64")]
#[allow(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn fastmod(x: u16, multiplier: FastmodMul, n: usize) -> usize {
    let lowbits = multiplier.wrapping_mul(u64::from(x));
    ((u128::from(lowbits) * n as u128) >> 64) as usize
}

/// Computes `x % n` without division using the multiplier from `fastmod_constant`.
#[cfg(not(target_pointer_width = "64"))]
#[allow(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn fastmod(x: u16, multiplier: FastmodMul, n: usize) -> usize {
    let lowbits = multiplier.wrapping_mul(u32::from(x));
    ((u64::from(lowbits) * n as u64) >> 32) as usize
}
