include!(concat!(env!("OUT_DIR"), "/baked.rs"));

#[cfg(test)]
mod tests {
    use super::{BIG_MAP, BIG_SET, SMALL_MAP, SMALL_SET};
    use lute::{Map, Set};

    #[test]
    fn small_map_roundtrip() {
        let expected = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
        assert_eq!(SMALL_MAP, expected);
        assert_eq!(SMALL_MAP.get("Earth"), Some(&3));
        assert_eq!(SMALL_MAP.get("Pluto"), None);
        assert_eq!(SMALL_MAP.len(), 4);
    }

    #[test]
    fn small_set_roundtrip() {
        let expected = Set::from([2u32, 3, 5, 7, 11]);
        assert_eq!(SMALL_SET, expected);
        assert!(SMALL_SET.contains(&7));
        assert!(!SMALL_SET.contains(&8));
    }

    #[test]
    fn big_map_roundtrip() {
        let expected: Map<u32, u32> = (0..300u32).map(|i| (i, i * 3)).collect();
        assert_eq!(BIG_MAP, expected);
        assert_eq!(BIG_MAP.len(), 300);
        for i in 0..300u32 {
            assert_eq!(BIG_MAP.get(&i), Some(&(i * 3)));
        }
        for i in 300..800u32 {
            assert_eq!(BIG_MAP.get(&i), None);
        }
    }

    #[test]
    fn big_set_roundtrip() {
        let expected: Set<u16> = (0..150u16).map(|i| i * 7).collect();
        assert_eq!(BIG_SET, expected);
        for i in 0..1200u16 {
            assert_eq!(BIG_SET.contains(&i), i % 7 == 0 && i < 1050);
        }
    }
}
