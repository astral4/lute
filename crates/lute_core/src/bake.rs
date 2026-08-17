//! Codegen support for serializing constructed maps and sets into Rust code. Requires `std` for crate name resolution.

use crate::map::Map;
use crate::set::Set;
use databake::{Bake, CrateEnv, TokenStream, quote};
use proc_macro_crate::{FoundCrate, crate_name};

/// Returns the path to the `lute` crate and registers it with the bake environment.
///
/// Generated code defaults to `::lute`. When Cargo can resolve how the consuming crate imports `lute`, that name is used instead.
fn crate_path(ctx: &CrateEnv) -> TokenStream {
    let name: &'static str = match crate_name("lute") {
        Ok(FoundCrate::Name(name)) if name != "lute" => name.leak(),
        _ => "lute",
    };
    ctx.insert(name);
    let path: TokenStream = name.parse().unwrap();
    quote!(::#path)
}

#[cfg(feature = "codegen")]
impl<K, V> Bake for Map<K, V>
where
    K: Bake,
    V: Bake,
{
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);

        let seed = self.seed.bake(ctx);
        let pilots = (&*self.pilots).bake(ctx);
        let remap = (&*self.remap).bake(ctx);
        let entries = (&*self.entries).bake(ctx);

        quote!(#krate::Map::from_baked_parts(#seed, #pilots, #remap, #entries))
    }
}

#[cfg(feature = "codegen")]
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

#[cfg(feature = "codegen")]
impl<T: Bake> Bake for Set<T> {
    fn bake(&self, ctx: &CrateEnv) -> TokenStream {
        let krate = crate_path(ctx);
        let map = self.map.bake(ctx);

        quote!(#krate::Set::from_baked_map(#map))
    }
}

#[cfg(feature = "codegen")]
impl<T: Bake> Set<T> {
    /// Serializes the `Set` into a token stream of literal Rust code that reconstructs it.
    /// Used for embedding in generated code.
    #[must_use]
    pub fn to_tokens(&self) -> TokenStream {
        self.bake(&CrateEnv::default())
    }
}
