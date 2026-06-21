//! Core implementation for [`lute`](https://docs.rs/lute).
//!
//! Depend on `lute` rather than this crate directly.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;
#[cfg(feature = "codegen")]
extern crate std;

mod kernel;
mod map;
mod set;

#[cfg(feature = "construct")]
mod construct;

#[cfg(feature = "codegen")]
mod bake;

pub use map::{Map, MapEntries, MapKeys, MapValues};
pub use set::{Set, SetEntries};

#[cfg(feature = "construct")]
#[doc(hidden)]
pub use construct::{MAX_LEN, MapState, construct};
