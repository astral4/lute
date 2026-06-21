include!(concat!(env!("OUT_DIR"), "/baked.rs"));

#[cfg(test)]
mod tests {
    use super::{BAKED_MAP, BAKED_SET};
    use lute::{Map, Set};

    #[test]
    fn map_roundtrip() {
        let expected = Map::from([("Mercury", 1), ("Venus", 2), ("Earth", 3), ("Mars", 4)]);
        assert_eq!(BAKED_MAP, expected);
        assert_eq!(BAKED_MAP.get("Earth"), Some(&3));
        assert_eq!(BAKED_MAP.get("Pluto"), None);
        assert_eq!(BAKED_MAP.len(), 4);
    }

    #[test]
    fn set_roundtrip() {
        let expected = Set::from([2u32, 3, 5, 7, 11]);
        assert_eq!(BAKED_SET, expected);
        assert!(BAKED_SET.contains(&7));
        assert!(!BAKED_SET.contains(&8));
    }
}
