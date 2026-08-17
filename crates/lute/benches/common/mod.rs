use core::borrow::Borrow;
use core::hash::Hash;
use divan::Bencher;
use divan::counter::ItemsCount;
use rand_core::{Rng, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

pub(crate) const SIZES: [usize; 26] = [
    1, 2, 4, 6, 8, 10, 11, 12, 14, 16, 32, 64, 128, 256, 512, 1024, 2048, 4096, 8192, 16384, 25000,
    32768, 50000, 60000, 65000, 65535,
];

const SHUFFLE_SEED: u64 = 271_828_182_845_904_523;

/// A family of keys to benchmark.
pub(crate) trait Workload {
    type Key: Eq + Hash + Clone + Send + Sync + Borrow<Self::Query> + 'static;
    type Query: ?Sized + Eq + Hash;

    /// Returns the key at `index`. Must be injective over `0..(2 * 65535)`, twice the largest benched map size.
    fn key(index: usize) -> Self::Key;

    fn present(n: usize) -> Vec<Self::Key> {
        (0..n).map(|i| Self::key(2 * i)).collect()
    }

    fn absent(n: usize) -> Vec<Self::Key> {
        (0..n).map(|i| Self::key(2 * i + 1)).collect()
    }
}

/// 64-bit integer keys.
pub(crate) struct Ints;

impl Workload for Ints {
    type Key = u64;
    type Query = u64;

    fn key(index: usize) -> u64 {
        index as u64
    }
}

/// Short string keys.
pub(crate) struct ShortStr;

impl Workload for ShortStr {
    type Key = String;
    type Query = str;

    fn key(index: usize) -> String {
        format!("key_{index:06}")
    }
}

const LONG_PREFIX: &str = "dQw4w9WgXcQ/*_@g/long-shared-key?prefix.posts(db#11509805&z=7n";

/// Long string keys.
pub(crate) struct LongStr;

impl Workload for LongStr {
    type Key = String;
    type Query = str;

    fn key(index: usize) -> String {
        format!("{LONG_PREFIX}{index:016}")
    }
}

pub(crate) trait Bench {
    type Key: Clone + Send + Sync + 'static;
    type Map: Sync;

    fn present(n: usize) -> Vec<Self::Key>;
    fn absent(n: usize) -> Vec<Self::Key>;

    /// Builds a queryable map.
    fn build(entries: Vec<(Self::Key, usize)>) -> Self::Map;

    /// Looks up `key`, returning the stored value if present.
    fn get(map: &Self::Map, key: &Self::Key) -> Option<usize>;
}

fn build_map<C: Bench>(n: usize) -> C::Map {
    C::build(C::present(n).into_iter().zip(0usize..).collect())
}

#[inline(never)]
fn isolated_lookup<C: Bench>(map: &C::Map, key: &C::Key) -> Option<usize> {
    C::get(map, key)
}

/// Shuffles `queries` with a fixed seed via Fisher-Yates.
fn shuffled<T>(mut queries: Vec<T>) -> Vec<T> {
    let mut rng = Xoshiro256PlusPlus::seed_from_u64(SHUFFLE_SEED);
    for i in (1..queries.len()).rev() {
        let j = usize::try_from(rng.next_u64() % (i as u64 + 1)).expect("index fits in usize");
        queries.swap(i, j);
    }
    queries
}

/// An all-hit query stream.
fn hit_stream<C: Bench>(n: usize) -> (Vec<C::Key>, usize) {
    (shuffled(C::present(n)), n)
}

/// An all-miss query stream.
fn miss_stream<C: Bench>(n: usize) -> (Vec<C::Key>, usize) {
    (shuffled(C::absent(n)), 0)
}

/// Builds an `n`-entry map, builds its query stream with `stream`, and benches repeated `lookup`s over it.
fn bench_stream<C: Bench>(
    bencher: Bencher<'_, '_>,
    n: usize,
    stream: impl FnOnce(usize) -> (Vec<C::Key>, usize),
    lookup: impl Fn(&C::Map, &C::Key) -> Option<usize> + Sync,
) {
    let map = build_map::<C>(n);
    let (queries, expected_hits) = stream(n);

    let hits = queries.iter().filter(|q| lookup(&map, q).is_some()).count();
    assert_eq!(hits, expected_hits, "unexpected hit count in query stream");

    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(lookup(&map, q).unwrap_or(1));
        }
        acc
    });
}

/// Repeated "hit" lookups for measuring amortized throughput.
pub(crate) fn bench_hit<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    bench_stream::<C>(bencher, n, hit_stream::<C>, C::get);
}

/// Repeated "hit" lookups with `#[inline(never)]` for measuring single-lookup latency.
pub(crate) fn bench_hit_isolated<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    bench_stream::<C>(bencher, n, hit_stream::<C>, isolated_lookup::<C>);
}

/// Repeated "miss" lookups for measuring amortized throughput.
pub(crate) fn bench_miss<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    bench_stream::<C>(bencher, n, miss_stream::<C>, C::get);
}

/// Repeated "miss" lookups with `#[inline(never)]` for measuring single-lookup latency.
pub(crate) fn bench_miss_isolated<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    bench_stream::<C>(bencher, n, miss_stream::<C>, isolated_lookup::<C>);
}

/// Registers the query benchmarks for one adapter type.
macro_rules! query_benches {
    ($adapter:ident) => {
        $crate::common::query_benches!(@one $adapter,
            /// Repeated "hit" lookups for measuring amortized throughput.
            get_hit, bench_hit);
        $crate::common::query_benches!(@one $adapter,
            /// Repeated "hit" lookups with `#[inline(never)]` for measuring single-lookup latency.
            get_hit_isolated, bench_hit_isolated);
        $crate::common::query_benches!(@one $adapter,
            /// Repeated "miss" lookups for measuring amortized throughput.
            get_miss, bench_miss);
        $crate::common::query_benches!(@one $adapter,
            /// Repeated "miss" lookups with `#[inline(never)]` for measuring single-lookup latency.
            get_miss_isolated, bench_miss_isolated);
    };
    (@one $adapter:ident, $(#[$doc:meta])* $name:ident, $harness:ident) => {
        $(#[$doc])*
        #[divan::bench(
            types = [
                $adapter<$crate::common::Ints>,
                $adapter<$crate::common::ShortStr>,
                $adapter<$crate::common::LongStr>,
            ],
            args = $crate::common::SIZES,
        )]
        fn $name<C: $crate::common::Bench>(bencher: ::divan::Bencher<'_, '_>, n: usize) {
            $crate::common::$harness::<C>(bencher, n);
        }
    };
}
pub(crate) use query_benches;
