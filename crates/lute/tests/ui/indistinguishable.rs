fn main() {
    let _: lute::Map<&[u8], i32> = lute::map!(b"hi" => 1, &[104u8, 105u8] => 2);
}
