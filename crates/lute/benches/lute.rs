mod common;

use common::{Bench, Ints, LongStr, SIZES, ShortStr, Workload, build_map, isolated_lookup};
use core::borrow::Borrow;
use core::marker::PhantomData;
use divan::counter::ItemsCount;
use divan::{Bencher, bench};
use lute::Map;

fn main() {
    divan::main();
}

/// The `lute` implementation of a workload.
struct Lute<W>(PhantomData<W>);

impl<W: Workload> Bench for Lute<W> {
    type Key = W::Key;
    type Map = Map<W::Key, usize>;

    fn present(n: usize) -> Vec<W::Key> {
        W::present(n)
    }

    fn absent(n: usize) -> Vec<W::Key> {
        W::absent(n)
    }

    fn construct_full(entries: Vec<(W::Key, usize)>) -> impl Sized {
        entries.into_iter().collect::<Map<W::Key, usize>>()
    }

    fn construct_search(entries: Vec<(W::Key, usize)>) -> impl Sized {
        let keys: Vec<_> = entries.iter().map(|(k, _)| k).collect();
        lute_core::construct(&keys)
    }

    fn build(entries: Vec<(W::Key, usize)>) -> Self::Map {
        entries.into_iter().collect()
    }

    fn get(map: &Self::Map, key: &W::Key) -> Option<usize> {
        map.get(key.borrow()).copied()
    }
}

#[bench(types = [Lute<Ints>, Lute<ShortStr>, Lute<LongStr>], args = SIZES)]
fn construct_full<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let entries: Vec<_> = C::present(n).into_iter().zip(0usize..).collect();
    bencher
        .with_inputs(|| entries.clone())
        .bench_values(|entries| C::construct_full(entries));
}

#[bench(types = [Lute<Ints>, Lute<ShortStr>, Lute<LongStr>], args = SIZES)]
fn construct_search<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let entries: Vec<_> = C::present(n).into_iter().zip(0usize..).collect();
    bencher
        .with_inputs(|| entries.clone())
        .bench_values(|entries| C::construct_search(entries));
}

/// Repeated "hit" lookups for measuring amortized throughput.
#[bench(types = [Lute<Ints>, Lute<ShortStr>, Lute<LongStr>], args = SIZES)]
fn get_hit<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build_map::<C>(n);
    let queries = C::present(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(C::get(&map, q).unwrap());
        }
        acc
    });
}

/// Repeated "hit" lookups with `#[inline(never)]` for measuring single-lookup latency.
#[bench(types = [Lute<Ints>, Lute<ShortStr>, Lute<LongStr>], args = SIZES)]
fn get_hit_isolated<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build_map::<C>(n);
    let queries = C::present(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(isolated_lookup::<C>(&map, q).unwrap());
        }
        acc
    });
}

#[bench(types = [Lute<Ints>, Lute<ShortStr>, Lute<LongStr>], args = SIZES)]
fn get_miss<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let map = build_map::<C>(n);
    let queries = C::absent(n);
    bencher.counter(ItemsCount::new(queries.len())).bench(|| {
        let mut acc = 0usize;
        for q in &queries {
            acc = acc.wrapping_add(usize::from(C::get(&map, q).is_none()));
        }
        acc
    });
}
