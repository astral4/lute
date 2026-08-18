//! Core implementation for [`lute`](https://docs.rs/lute).
//!
//! Depend on `lute` rather than this crate directly.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

extern crate alloc;
#[cfg(feature = "codegen")]
extern crate std;

mod cow;
mod iter;
mod kernel;
mod map;
mod set;
mod strategy;

#[cfg(feature = "construct")]
mod construct;

#[cfg(feature = "codegen")]
mod bake;

pub use iter::{MapEntries, MapKeys, MapValues, SetEntries};
pub use map::Map;
pub use set::Set;

#[doc(hidden)]
pub use kernel::MAX_LEN;
#[doc(hidden)]
pub use strategy::BakedStrategy;

#[cfg(feature = "construct")]
#[doc(hidden)]
pub use construct::{ConstructError, MapState, Strategy, construct};
