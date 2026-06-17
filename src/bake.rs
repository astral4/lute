//! Codegen support for serializing constructed maps and sets into Rust code.
//! Requires `std` for crate name resolution.

use crate::map::{CowSlice, Map};
use databake::{Bake, CrateEnv, TokenStream, quote};
use proc_macro_crate::{FoundCrate, crate_name};
use std::sync::OnceLock;

/// Registers `haph` with the bake environment under the name the consuming crate imports it as,
/// and returns that name as tokens for emitting paths in generated code.
fn crate_path(ctx: &CrateEnv) -> TokenStream {
    static NAME: OnceLock<&'static str> = OnceLock::new();

    let name = *NAME.get_or_init(|| {
        match crate_name("haph").expect("crate should be included as dependency") {
            FoundCrate::Itself => "haph",
            FoundCrate::Name(name) => name.leak(),
        }
    });

    ctx.insert(name);
    name.parse().unwrap()
}

impl<T: Bake> Bake for CowSlice<T> {
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);
        let tokens = self.iter().map(|d| d.bake(ctx));

        quote! {
            ::#krate::CowSlice::Borrowed(&[#(#tokens),*])
        }
    }
}

impl<K, V> Bake for Map<K, V>
where
    K: Bake,
    V: Bake,
{
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);
        let seed_tokens = self.seed.bake(ctx);
        let displacements_tokens = self.displacements.bake(ctx);
        let entries_tokens = self.entries.bake(ctx);

        quote! {
            ::#krate::Map {
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
