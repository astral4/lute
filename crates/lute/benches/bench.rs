use core::borrow::Borrow;
use core::hash::Hash;
use divan::counter::ItemsCount;
use divan::{Bencher, bench};
use lute::Map;

fn main() {
    divan::main();
}

const SIZES: [usize; 12] = [1, 2, 4, 8, 9, 10, 12, 16, 32, 64, 256, 1024];

/// A family of keys to benchmark. `present` and `absent` keys must be complementary sets.
trait Workload {
    type Key: Eq + Hash + Clone + Send + Sync + 'static;

    fn present(n: usize) -> Vec<Self::Key>;
    fn absent(n: usize) -> Vec<Self::Key>;
}

/// 64-bit integer keys.
struct Ints;

impl Workload for Ints {
    type Key = u64;

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
struct ShortStr;

impl Workload for ShortStr {
    type Key = String;

    fn present(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("key_{i:06}")).collect()
    }

    fn absent(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("absent_{i:06}")).collect()
    }
}

const LONG_PREFIX: &str = "dQw4w9WgXcQ/*_@g/long-shared-key?prefix.posts(db#11509805&z=7n";

/// Long string keys sharing a common prefix.
struct LongStr;

impl Workload for LongStr {
    type Key = String;

    fn present(n: usize) -> Vec<String> {
        (0..n).map(|i| format!("{LONG_PREFIX}{i:016}")).collect()
    }

    fn absent(n: usize) -> Vec<String> {
        (0..n)
            .map(|i| format!("{LONG_PREFIX}{:016}", i + n))
            .collect()
    }
}

fn build<T: Workload>(n: usize) -> Map<T::Key, usize> {
    T::present(n).into_iter().zip(0usize..).collect()
}

/// An opaque single lookup.
#[inline(never)]
fn isolated_lookup<K, Q>(map: &Map<K, usize>, key: &Q) -> Option<usize>
where
    K: Eq + Hash + Borrow<Q>,
    Q: Eq + Hash + ?Sized,
{
    map.get(key).copied()
}

#[bench(types = [Ints, ShortStr, LongStr], args = SIZES)]
fn construct<T: Workload>(bencher: Bencher<'_, '_>, n: usize) {
    let entries: Vec<_> = T::present(n).into_iter().zip(0usize..).collect();
    bencher
        .with_inputs(|| entries.clone())
        .bench_values(|entries| entries.into_iter().collect::<Map<_, _>>());
}

/// Repeated "hit" lookups for measuring amortized throughput.
#[bench(types = [Ints, ShortStr, LongStr], args = SIZES)]
fn get_hit<T: Workload>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build::<T>(n);
    let queries = T::present(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(*map.get(q).unwrap());
        }
        acc
    });
}

/// Repeated "hit" lookups with `#[inline(never)]` for measuring single-lookup latency.
#[bench(types = [Ints, ShortStr, LongStr], args = SIZES)]
fn get_hit_isolated<T: Workload>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build::<T>(n);
    let queries = T::present(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(isolated_lookup(&map, q).unwrap());
        }
        acc
    });
}

#[bench(types = [Ints, ShortStr, LongStr], args = SIZES)]
fn get_miss<T: Workload>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build::<T>(n);
    let queries = T::absent(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(usize::from(map.get(q).is_none()));
        }
        acc
    });
}
