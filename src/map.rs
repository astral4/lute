use crate::generate;
use crate::get_crate_name;
use alloc::borrow::ToOwned;
use alloc::vec::Vec;
use core::borrow::Borrow;
use core::fmt::{Debug, Formatter, Result as FmtResult};
use core::hash::Hash;
use core::ops::{Deref, Index};
use databake::{Bake, CrateEnv, TokenStream, quote};
use foldhash::{HashSet, HashSetExt};

#[doc(hidden)]
pub enum CowSlice<T: 'static> {
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

/// An immutable map constructed at compile time.
///
/// Construct one with [`Map::from_vec`], [`From`], or [`FromIterator`].
#[derive(Clone, Debug)]
pub struct Map<K: 'static, V: 'static> {
    #[doc(hidden)]
    pub seed: u64,
    #[doc(hidden)]
    pub displacements: CowSlice<(u16, u16)>,
    #[doc(hidden)]
    pub entries: CowSlice<(K, V)>,
}

impl<K, V> Default for Map<K, V> {
    #[inline]
    fn default() -> Self {
        Self {
            seed: 0,
            displacements: CowSlice::default(),
            entries: CowSlice::default(),
        }
    }
}

impl<K, V> Map<K, V> {
    /// Constructs a `Map` from a vector of key-value entries.
    ///
    /// # Panics
    ///
    /// Panics if there are more than `u16::MAX` entries, or if any keys are duplicated.
    #[must_use]
    #[inline]
    pub fn from_vec(entries: Vec<(K, V)>) -> Self
    where
        K: Eq + Hash,
    {
        assert!(
            entries.len() <= u16::MAX.into(),
            "cannot have more entries than possible hash values"
        );

        let keys: Vec<_> = entries.iter().map(|entry| &entry.0).collect();

        assert!(!has_duplicates(&keys), "duplicate key present");

        let state = generate::generate(&keys);

        let mut entries = entries;
        sort_by_indices(&mut entries, state.indices);

        Self {
            seed: state.seed,
            displacements: CowSlice::Owned(state.displacements),
            entries: CowSlice::Owned(entries),
        }
    }
}

#[inline]
fn has_duplicates<T: Eq + Hash>(items: &[T]) -> bool {
    let mut set = HashSet::with_capacity(items.len());
    !items.iter().all(|item| set.insert(item))
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
                entries: #entries_tokens
            }
        }
    }
}

impl<K, V> Map<K, V>
where
    K: Bake,
    V: Bake,
{
    /// Serializes the `Map` into a token stream of literal Rust code that reconstructs it.
    /// Used for embedding in generated code.
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        self.bake(&CrateEnv::default())
    }
}

impl<K, V, const N: usize> From<[(K, V); N]> for Map<K, V>
where
    K: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if any keys are duplicated.
    #[inline]
    fn from(entries: [(K, V); N]) -> Self {
        Self::from_vec(Vec::from(entries))
    }
}

impl<K, V> FromIterator<(K, V)> for Map<K, V>
where
    K: Eq + Hash,
{
    /// # Panics
    ///
    /// Panics if any keys are duplicated.
    #[inline]
    fn from_iter<T: IntoIterator<Item = (K, V)>>(iter: T) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

impl<K, V> Map<K, V> {
    /// Returns the key-value entry corresponding to the given key, if present.
    #[inline]
    pub fn get_entry<Q>(&self, key: &Q) -> Option<(&K, &V)>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        let entries = &self.entries;
        let n = entries.len();

        if n <= generate::SCAN_MAX {
            // Linear scanning
            return entries
                .iter()
                .find(|(k, _)| k.borrow() == key)
                .map(|(k, v)| (k, v));
        }

        let disps = &self.displacements;
        let hash = generate::hash(key, self.seed);

        let index = if disps.is_empty() {
            // Direct strategy
            generate::fastrange(hash, n)
        } else {
            // CHD
            let (f1, f2) = generate::split(hash);
            let (d1, d2) = disps[generate::bucket(hash, disps.len())];
            generate::displace(f1, f2, d1, d2) as usize % n
        };

        let (k, v) = &entries[index];

        if k.borrow() == key {
            Some((k, v))
        } else {
            None
        }
    }

    /// Returns a reference to the value corresponding to the given key, if present.
    #[inline]
    pub fn get<Q>(&self, key: &Q) -> Option<&V>
    where
        Q: Hash + Eq + ?Sized,
        K: Borrow<Q>,
    {
        self.get_entry(key).map(|(_, v)| v)
    }
}

impl<Q, K, V> Index<&Q> for Map<K, V>
where
    Q: Hash + Eq + ?Sized,
    K: Borrow<Q>,
{
    type Output = V;

    /// # Panics
    ///
    /// Panics if there is no entry for the given key.
    #[inline]
    fn index(&self, index: &Q) -> &Self::Output {
        self.get(index).expect("no entry found for key")
    }
}

#[cfg(test)]
mod test {
    use super::Map;
    use foldhash::HashSet;

    type Key = u8;

    #[test]
    fn empty() {
        let map = Map::<Key, ()>::from_vec(vec![]);

        for key in Key::MIN..=Key::MAX {
            assert!(map.get_entry(&key).is_none());
            assert!(map.get(&key).is_none());
        }
    }

    #[test]
    fn single() {
        let map = Map::from_vec(vec![(Key::MAX, "foo")]);

        for key in Key::MIN..Key::MAX {
            assert!(map.get_entry(&key).is_none());
            assert!(map.get(&key).is_none());
        }

        assert_eq!(map.get_entry(&Key::MAX), Some((&Key::MAX, &"foo")));
        assert_eq!(map.get(&Key::MAX), Some(&"foo"));
        assert_eq!(map[&Key::MAX], "foo");
    }

    #[test]
    fn multiple() {
        let entries = vec![(1, "foo"), (3, "bar"), (9, "baz")];
        let keys: HashSet<_> = entries.clone().into_iter().map(|(k, _)| k).collect();

        let map = Map::from_vec(entries);

        for key in Key::MIN..=Key::MAX {
            if !keys.contains(&key) {
                assert!(map.get_entry(&key).is_none());
                assert!(map.get(&key).is_none());
            }
        }

        assert_eq!(map.get_entry(&1), Some((&1, &"foo")));
        assert_eq!(map.get(&1), Some(&"foo"));
        assert_eq!(map[&1], "foo");

        assert_eq!(map.get_entry(&3), Some((&3, &"bar")));
        assert_eq!(map.get(&3), Some(&"bar"));
        assert_eq!(map[&3], "bar");

        assert_eq!(map.get_entry(&9), Some((&9, &"baz")));
        assert_eq!(map.get(&9), Some(&"baz"));
        assert_eq!(map[&9], "baz");
    }

    #[test]
    #[should_panic = "no entry found for key"]
    fn panic_index() {
        let map = Map::from_vec(vec![(Key::MAX, "foo")]);

        let _ = map[&0];
    }
}
