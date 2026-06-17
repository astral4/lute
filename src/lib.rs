//! # haph
//!
//! This crate provides immutable maps and sets built from fixed collections of up to 65535 (i.e. 2¹⁶ − 1) entries.
//! It is optimized for small sizes and use cases like lookup tables. It is usable in a `no_std` environment by default.
//!
//! ## Feature flags
//!
//! - `construct` (enabled by default): Build maps and sets at runtime.
//! - `codegen`: Serialize maps and sets into Rust source code. Implies `construct` and requires `std`.
//!
//! ## Usage
//!
//! ```
//! use haph::Map;
//!
//! let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3)]);
//! assert_eq!(planets.get("Earth"), Some(&3));
//! assert_eq!(planets.get("Pluto"), None);
//! ```
//!
//! ```
//! use haph::Set;
//!
//! let primes = Set::from([2, 3, 5, 7, 11]);
//! assert!(primes.contains(&7));
//! assert!(!primes.contains(&8));
//! ```
//!
//! ## Compile-time generation
//!
//! When the entries are known ahead of time, it is often best to build a map or set once at compile time and directly embed it.
//! This can be done from a [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html)
//! with the `codegen` feature flag enabled. Here is an example with [`Map`]:
//!
//! ```toml
//! [dependencies]
//! haph = { version = "0.0.0", default-features = false }
//!
//! [build-dependencies]
//! haph = { version = "0.0.0", features = ["codegen"] }
//! ```
//!
//! In `build.rs`, build the map and write it to a file in `OUT_DIR`:
//!
//! ```ignore
//! use haph::Map;
//! use std::env::var;
//! use std::fs::write;
//! use std::path::Path;
//!
//! fn main() {
//!     let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
//!
//!     let code = format!(
//!         "pub static PLANETS: ::haph::Map<&'static str, i32> = {};",
//!         planets.to_tokens()
//!     );
//!
//!     let path = Path::new(&var("OUT_DIR").unwrap()).join("planets.rs");
//!     write(path, code).unwrap();
//!     println!("cargo:rerun-if-changed=build.rs");
//! }
//! ```
//!
//! Then include the generated file anywhere in your code:
//!
//! ```ignore
//! include!(concat!(env!("OUT_DIR"), "/planets.rs"));
//!
//! assert_eq!(PLANETS.get("Earth"), Some(&3));
//! assert_eq!(PLANETS["Mars"], 4);
//! ```
//!
//! ## Reproducibility and portability
//!
//! Embedded maps and sets are not necessarily stable across breaking versions and should be regenerated.
//!
//! Keys must hash identically on the machine that builds the map and the target that runs it.
//!
//! Keys must also have consistent [`Hash`](core::hash::Hash) and [`Eq`](core::cmp::Eq): equal keys must hash equally
//! and two keys that are distinct under `Eq` must not hash identically under every seed.

#![cfg_attr(not(test), no_std)]

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

pub use map::{Entries as MapEntries, Keys, Map, Values};
pub use set::{Entries as SetEntries, Set};
