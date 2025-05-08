use crate::generate;
use crate::get_crate_name;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::ops::Deref;
use databake::{quote, Bake, CrateEnv, TokenStream};
use foldhash::{HashSet, HashSetExt};

enum CowSlice<T: 'static> {
    Borrowed(&'static [T]),
    Owned(Vec<T>),
}

impl<T> Deref for CowSlice<T> {
    type Target = [T];

    fn deref(&self) -> &Self::Target {
        match *self {
            Self::Borrowed(borrowed) => borrowed,
            Self::Owned(ref owned) => owned.borrow(),
        }
    }
}

impl<T: Clone> Clone for CowSlice<T> {
    fn clone(&self) -> Self {
        match *self {
            Self::Borrowed(b) => Self::Borrowed(b),
            Self::Owned(ref o) => {
                let b: &[T] = o.borrow();
                Self::Owned(b.to_vec())
            }
        }
    }

    fn clone_from(&mut self, source: &Self) {
        match (self, source) {
            (&mut Self::Owned(ref mut dest), Self::Owned(o)) => {
                let b: &[T] = o.borrow();
                b.clone_into(dest);
            }
            (t, s) => *t = s.clone(),
        }
    }
}

impl<T: Debug> Debug for CowSlice<T> {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        match *self {
            Self::Borrowed(b) => Debug::fmt(b, f),
            Self::Owned(ref o) => Debug::fmt(o, f),
        }
    }
}

impl<T> Default for CowSlice<T> {
    fn default() -> Self {
        Self::Owned(Vec::new())
    }
}

impl<T: Bake> Bake for CowSlice<T> {
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let (name, name_tokens) = get_crate_name();

        ctx.insert(name);

        let tokens = self.iter().map(|d| d.bake(ctx));

        quote! {
            ::#name_tokens::CowSlice::Borrowed(&[#(#tokens),*])
        }
    }
}

#[allow(private_interfaces)]
#[derive(Clone, Debug, Default)]
pub struct Map<K: 'static, V: 'static> {
    #[doc(hidden)]
    pub seed: u64,
    #[doc(hidden)]
    pub displacements: CowSlice<(u16, u16)>,
    #[doc(hidden)]
    pub entries: CowSlice<(K, V)>,
}

impl<K, V> Map<K, V> {
    pub fn new(entries: Vec<(K, V)>) -> Self
    where
        K: Eq + Hash,
    {
        assert!(
            entries.len() <= u16::MAX.into(),
            "cannot have more entries than possible hash values"
        );

        let keys: Vec<_> = entries.iter().map(|entry| &entry.0).collect();

        assert!(!has_duplicates(&keys), "duplicate key present");

        let (seed, state) = generate::generate(&keys);

        let mut entries = entries;
        sort_by_indices(&mut entries, state.indices);

        Self {
            seed,
            displacements: CowSlice::Owned(state.displacements),
            entries: CowSlice::Owned(entries),
        }
    }
}

#[inline]
fn has_duplicates<T: Eq + Hash>(items: &[T]) -> bool {
    let mut set = HashSet::with_capacity(items.len());

    for item in items {
        if !set.insert(item) {
            return true;
        }
    }

    false
}

#[inline]
fn sort_by_indices<T>(data: &mut [T], mut indices: Vec<usize>) {
    for idx in 0..data.len() {
        if indices[idx] != idx {
            let mut current_idx = idx;
            loop {
                let target_idx = indices[current_idx];
                indices[current_idx] = current_idx;
                if indices[target_idx] == target_idx {
                    break;
                }
                data.swap(current_idx, target_idx);
                current_idx = target_idx;
            }
        }
    }
}

impl<K, V> Bake for Map<K, V>
where
    K: Bake,
    V: Bake,
{
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let (name, name_tokens) = get_crate_name();

        ctx.insert(name);

        let seed_tokens = self.seed.bake(ctx);
        let displacements_tokens = self.displacements.bake(ctx);
        let entries_tokens = self.entries.bake(ctx);

        quote! {
            ::#name_tokens::Map {
                seed: #seed_tokens,
                displacements: #displacements_tokens,
                entries_tokens: #entries_tokens
            }
        }
    }
}

impl<K, V> Map<K, V>
where
    K: Bake,
    V: Bake,
{
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        self.bake(&CrateEnv::default())
    }
}

impl<K, V> Map<K, V> {
    pub fn get_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        if self.displacements.is_empty() {
            return None;
        }

        let hashes = generate::hash(key, self.seed);
        let (d1, d2) = self.displacements[hashes.0 as usize % self.displacements.len()];
        let index = generate::displace(hashes.1, hashes.2, d1, d2) as usize % self.entries.len();
        let entry = &self.entries[index];

        if entry.0.borrow() == key {
            Some((&entry.0, &entry.1))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod test {
    use super::Map;

    #[test]
    fn empty() {
        type Key = u8;

        let map = Map::<Key, ()>::new(vec![]);

        for key in Key::MIN..=Key::MAX {
            assert!(map.get_entry(&key).is_none());
        }
    }

    #[test]
    fn single() {
        type Key = u8;

        let map = Map::<Key, &str>::new(vec![(Key::MAX, "foo")]);

        for key in Key::MIN..Key::MAX {
            assert!(map.get_entry(&key).is_none());
        }

        println!("{map:?}");
        println!("{}", map.to_tokens());

        assert_eq!(map.get_entry(&Key::MAX), Some((&Key::MAX, &"foo")));
    }

    #[test]
    fn multiple() {
        type Key = u8;

        let entries = vec![(1, "foo"), (3, "bar"), (9, "baz")];
        let keys: Vec<_> = entries.clone().into_iter().map(|(k, _)| k).collect();

        let map = Map::<Key, &str>::new(entries);

        for key in Key::MIN..=Key::MAX {
            if !keys.contains(&key) {
                assert!(map.get_entry(&key).is_none());
            }
        }

        assert_eq!(map.get_entry(&1), Some((&1, &"foo")));
        assert_eq!(map.get_entry(&3), Some((&3, &"bar")));
        assert_eq!(map.get_entry(&9), Some((&9, &"baz")));
    }
}
