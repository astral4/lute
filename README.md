# haph

[![Crates.io](https://img.shields.io/crates/v/haph)](https://crates.io/crates/haph)
[![Docs.rs](https://img.shields.io/docsrs/haph)](https://docs.rs/haph)
[![License](https://img.shields.io/crates/l/haph)](#license)

`haph` is a Rust library for **h**asher-**a**gnostic **p**erfect **h**ash function-based hashmaps.
It is intended to replace some use cases of [`phf`](https://crates.io/crates/phf) while having a more flexible API.

## Usage

```rust
use haph::Map;

// Build a map from a set of entries.
let planets = Map::from([
    ("Mercury", 1),
    ("Venus", 2),
    ("Earth", 3),
    ("Mars", 4),
]);

// Look up a value by key.
assert_eq!(planets.get("Earth"), Some(&3));
assert_eq!(planets.get("Pluto"), None);

// Index directly when the key is known to be present.
assert_eq!(planets["Mars"], 4);

// Get the key and value at the same time.
assert_eq!(planets.get_entry("Venus"), Some((&"Venus", &2)));
```

A `Map` can be built from any iterator of key-value pairs.

## Generating a map at build time

Computing a perfect hash function takes work, so when the entries are known ahead of time it is often best to do it once at build time and embed the constructed map directly in your binary. `Map::to_tokens` serializes a map into literal Rust code (backed by `&'static` data) that reconstructs it without allocating at runtime.

This is usually done from a [build script](https://doc.rust-lang.org/cargo/reference/build-scripts.html), so `haph` is both a build dependency and a regular dependency:

```toml
[dependencies]
haph = "0.0.0"

[build-dependencies]
haph = "0.0.0"
```

In `build.rs`, build the map and write it to a file in `OUT_DIR`:

```rust
use haph::Map;
use std::env::var;
use std::fs::write;
use std::path::Path;

fn main() {
    let planets = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);

    let code = format!(
        "pub static PLANETS: ::haph::Map<&'static str, i32> = {};",
        planets.to_tokens()
    );

    let path = Path::new(&var("OUT_DIR").unwrap()).join("planets.rs");
    write(path, code).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
```

Then include the generated file anywhere in your code:

```rust
include!(concat!(env!("OUT_DIR"), "/planets.rs"));

assert_eq!(PLANETS.get("Earth"), Some(&3));
assert_eq!(PLANETS["Mars"], 4);
```

## License

Licensed under either of

- Apache License, Version 2.0 ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
- MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.
