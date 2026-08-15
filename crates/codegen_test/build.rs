use lute::{Map, Set};
use std::env::var_os;
use std::fs::File;
use std::io::Write as _;
use std::path::Path;

fn main() {
    let small_map = Map::from([("Mercury", 1i32), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
    let small_set = Set::from([2u32, 3, 5, 7, 11]);
    let big_map: Map<_, _> = (0..300u32).map(|i| (i, i * 3)).collect();
    let big_set: Set<_> = (0..150u16).map(|i| i * 7).collect();

    let mut out = File::create(Path::new(&var_os("OUT_DIR").unwrap()).join("baked.rs")).unwrap();
    writeln!(
        out,
        "const SMALL_MAP: ::lute::Map<&'static str, i32> = {};\n\
         const SMALL_SET: ::lute::Set<u32> = {};\n\
         const BIG_MAP: ::lute::Map<u32, u32> = {};\n\
         const BIG_SET: ::lute::Set<u16> = {};",
        small_map.to_tokens(),
        small_set.to_tokens(),
        big_map.to_tokens(),
        big_set.to_tokens()
    )
    .unwrap();

    println!("cargo:rerun-if-changed=build.rs");
}
