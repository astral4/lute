mod common;
mod common_construct;

use common::{Bench, Workload, query_benches};
use common_construct::{BenchConstruct, construct_benches};
use core::borrow::Borrow;
use core::marker::PhantomData;
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

    fn build(entries: Vec<(W::Key, usize)>) -> Self::Map {
        entries.into_iter().collect()
    }

    fn get(map: &Self::Map, key: &W::Key) -> Option<usize> {
        map.get(key.borrow()).copied()
    }
}

impl<W: Workload> BenchConstruct for Lute<W> {
    fn construct_full(entries: Vec<(W::Key, usize)>) -> impl Sized {
        entries.into_iter().collect::<Map<W::Key, usize>>()
    }

    fn construct_search(entries: Vec<(W::Key, usize)>) -> impl Sized {
        let keys: Vec<_> = entries.iter().map(|(k, _)| k).collect();
        lute_core::construct(&keys)
    }
}

query_benches!(Lute);
construct_benches!(Lute);
