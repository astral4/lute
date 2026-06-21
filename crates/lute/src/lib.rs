//! # lute
//!
//! This crate provides immutable maps and sets built from fixed collections of up to 65535 (i.e. 2¹⁶ − 1) entries.
//! It is designed with small sizes and use cases like lookup tables in mind. It is usable in a `no_std` environment by default.
//!
//! Expected construction time is `O(n)`, where `n` is the number of entries, and worst-case query time is `O(1)`.
//!
//! ## Feature flags
//!
//! - `construct` (enabled by default): Build maps and sets at runtime.
//! - `macros`: Build maps and sets at compile time with the [`map!`] and [`set!`] macros.
//! - `codegen`: Serialize maps and sets into Rust source code from a build script. Implies `construct` and requires `std`.
//!
//! ## Usage
//!
//! ```
//! # #[cfg(feature = "construct")] {
//! use lute::Map;
//!
//! let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3)]);
//! assert_eq!(planets.get("Earth"), Some(&3));
//! assert_eq!(planets.get("Pluto"), None);
//! # }
//! ```
//!
//! ```
//! # #[cfg(feature = "construct")] {
//! use lute::Set;
//!
//! let primes = Set::from([2, 3, 5, 7, 11]);
//! assert!(primes.contains(&7));
//! assert!(!primes.contains(&8));
//! # }
//! ```
//!
//! ## Compile-time generation with macros
//!
//! The [`map!`] and [`set!`] macros (enabled by the `macros` feature flag) build maps and sets at compile time.
//! The result is an expression that can be used for a `static` or `const`. See the documentation of each macro for more details.
//!
//! ```
//! # #[cfg(feature = "macros")] {
//! use lute::Map;
//!
//! static PLANETS: Map<&str, i32> = lute::map! {
//!     "Mercury" => 1,
//!     "Venus" => 2,
//!     "Earth" => 3,
//! };
//!
//! assert_eq!(PLANETS.get("Earth"), Some(&3));
//! assert_eq!(PLANETS["Venus"], 2);
//! # }
//! ```
//!
//! ```
//! # #[cfg(feature = "macros")] {
//! use lute::Set;
//!
//! static PRIMES: Set<u32> = lute::set! { 2u32, 3u32, 5u32, 7u32, 11u32 };
//!
//! assert!(PRIMES.contains(&7));
//! assert!(!PRIMES.contains(&8));
//! # }
//! ```
//!
//! ## Compile-time generation with a build script
//!
//! For entries that are not supported by the [`map!`] or [`set!`] macros, you can also construct maps and sets
//! in a [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html) with the `codegen` feature flag enabled.
//! Here is an example with [`Map`]:
//!
//! ```toml
//! [dependencies]
//! lute = { version = "0.0.0", default-features = false }
//!
//! [build-dependencies]
//! lute = { version = "0.0.0", features = ["codegen"] }
//! ```
//!
//! In `build.rs`, build the map and write it to a file in `OUT_DIR`:
//!
//! ```ignore
//! use lute::Map;
//! use std::env::var_os;
//! use std::fs::write;
//! use std::path::Path;
//!
//! fn main() {
//!     let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
//!
//!     let code = format!(
//!         "pub static PLANETS: ::lute::Map<&'static str, i32> = {};",
//!         planets.to_tokens()
//!     );
//!
//!     let path = Path::new(&var_os("OUT_DIR").unwrap()).join("planets.rs");
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
//! ### Reproducibility and portability
//!
//! Embedded maps and sets are not necessarily stable across breaking versions and should be regenerated.
//!
//! Keys must hash identically on the machine that builds the map and the target that runs it.
//! Platform properties that can affect this include:
//! - Pointer width. Keys whose hash uses `usize` or `isize` either directly or via a length prefix
//!   (e.g. arrays, slices, byte strings, C strings) hash differently across targets of different pointer width.
//! - Endianness. Arrays and slices of multibyte integers (e.g. `[u16; N]`, `&[u32]`) hash their raw native-endian bytes,
//!   so they hash differently across targets of different endianness.
//!
//! Keys must also have consistent [`Hash`](core::hash::Hash) and [`Eq`](core::cmp::Eq): equal keys must hash equally
//! and two keys that are distinct under `Eq` must not hash identically under every seed.

#![cfg_attr(not(test), no_std)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub use lute_core::{Map, MapEntries, MapKeys, MapValues, Set, SetEntries};

#[cfg(feature = "macros")]
pub use lute_macros::{map, set};
