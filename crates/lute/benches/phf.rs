mod common;

use common::{Bench, Ints, LongStr, SIZES, ShortStr, Workload, build_map, isolated_lookup};
use core::borrow::Borrow;
use core::marker::PhantomData;
use divan::counter::ItemsCount;
use divan::{Bencher, bench};
use phf::{Map, PhfEq, PhfHash};

fn main() {
    divan::main();
}

/// The `phf` implementation of a workload.
struct Phf<W>(PhantomData<W>);

impl<W: Workload> Bench for Phf<W>
where
    W::Key: PhfEq<W::Query>,
    W::Query: PhfHash,
{
    type Key = W::Key;
    type Map = Map<W::Key, usize>;

    fn present(n: usize) -> Vec<W::Key> {
        W::present(n)
    }

    fn absent(n: usize) -> Vec<W::Key> {
        W::absent(n)
    }

    fn construct_full(entries: Vec<(W::Key, usize)>) -> impl Sized {
        phf_parts::<W::Key, W::Query, usize>(entries)
    }

    fn construct_search(entries: Vec<(W::Key, usize)>) -> impl Sized {
        let keys: Vec<_> = entries.iter().map(|(k, _)| k.borrow()).collect();
        phf_generator::generate_hash(&keys)
    }

    fn build(entries: Vec<(W::Key, usize)>) -> Self::Map {
        build_phf_map(entries)
    }

    fn get(map: &Self::Map, key: &W::Key) -> Option<usize> {
        map.get(key.borrow()).copied()
    }
}

/// The key, displacements, and slot-ordered entries of a `phf::Map`.
type PhfParts<K, V> = (u64, Vec<(u32, u32)>, Vec<(K, V)>);

/// Runs `phf`'s perfect-hash search and materializes the entries into slot order.
fn phf_parts<K, Q, V>(entries: Vec<(K, V)>) -> PhfParts<K, V>
where
    K: Borrow<Q>,
    Q: ?Sized + PhfHash,
{
    let state = {
        let keys: Vec<_> = entries.iter().map(|(k, _)| k.borrow()).collect();
        phf_generator::generate_hash(&keys)
    };

    let mut sources: Vec<_> = entries.into_iter().map(Some).collect();
    let entries = state
        .map
        .iter()
        .map(|&i| {
            sources[i]
                .take()
                .expect("each source index appears exactly once")
        })
        .collect();

    (state.key, state.disps, entries)
}

/// Assembles a `phf::Map` at runtime, mirroring what `phf_codegen` emits at build time.
fn build_phf_map<K, Q, V>(entries: Vec<(K, V)>) -> Map<K, V>
where
    K: Borrow<Q> + 'static,
    Q: ?Sized + PhfHash,
    V: 'static,
{
    let (key, disps, entries) = phf_parts(entries);
    Map {
        key,
        disps: Box::leak(disps.into_boxed_slice()),
        entries: Box::leak(entries.into_boxed_slice()),
    }
}

#[bench(types = [Phf<Ints>, Phf<ShortStr>, Phf<LongStr>], args = SIZES)]
fn construct_full<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let entries: Vec<_> = C::present(n).into_iter().zip(0usize..).collect();
    bencher
        .with_inputs(|| entries.clone())
        .bench_values(|entries| C::construct_full(entries));
}

#[bench(types = [Phf<Ints>, Phf<ShortStr>, Phf<LongStr>], args = SIZES)]
fn construct_search<C: Bench>(bencher: Bencher<'_, '_>, n: usize) {
    let entries: Vec<_> = C::present(n).into_iter().zip(0usize..).collect();
    bencher
        .with_inputs(|| entries.clone())
        .bench_values(|entries| C::construct_search(entries));
}

/// Repeated "hit" lookups for measuring amortized throughput.
#[bench(types = [Phf<Ints>, Phf<ShortStr>, Phf<LongStr>], args = SIZES)]
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
#[bench(types = [Phf<Ints>, Phf<ShortStr>, Phf<LongStr>], args = SIZES)]
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

#[bench(types = [Phf<Ints>, Phf<ShortStr>, Phf<LongStr>], args = SIZES)]
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
