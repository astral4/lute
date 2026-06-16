use alloc::{vec, vec::Vec};
use core::hash::{Hash, Hasher};
use foldhash::fast::FoldHasher;
use foldhash::SharedSeed;
use rand::distr::StandardUniform;
use rand::{RngExt, SeedableRng};
use rand_xoshiro::Xoshiro256PlusPlus;

const FIXED_SEED: u64 = 310_514_310_514_310_514;

const LAMBDA: usize = 5;

pub(crate) struct MapState {
    pub(crate) displacements: Vec<(u16, u16)>,
    pub(crate) indices: Vec<usize>,
}

struct Bucket {
    index: usize,
    keys: Vec<usize>,
}

impl Bucket {
    #[inline]
    fn new(index: usize) -> Self {
        Self {
            index,
            keys: Vec::new(),
        }
    }
}

#[inline]
pub(crate) fn generate<T>(entries: &[T]) -> (u64, MapState)
where
    T: Hash,
{
    Xoshiro256PlusPlus::seed_from_u64(FIXED_SEED)
        .sample_iter(StandardUniform)
        .find_map(|seed| {
            let hashes: Vec<_> = entries.iter().map(|entry| hash(entry, seed)).collect();
            try_generate(&hashes).map(|s| (seed, s))
        })
        .expect("failed to find perfect hash function")
}

#[inline]
fn try_generate(hashes: &[(u16, u16, u16)]) -> Option<MapState> {
    let table_len = hashes.len();
    let num_buckets = table_len.div_ceil(LAMBDA);

    let mut buckets: Vec<_> = (0..num_buckets).map(Bucket::new).collect();

    for (i, hash) in hashes.iter().enumerate() {
        buckets[hash.0 as usize % num_buckets].keys.push(i);
    }
    buckets.sort_by(|a, b| Ord::cmp(&a.keys.len(), &b.keys.len()).reverse());

    let mut displacements = vec![(0u16, 0u16); num_buckets];
    let mut map = vec![None; table_len];
    let mut try_map = vec![0u64; table_len];
    let mut generation = 0;
    let mut values_to_add = Vec::with_capacity(LAMBDA);

    'buckets: for bucket in &buckets {
        for d1 in 0..table_len {
            'disps: for d2 in 0..table_len {
                let (d1, d2) = (u16::try_from(d1).unwrap(), u16::try_from(d2).unwrap());
                values_to_add.clear();
                generation += 1;

                for &key in &bucket.keys {
                    let index = displace(hashes[key].1, hashes[key].2, d1, d2) as usize % table_len;

                    if map[index].is_some() || try_map[index] == generation {
                        continue 'disps;
                    }

                    try_map[index] = generation;
                    values_to_add.push((index, key));
                }

                displacements[bucket.index] = (d1, d2);
                for &(index, key) in &values_to_add {
                    map[index] = Some(key);
                }
                continue 'buckets;
            }
        }
        return None;
    }

    Some(MapState {
        displacements,
        indices: map.into_iter().map(Option::unwrap).collect(),
    })
}

#[allow(clippy::cast_possible_truncation)]
#[inline]
pub(crate) fn hash<T>(x: T, seed: u64) -> (u16, u16, u16)
where
    T: Hash,
{
    let mut hasher = FoldHasher::with_seed(seed, SharedSeed::global_fixed());
    x.hash(&mut hasher);
    let output = hasher.finish();
    ((output >> 32) as u16, (output >> 16) as u16, output as u16)
}

#[inline]
pub(crate) fn displace(f1: u16, f2: u16, d1: u16, d2: u16) -> u16 {
    f1.wrapping_mul(d1).wrapping_add(f2).wrapping_add(d2)
}
