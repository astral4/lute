//! # haph
//!
//! Hasher-agnostic static hashmaps

#![cfg_attr(not(test), no_std)]

mod generate;
mod map;

pub use map::{CowSlice, Map};

extern crate alloc;

use databake::TokenStream;
use proc_macro_crate::{crate_name, FoundCrate};

fn get_crate_name() -> (&'static str, TokenStream) {
    let name = match crate_name("haph").expect("crate should be included as dependency") {
        FoundCrate::Itself => "haph",
        FoundCrate::Name(name) => name.leak(),
    };

    (name, name.parse().unwrap())
}
