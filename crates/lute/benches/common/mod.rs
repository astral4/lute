use core::borrow::Borrow;
use core::hash::Hash;

pub(crate) const SIZES: [usize; 25] = [
    1, 2, 4, 6, 8, 9, 10, 12, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 25000,
    32768, 50000, 60000, 65000, 65535,
];

/// A family of keys to benchmark. `present` and `absent` keys must be complementary sets.
pub(crate) trait Workload {
    type Key: Eq + Hash + Clone + Send + Sync + Borrow<Self::Query> + 'static;
    type Query: ?Sized + Eq + Hash;

    fn present(n: usize) -> Vec<Self::Key>;
    fn absent(n: usize) -> Vec<Self::Key>;
}

/// 64-bit integer keys.
pub(crate) struct Ints;

impl Workload for Ints {
    type Key = u64;
    type Query = u64;

    fn present(n: usize) -> Vec<u64> {
        (0..n as u64).map(|i| i.wrapping_mul(2)).collect()
    }

    fn absent(n: usize) -> Vec<u64> {
        (0..n as u64)
            .map(|i| i.wrapping_mul(2).wrapping_add(1))
            .collect()
    }
}

/// Short string keys.
pub(crate) struct ShortStr;

impl Workload for ShortStr {
    type Key = String;
    type Query = str;

    fn present(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("key_{i:06}")).collect()
    }

    fn absent(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("absent_{i:06}")).collect()
    }
}

const LONG_PREFIX: &str = "dQw4w9WgXcQ/*_@g/long-shared-key?prefix.posts(db#11509805&z=7n";

/// Long string keys sharing a common prefix.
pub(crate) struct LongStr;

impl Workload for LongStr {
    type Key = String;
    type Query = str;

    fn present(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{LONG_PREFIX}{i:016}")).collect()
    }

    fn absent(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("{LONG_PREFIX}{:016}", i + n))
            .collect()
    }
}

pub(crate) trait Bench {
    type Key: Clone + Send + Sync + 'static;
    type Map: Sync;

    fn present(n: usize) -> Vec<Self::Key>;
    fn absent(n: usize) -> Vec<Self::Key>;

    /// The full construction.
    fn construct_full(entries: Vec<(Self::Key, usize)>) -> impl Sized;

    /// The perfect-hash search alone.
    fn construct_search(entries: Vec<(Self::Key, usize)>) -> impl Sized;

    /// Builds a queryable map.
    fn build(entries: Vec<(Self::Key, usize)>) -> Self::Map;

    /// Looks up `key`, returning the stored value if present.
    fn get(map: &Self::Map, key: &Self::Key) -> Option<usize>;
}

pub(crate) fn build_map<C: Bench>(n: usize) -> C::Map {
    C::build(C::present(n).into_iter().zip(0usize..).collect())
}

#[inline(never)]
pub(crate) fn isolated_lookup<C: Bench>(map: &C::Map, key: &C::Key) -> Option<usize> {
    C::get(map, key)
}
