//! Codegen support for serializing constructed maps and sets into Rust code.
//! Requires `std` for crate name resolution.

use crate::map::Map;
use crate::set::Set;
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

impl<K, V> Bake for Map<K, V>
where
    K: Bake,
    V: Bake,
{
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);
        let seed = self.seed.bake(ctx);
        let displacements = self.displacements.iter().map(|d| d.bake(ctx));
        let entries = self.entries.iter().map(|e| e.bake(ctx));

        quote! {
            ::#krate::Map::from_baked_parts(
                #seed,
                &[#(#displacements),*],
                &[#(#entries),*],
            )
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

impl<T: Bake> Bake for Set<T> {
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);
        let map = self.map.bake(ctx);

        quote! {
            ::#krate::Set::from_baked_map(#map)
        }
    }
}

impl<T: Bake> Set<T> {
    /// Serializes the `Set` into a token stream of literal Rust code that reconstructs it.
    /// Used for embedding in generated code.
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        self.bake(&CrateEnv::default())
    }
}
