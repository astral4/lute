# lute

[![Crates.io](https://img.shields.io/crates/v/lute)](https://crates.io/crates/lute)
[![Docs.rs](https://img.shields.io/docsrs/lute)](https://docs.rs/lute)
[![License](https://img.shields.io/crates/l/lute)](#license)

`lute` is a Rust library for immutable maps and sets built from fixed collections of up to 65535 (i.e. 2¹⁶ − 1) entries. It is designed with small sizes and use cases like lookup tables in mind. It is usable in a `no_std` environment by default.

Expected construction time is `O(n)`, where $n$ is the number of entries, and worst-case query time is `O(1)`.

See [`BENCHMARKS.md`](https://github.com/astral4/lute/blob/main/BENCHMARKS.md) for performance comparisons to the `phf` crate.

## Usage

```rs
use lute::Map;

let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3)]);
assert_eq!(planets.get("Earth"), Some(&3));
assert_eq!(planets.get("Pluto"), None);
```

```rs
use lute::Set;

let primes = Set::from([2, 3, 5, 7, 11]);
assert!(primes.contains(&7));
assert!(!primes.contains(&8));
```

## Compile-time generation with macros

The `map!` and `set!` macros (enabled by the `macros` feature flag) build maps and sets at compile time. The result is an expression that can be used for a `static` or `const`. See the documentation of each macro for more details.

```rs
use lute::{Map, map};

static PLANETS: Map<&str, i32> = map! {
    "Mercury" => 1,
    "Venus" => 2,
    "Earth" => 3,
};

assert_eq!(PLANETS.get("Earth"), Some(&3));
assert_eq!(PLANETS["Venus"], 2);
```

```rs
use lute::{Set, set};

static PRIMES: Set<u32> = set! { 2u32, 3u32, 5u32, 7u32, 11u32 };

assert!(PRIMES.contains(&7));
assert!(!PRIMES.contains(&8));
```

## Compile-time generation with a build script

For entries that are not supported by the `map!` or `set!` macros, you can also construct maps and sets in a [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html) with the `codegen` feature flag enabled. Here is an example with `Map`:

```toml
[dependencies]
lute = { version = "0.1.0", default-features = false }

[build-dependencies]
lute = { version = "0.1.0", features = ["codegen"] }
```

In `build.rs`, build the map and write it to a file in `OUT_DIR`:

```rs
use lute::Map;
use std::env::var_os;
use std::fs::write;
use std::path::Path;

fn main() {
    let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);

    let code = format!(
        "pub static PLANETS: ::lute::Map<&'static str, i32> = {};",
        planets.to_tokens()
    );

    let path = Path::new(&var_os("OUT_DIR").unwrap()).join("planets.rs");
    write(path, code).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
```

Then include the generated file anywhere in your code:

```rs
include!(concat!(env!("OUT_DIR"), "/planets.rs"));

assert_eq!(PLANETS.get("Earth"), Some(&3));
assert_eq!(PLANETS["Mars"], 4);
```

## Reproducibility and portability

Embedded maps and sets are not necessarily stable across breaking versions and should be regenerated.

Keys must hash identically on the machine that builds the map and the target that runs it. Platform properties that can affect this include:
- Pointer width. Keys whose hash uses `usize` or `isize` either directly or via a length prefix (e.g. arrays, slices, byte strings, C strings) hash differently across targets of different pointer width.
- Endianness. Arrays and slices of multibyte integers (e.g. `[u16; N]`, `&[u32]`) hash their raw native-endian bytes, so they hash differently across targets of different endianness.

Keys must also have consistent [`Hash`](core::hash::Hash) and [`Eq`](core::cmp::Eq): equal keys must hash equally and two keys that are distinct under `Eq` must not hash identically under every seed.

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
