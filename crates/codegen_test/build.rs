use lute::{Map, Set};
use std::env::var_os;
use std::fs::write;
use std::path::Path;

fn main() {
    let map = Map::from([("Mercury", 1i32), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
    let set = Set::from([2u32, 3, 5, 7, 11]);

    let code = format!(
        "pub static BAKED_MAP: ::lute::Map<&'static str, i32> = {};\n\
         pub static BAKED_SET: ::lute::Set<u32> = {};\n",
        map.to_tokens(),
        set.to_tokens(),
    );

    let out = Path::new(&var_os("OUT_DIR").unwrap()).join("baked.rs");
    write(&out, code).unwrap();
    println!("cargo:rerun-if-changed=build.rs");
}
