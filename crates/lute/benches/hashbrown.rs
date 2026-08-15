mod common;

use common::{Bench, Workload, query_benches};
use core::marker::PhantomData;
use foldhash::fast::FixedState;
use hashbrown::HashMap;

const HASHER_SEED: u64 = 271_828_182_845_904_523;

fn main() {
    divan::main();
}

/// The `hashbrown` implementation of a workload.
struct Hashbrown<W>(PhantomData<W>);

impl<W: Workload> Bench for Hashbrown<W> {
    type Key = W::Key;
    type Map = HashMap<W::Key, usize, FixedState>;

    fn present(n: usize) -> Vec<W::Key> {
        W::present(n)
    }

    fn absent(n: usize) -> Vec<W::Key> {
        W::absent(n)
    }

    fn build(entries: Vec<(W::Key, usize)>) -> Self::Map {
        let mut map =
            HashMap::with_capacity_and_hasher(entries.len(), FixedState::with_seed(HASHER_SEED));
        map.extend(entries);
        map
    }

    fn get(map: &Self::Map, key: &W::Key) -> Option<usize> {
        map.get(key).copied()
    }
}

query_benches!(Hashbrown);
