#![cfg(feature = "macros")]

use lute::{Map, Set};

static PLANETS: Map<&str, i32> = lute::map! {
    "Mercury" => 1,
    "Venus" => 2,
    "Earth" => 3,
    "Mars" => 4,
};

#[test]
fn map_str_keys() {
    assert_eq!(PLANETS.len(), 4);
    assert_eq!(PLANETS.get("Earth"), Some(&3));
    assert_eq!(PLANETS["Mars"], 4);
    assert_eq!(PLANETS.get("Pluto"), None);
}

static CODES: Map<u32, &str> = lute::map! {
    200u32 => "ok",
    404u32 => "not found",
    500u32 => "error",
};

#[test]
fn map_int_keys() {
    assert_eq!(CODES.get(&404), Some(&"not found"));
    assert_eq!(CODES.get(&200), Some(&"ok"));
    assert_eq!(CODES.get(&418), None);
}

static OFFSETS: Map<i32, &str> = lute::map! {
    -1i32 => "before",
    0i32 => "here",
    1i32 => "after",
};

#[test]
fn map_negative_keys() {
    assert_eq!(OFFSETS.get(&-1), Some(&"before"));
    assert_eq!(OFFSETS.get(&0), Some(&"here"));
    assert_eq!(OFFSETS.get(&1), Some(&"after"));
    assert_eq!(OFFSETS.get(&2), None);
}

static FLAGS: Map<bool, &str> = lute::map! {
    true => "yes",
    false => "no",
};

#[test]
fn map_bool_keys() {
    assert_eq!(FLAGS[&true], "yes");
    assert_eq!(FLAGS[&false], "no");
}

static EMPTY: Map<&str, i32> = lute::map! {};

#[test]
fn map_empty() {
    assert!(EMPTY.is_empty());
    assert_eq!(EMPTY.get("anything"), None);
}

static BYTES: Map<&[u8], i32> = lute::map! {
    b"a" => 1,
    b"bb" => 2,
    b"ccc" => 3,
};

#[test]
fn map_byte_string_keys() {
    assert_eq!(BYTES.get("a".as_bytes()), Some(&1));
    assert_eq!(BYTES.get(b"bb".as_slice()), Some(&2));
    assert_eq!(BYTES.get("ccc".as_bytes()), Some(&3));
    assert_eq!(BYTES.get(b"zzz".as_slice()), None);
}

static VOWELS: Set<char> = lute::set! { 'a', 'e', 'i', 'o', 'u' };

#[test]
fn set_char_elements() {
    assert_eq!(VOWELS.len(), 5);
    assert!(VOWELS.contains(&'e'));
    assert!(!VOWELS.contains(&'z'));
}

static PRIMES: Set<u64> = lute::set! { 2u64, 3u64, 5u64, 7u64, 11u64, 13u64 };

#[test]
fn set_int_elements() {
    assert!(PRIMES.contains(&7));
    assert!(!PRIMES.contains(&8));
}

static MANY: Map<u32, u32> = lute::map! {
    0u32 => 0u32, 1u32 => 10u32, 2u32 => 20u32, 3u32 => 30u32, 4u32 => 40u32,
    5u32 => 50u32, 6u32 => 60u32, 7u32 => 70u32, 8u32 => 80u32, 9u32 => 90u32,
    10u32 => 100u32, 11u32 => 110u32, 12u32 => 120u32, 13u32 => 130u32, 14u32 => 140u32,
    15u32 => 150u32, 16u32 => 160u32, 17u32 => 170u32, 18u32 => 180u32, 19u32 => 190u32,
};

#[test]
fn map_many_entries() {
    for k in 0u32..20 {
        assert_eq!(MANY.get(&k), Some(&(k * 10)));
    }
    assert_eq!(MANY.get(&20), None);
}

static POINTS: Map<(i16, i16), &str> = lute::map! {
    (0i16, 0i16) => "origin",
    (1i16, 0i16) => "east",
    (0i16, 1i16) => "north",
    (-1i16, -1i16) => "southwest",
};

#[test]
fn map_tuple_keys() {
    assert_eq!(POINTS.get(&(0, 0)), Some(&"origin"));
    assert_eq!(POINTS.get(&(-1, -1)), Some(&"southwest"));
    assert_eq!(POINTS.get(&(5, 5)), None);
}

static PAIRS: Set<(&str, u8)> = lute::set! {
    ("a", 1u8),
    ("a", 2u8),
    ("b", 1u8),
};

#[test]
fn set_tuple_elements() {
    assert!(PAIRS.contains(&("a", 1)));
    assert!(PAIRS.contains(&("b", 1)));
    assert!(!PAIRS.contains(&("b", 2)));
}

static IDS: Map<[u8; 2], &str> = lute::map! {
    [1u8, 2u8] => "a",
    [3u8, 4u8] => "b",
    [5u8, 6u8] => "c",
};

#[test]
fn map_array_keys() {
    assert_eq!(IDS.get(&[3u8, 4]), Some(&"b"));
    assert_eq!(IDS.get(&[5u8, 6]), Some(&"c"));
    assert_eq!(IDS.get(&[9u8, 9]), None);
}

static SLICES: Map<&[u32], &str> = lute::map! {
    &[1u32] => "one",
    &[1u32, 2u32] => "one-two",
    &[9u32, 9u32, 9u32] => "nines",
};

#[test]
fn map_slice_keys() {
    assert_eq!(SLICES.get([1u32].as_slice()), Some(&"one"));
    assert_eq!(SLICES.get([1u32, 2].as_slice()), Some(&"one-two"));
    assert_eq!(SLICES.get([9u32, 9, 9].as_slice()), Some(&"nines"));
    assert_eq!(SLICES.get([0u32].as_slice()), None);
}

static SPANS: Map<core::ops::Range<u16>, &str> = lute::map! {
    0u16..10u16 => "low",
    10u16..20u16 => "mid",
    20u16..30u16 => "high",
};

#[test]
fn map_range_keys() {
    assert_eq!(SPANS.get(&(10u16..20)), Some(&"mid"));
    assert_eq!(SPANS.get(&(0u16..10)), Some(&"low"));
    assert_eq!(SPANS.get(&(5u16..15)), None);
}

static GRADES: Map<core::ops::RangeInclusive<u8>, char> = lute::map! {
    90u8..=100u8 => 'A',
    80u8..=89u8 => 'B',
    70u8..=79u8 => 'C',
};

#[test]
fn map_inclusive_range_keys() {
    assert_eq!(GRADES.get(&(90u8..=100)), Some(&'A'));
    assert_eq!(GRADES.get(&(80u8..=89)), Some(&'B'));
    assert_eq!(GRADES.get(&(0u8..=10)), None);
}

static LETTER_BANDS: Set<core::ops::Range<char>> = lute::set! { 'a'..'e', 'm'..'q' };

#[test]
fn set_char_range_elements() {
    assert!(LETTER_BANDS.contains(&('a'..'e')));
    assert!(!LETTER_BANDS.contains(&('a'..'f')));
}

static CSTRS: Map<&core::ffi::CStr, i32> = lute::map! {
    c"alpha" => 1,
    c"beta" => 2,
};

#[test]
fn map_cstr_keys() {
    assert_eq!(CSTRS.get(c"alpha"), Some(&1));
    assert_eq!(CSTRS.get(c"beta"), Some(&2));
    assert_eq!(CSTRS.get(c"gamma"), None);
}

static UNIT: Map<(), i32> = lute::map! { () => 42 };

#[test]
fn map_unit_key() {
    assert_eq!(UNIT.len(), 1);
    assert_eq!(UNIT.get(&()), Some(&42));
}

static NESTED: Map<([u8; 2], u16), &str> = lute::map! {
    ([1u8, 2u8], 100u16) => "x",
    ([3u8, 4u8], 200u16) => "y",
};

#[test]
fn map_nested_keys() {
    assert_eq!(NESTED.get(&([1u8, 2], 100)), Some(&"x"));
    assert_eq!(NESTED.get(&([3u8, 4], 200)), Some(&"y"));
    assert_eq!(NESTED.get(&([0u8, 0], 0)), None);
}

static EXTREMES: Map<i32, &str> = lute::map! {
    -2_147_483_648i32 => "min",
    0i32 => "zero",
    2_147_483_647i32 => "max",
};

#[test]
fn map_extreme_int_keys() {
    assert_eq!(EXTREMES.get(&i32::MIN), Some(&"min"));
    assert_eq!(EXTREMES.get(&i32::MAX), Some(&"max"));
    assert_eq!(EXTREMES.get(&0), Some(&"zero"));
    assert_eq!(EXTREMES.get(&1), None);
}

static REPEATED: Map<[u8; 4], &str> = lute::map! {
    [0u8; 4] => "zeros",
    [1u8; 4] => "ones",
    [9u8; 4] => "nines",
};

#[test]
fn map_repeat_array_keys() {
    assert_eq!(REPEATED.get(&[0u8; 4]), Some(&"zeros"));
    assert_eq!(REPEATED.get(&[1u8, 1, 1, 1]), Some(&"ones"));
    assert_eq!(REPEATED.get(&[9u8; 4]), Some(&"nines"));
    assert_eq!(REPEATED.get(&[2u8; 4]), None);
}

static EMPTY_ARRAYS: Map<([u8; 0], u16), &str> = lute::map! {
    ([0u8; 0], 1u16) => "one",
    ([0u8; 0], 2u16) => "two",
    ([0u8; 0], 3u16) => "three",
};

#[test]
fn map_empty_int_array_keys() {
    assert_eq!(EMPTY_ARRAYS.get(&([0u8; 0], 2u16)), Some(&"two"));
    assert_eq!(EMPTY_ARRAYS.get(&([0u8; 0], 9u16)), None);
}

#[cfg(feature = "construct")]
#[test]
fn macro_matches_runtime() {
    macro_rules! same {
        ($macro_built:expr, $runtime:expr) => {{
            let runtime = $runtime;
            assert_eq!($macro_built, runtime);
            assert_eq!(runtime, $macro_built);
        }};
    }

    same!(
        PLANETS,
        Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)])
    );
    same!(
        POINTS,
        Map::from([
            ((0i16, 0i16), "origin"),
            ((1, 0), "east"),
            ((0, 1), "north"),
            ((-1, -1), "southwest"),
        ])
    );
    same!(
        IDS,
        Map::from([([1u8, 2], "a"), ([3u8, 4], "b"), ([5u8, 6], "c")])
    );
    same!(
        SPANS,
        Map::from([(0u16..10, "low"), (10u16..20, "mid"), (20u16..30, "high")])
    );
    same!(
        GRADES,
        Map::from([(90u8..=100, 'A'), (80u8..=89, 'B'), (70u8..=79, 'C')])
    );
    same!(
        NESTED,
        Map::from([(([1u8, 2], 100u16), "x"), (([3u8, 4], 200u16), "y")])
    );

    same!(
        EXTREMES,
        Map::from([(i32::MIN, "min"), (0, "zero"), (i32::MAX, "max")])
    );

    same!(
        REPEATED,
        Map::from([([0u8; 4], "zeros"), ([1u8; 4], "ones"), ([9u8; 4], "nines")])
    );
    same!(
        EMPTY_ARRAYS,
        Map::from([
            (([0u8; 0], 1u16), "one"),
            (([0u8; 0], 2u16), "two"),
            (([0u8; 0], 3u16), "three"),
        ])
    );

    same!(
        MANY,
        (0u32..20).map(|k| (k, k * 10)).collect::<Map<u32, u32>>()
    );
    same!(
        PRIMES,
        [2u64, 3, 5, 7, 11, 13].into_iter().collect::<Set<u64>>()
    );
}
