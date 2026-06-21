//! Procedural macros for the [`lute`](https://docs.rs/lute) crate.
//!
//! These are re-exported by `lute` behind its `macros` feature. Depend on `lute` rather than this crate directly.

use lute_core::{MAX_LEN, MapState, construct};
use proc_macro::TokenStream;
use proc_macro_crate::{FoundCrate, crate_name};
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::quote;
use std::collections::HashMap;
use std::ffi::CString;
use std::hash::{Hash, Hasher};
use std::mem::discriminant;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    Error, Expr, ExprLit, ExprRange, Ident, Lit, LitInt, RangeLimits, Result as SynResult, Token,
    UnOp, parse_macro_input,
};

/// An integer key's bit pattern, held as an unsigned value of the same width.
///
/// Signedness doesn't affect an integer's hash; only the bit width.
/// The default `Hasher::write_iN` methods delegate to `Hasher::write_uN` and `foldhash` doesn't override them,
/// so an `iN` and the `uN` with the same bits hash identically. Therefore, one unsigned variant per width is enough.
#[derive(Clone, Debug, PartialEq, Eq)]
enum IntBits {
    W8(u8),
    W16(u16),
    W32(u32),
    W64(u64),
    W128(u128),
}

/// A key parsed from a literal expression, hashable identically to the runtime key it represents.
#[derive(Clone, Debug, PartialEq, Eq)]
enum HashKey {
    Str(String),
    Bytes(Vec<u8>),
    CStr(CString),
    Char(char),
    Bool(bool),
    Int(IntBits),
    Tuple(Vec<HashKey>),
    Seq(Vec<HashKey>),
    Range {
        inclusive: bool,
        start: Option<Box<HashKey>>,
        end: Option<Box<HashKey>>,
    },
}

impl Hash for HashKey {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match self {
            HashKey::Str(s) => s.as_str().hash(state),
            // `[u8; N]`, `&[u8; N]`, `&[u8]`, and `Vec<u8>` all hash as a slice (length prefix plus bytes),
            // so hashing the slice matches whatever byte container the key type uses.
            HashKey::Bytes(b) => b.as_slice().hash(state),
            HashKey::CStr(c) => c.as_c_str().hash(state),
            HashKey::Char(c) => c.hash(state),
            HashKey::Bool(b) => b.hash(state),
            HashKey::Int(int) => match int {
                IntBits::W8(v) => v.hash(state),
                IntBits::W16(v) => v.hash(state),
                IntBits::W32(v) => v.hash(state),
                IntBits::W64(v) => v.hash(state),
                IntBits::W128(v) => v.hash(state),
            },
            HashKey::Tuple(items) => {
                for item in items {
                    item.hash(state);
                }
            }
            HashKey::Seq(items) => hash_seq(items, state),
            HashKey::Range {
                inclusive,
                start,
                end,
            } => hash_range(*inclusive, start.as_deref(), end.as_deref(), state),
        }
    }
}

/// Reproduces `Hash` for an array or slice of keys.
fn hash_seq<H: Hasher>(items: &[HashKey], state: &mut H) {
    // Slices of integers are bulk-hashed as a length prefix followed by the raw contiguous bytes.
    // For these, we reconstruct a real slice of the right width so `std`'s encoding is reused.
    // Other element types are hashed element by element after a length prefix.
    if let Some(HashKey::Int(first)) = items.first() {
        let width = discriminant(first);
        if items
            .iter()
            .all(|k| matches!(k, HashKey::Int(int) if discriminant(int) == width))
        {
            macro_rules! bulk {
                ($variant:ident, $t:ty) => {{
                    let values: Vec<$t> = items
                        .iter()
                        .map(|k| match k {
                            HashKey::Int(IntBits::$variant(v)) => *v,
                            _ => unreachable!(),
                        })
                        .collect();
                    values.as_slice().hash(state);
                }};
            }
            match first {
                IntBits::W8(_) => bulk!(W8, u8),
                IntBits::W16(_) => bulk!(W16, u16),
                IntBits::W32(_) => bulk!(W32, u32),
                IntBits::W64(_) => bulk!(W64, u64),
                IntBits::W128(_) => bulk!(W128, u128),
            }
            return;
        }
    }

    state.write_usize(items.len());
    for item in items {
        item.hash(state);
    }
}

/// Reproduces `Hash` for a range.
fn hash_range<H: Hasher>(
    inclusive: bool,
    start: Option<&HashKey>,
    end: Option<&HashKey>,
    state: &mut H,
) {
    match start.or(end) {
        None => (..).hash(state),
        Some(HashKey::Int(_)) => hash_int_range(inclusive, start, end, state),
        Some(HashKey::Char(_)) => hash_char_range(inclusive, start, end, state),
        _ => unreachable!("range bounds are validated to be integers or chars"),
    }
}

macro_rules! hash_reconstructed_range {
    ($inclusive:expr, $start:expr, $end:expr, $state:expr) => {
        match ($inclusive, $start, $end) {
            (false, Some(s), Some(e)) => (s..e).hash($state),
            (false, Some(s), None) => (s..).hash($state),
            (false, None, Some(e)) => (..e).hash($state),
            (true, Some(s), Some(e)) => (s..=e).hash($state),
            (true, None, Some(e)) => (..=e).hash($state),
            _ => unreachable!("validated range bound combination"),
        }
    };
}

fn hash_int_range<H: Hasher>(
    inclusive: bool,
    start: Option<&HashKey>,
    end: Option<&HashKey>,
    state: &mut H,
) {
    macro_rules! reconstruct {
        ($variant:ident) => {{
            let extract = |bound: Option<&HashKey>| {
                bound.map(|k| match k {
                    HashKey::Int(IntBits::$variant(v)) => *v,
                    _ => unreachable!(),
                })
            };
            hash_reconstructed_range!(inclusive, extract(start), extract(end), state)
        }};
    }
    match start.or(end) {
        Some(HashKey::Int(IntBits::W8(_))) => reconstruct!(W8),
        Some(HashKey::Int(IntBits::W16(_))) => reconstruct!(W16),
        Some(HashKey::Int(IntBits::W32(_))) => reconstruct!(W32),
        Some(HashKey::Int(IntBits::W64(_))) => reconstruct!(W64),
        Some(HashKey::Int(IntBits::W128(_))) => reconstruct!(W128),
        _ => unreachable!("range bounds are validated to be ints"),
    }
}

fn hash_char_range<H: Hasher>(
    inclusive: bool,
    start: Option<&HashKey>,
    end: Option<&HashKey>,
    state: &mut H,
) {
    let ch = |bound: Option<&HashKey>| {
        bound.map(|k| match k {
            HashKey::Char(c) => *c,
            _ => unreachable!(),
        })
    };
    hash_reconstructed_range!(inclusive, ch(start), ch(end), state);
}

/// Interprets a key expression as a value hashable identically to the corresponding runtime key.
fn expr_to_hash_key(expr: &Expr) -> SynResult<HashKey> {
    match expr {
        Expr::Lit(lit) => lit_to_hash_key(&lit.lit),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => negated_int(&unary.expr),
        Expr::Tuple(tuple) => Ok(HashKey::Tuple(
            tuple
                .elems
                .iter()
                .map(expr_to_hash_key)
                .collect::<SynResult<_>>()?,
        )),
        // `[]` and `&[]` don't contain an element to reveal its type, yet the element type decides the hash:
        // an empty integer slice does `write_usize(0)` then `write(&[])`, while an empty non-integer slice
        // just does `write_usize(0)` (and `write(&[])` is not a no-op in `foldhash`).
        Expr::Array(array) if array.elems.is_empty() => Err(Error::new_spanned(
            expr,
            "empty array/slice keys need their element type: write `[T; 0]` (e.g. `[0u8; 0]`)",
        )),
        Expr::Array(array) => Ok(HashKey::Seq(
            array
                .elems
                .iter()
                .map(expr_to_hash_key)
                .collect::<SynResult<_>>()?,
        )),
        Expr::Repeat(repeat) => {
            let count = repeat_count(&repeat.len)?;
            let element = expr_to_hash_key(&repeat.expr)?;
            Ok(if count == 0 {
                // `[T; 0]` is empty but, unlike `[]`, reveals `T`. See the above comments for `Expr::Array` and in `hash_seq`.
                if matches!(element, HashKey::Int(_)) {
                    HashKey::Bytes(Vec::new())
                } else {
                    HashKey::Seq(Vec::new())
                }
            } else {
                HashKey::Seq(vec![element; count])
            })
        }
        Expr::Range(range) => range_to_hash_key(range),
        Expr::Paren(paren) => expr_to_hash_key(&paren.expr),
        Expr::Group(group) => expr_to_hash_key(&group.expr),
        Expr::Reference(reference) => expr_to_hash_key(&reference.expr),
        Expr::Closure(_) => Err(Error::new_spanned(expr, "closures cannot be keys")),
        Expr::Path(_) => Err(Error::new_spanned(
            expr,
            "named constants, paths, and enum variants cannot be keys: the macro only sees tokens and \
             cannot evaluate them; use a literal value, or a build script for non-literal keys",
        )),
        Expr::Cast(_) => Err(Error::new_spanned(
            expr,
            "cast expressions cannot be keys; write the literal with a type suffix instead, e.g. `1u32`",
        )),
        _ => Err(Error::new_spanned(
            expr,
            "unsupported key; expected a literal, tuple, array, or range of supported keys",
        )),
    }
}

/// Interprets a range expression. Bounds must be integer or char keys of one shared type.
fn range_to_hash_key(range: &ExprRange) -> SynResult<HashKey> {
    let inclusive = matches!(range.limits, RangeLimits::Closed(_));
    let start = range.start.as_deref().map(expr_to_hash_key).transpose()?;
    let end = range.end.as_deref().map(expr_to_hash_key).transpose()?;

    for bound in [start.as_ref(), end.as_ref()].into_iter().flatten() {
        if !matches!(bound, HashKey::Int(_) | HashKey::Char(_)) {
            return Err(Error::new_spanned(
                range,
                "range keys must have integer or character bounds",
            ));
        }
    }
    if let (Some(s), Some(e)) = (&start, &end) {
        let same = match (s, e) {
            (HashKey::Int(a), HashKey::Int(b)) => discriminant(a) == discriminant(b),
            (HashKey::Char(_), HashKey::Char(_)) => true,
            _ => false,
        };
        if !same {
            return Err(Error::new_spanned(
                range,
                "range bounds must have the same type",
            ));
        }
    }

    Ok(HashKey::Range {
        inclusive,
        start: start.map(Box::new),
        end: end.map(Box::new),
    })
}

/// Interprets a literal key.
fn lit_to_hash_key(lit: &Lit) -> SynResult<HashKey> {
    match lit {
        Lit::Str(s) => Ok(HashKey::Str(s.value())),
        Lit::ByteStr(s) => Ok(HashKey::Bytes(s.value())),
        Lit::CStr(s) => Ok(HashKey::CStr(s.value())),
        Lit::Char(c) => Ok(HashKey::Char(c.value())),
        Lit::Bool(b) => Ok(HashKey::Bool(b.value)),
        Lit::Byte(b) => Ok(HashKey::Int(IntBits::W8(b.value()))),
        Lit::Int(int) => {
            let (signed, width) = int_type(int)?;
            let bits =
                positive_bits(int.base10_parse::<u128>()?, signed, width).ok_or_else(|| {
                    Error::new_spanned(int, "integer key is out of range for its type")
                })?;
            Ok(HashKey::Int(int_bits(width, bits)))
        }
        Lit::Float(_) => Err(Error::new_spanned(
            lit,
            "floating-point values cannot be keys",
        )),
        _ => Err(Error::new_spanned(lit, "unsupported key literal")),
    }
}

fn strip_groups(mut expr: &Expr) -> &Expr {
    loop {
        match expr {
            Expr::Paren(paren) => expr = &paren.expr,
            Expr::Group(group) => expr = &group.expr,
            _ => return expr,
        }
    }
}

/// Parses an array-repeat count `N` in `[T; N]`.
fn repeat_count(len: &Expr) -> SynResult<usize> {
    match strip_groups(len) {
        Expr::Lit(ExprLit {
            lit: Lit::Int(int), ..
        }) => int.base10_parse::<usize>(),
        Expr::Unary(unary) if matches!(unary.op, UnOp::Neg(_)) => Err(Error::new_spanned(
            len,
            "array repeat count cannot be negative",
        )),
        other => Err(Error::new_spanned(
            other,
            "array repeat count must be an integer literal (the macro cannot evaluate a named constant)",
        )),
    }
}

/// Interprets `-<integer literal>`.
fn negated_int(operand: &Expr) -> SynResult<HashKey> {
    let Expr::Lit(ExprLit {
        lit: Lit::Int(int), ..
    }) = strip_groups(operand)
    else {
        return Err(Error::new_spanned(
            operand,
            "only integer keys can be negated",
        ));
    };
    let (signed, width) = int_type(int)?;
    if !signed {
        return Err(Error::new_spanned(
            int,
            "cannot negate an unsigned integer key",
        ));
    }
    let bits = negative_bits(int.base10_parse::<u128>()?, width)
        .ok_or_else(|| Error::new_spanned(int, "integer key is out of range for its type"))?;
    Ok(HashKey::Int(int_bits(width, bits)))
}

/// Returns whether an integer literal's suffix is signed, and its width in bytes.
fn int_type(int: &LitInt) -> SynResult<(bool, u8)> {
    let ptr_width = (usize::BITS / 8)
        .try_into()
        .expect("pointer width in bytes fits in u8");

    let info = match int.suffix() {
        "" => {
            return Err(Error::new_spanned(
                int,
                "integer keys must have a type suffix so their width is known, e.g. `1u32`",
            ));
        }
        "u8" => (false, 1),
        "u16" => (false, 2),
        "u32" => (false, 4),
        "u64" => (false, 8),
        "u128" => (false, 16),
        "usize" => (false, ptr_width),
        "i8" => (true, 1),
        "i16" => (true, 2),
        "i32" => (true, 4),
        "i64" => (true, 8),
        "i128" => (true, 16),
        "isize" => (true, ptr_width),
        other => {
            return Err(Error::new_spanned(
                int,
                format!("`{other}` is not a valid integer type for a key"),
            ));
        }
    };
    Ok(info)
}

/// The two's-complement bit pattern of `mag` in `width` bytes, or `None` if it does not fit the type.
fn positive_bits(mag: u128, signed: bool, width: u8) -> Option<u128> {
    let bits = u32::from(width) * 8;
    let max = if signed {
        (1u128 << (bits - 1)) - 1
    } else if width >= 16 {
        u128::MAX
    } else {
        (1u128 << bits) - 1
    };
    (mag <= max).then_some(mag)
}

/// The two's-complement bit pattern of `-mag` in `width` bytes, or `None` if it does not fit the type.
fn negative_bits(mag: u128, width: u8) -> Option<u128> {
    let bits = u32::from(width) * 8;
    let min_magnitude = 1u128 << (bits - 1);
    if mag > min_magnitude {
        return None;
    }
    Some(mask(0u128.wrapping_sub(mag), width))
}

/// Keeps only the low `width` bytes of `value`.
fn mask(value: u128, width: u8) -> u128 {
    if width >= 16 {
        value
    } else {
        value & ((1u128 << (u32::from(width) * 8)) - 1)
    }
}

/// Narrows a validated bit pattern into the matching `IntBits` variant.
#[expect(
    clippy::cast_possible_truncation,
    reason = "the value is already validated to fit `width` bytes"
)]
fn int_bits(width: u8, bits: u128) -> IntBits {
    match width {
        1 => IntBits::W8(bits as u8),
        2 => IntBits::W16(bits as u16),
        4 => IntBits::W32(bits as u32),
        8 => IntBits::W64(bits as u64),
        16 => IntBits::W128(bits),
        _ => unreachable!(),
    }
}

/// A `Hasher` that records the exact sequence of write operations.
struct Recorder(Vec<u8>);

impl Hasher for Recorder {
    fn finish(&self) -> u64 {
        0
    }
    fn write(&mut self, bytes: &[u8]) {
        self.0.push(0);
        self.0
            .extend_from_slice(&(bytes.len() as u64).to_le_bytes());
        self.0.extend_from_slice(bytes);
    }
    fn write_u8(&mut self, i: u8) {
        self.0.push(1);
        self.0.push(i);
    }
    fn write_u16(&mut self, i: u16) {
        self.0.push(2);
        self.0.extend_from_slice(&i.to_le_bytes());
    }
    fn write_u32(&mut self, i: u32) {
        self.0.push(3);
        self.0.extend_from_slice(&i.to_le_bytes());
    }
    fn write_u64(&mut self, i: u64) {
        self.0.push(4);
        self.0.extend_from_slice(&i.to_le_bytes());
    }
    fn write_u128(&mut self, i: u128) {
        self.0.push(5);
        self.0.extend_from_slice(&i.to_le_bytes());
    }
    fn write_usize(&mut self, i: usize) {
        self.0.push(6);
        self.0.extend_from_slice(&(i as u64).to_le_bytes());
    }
}

/// Builds an error that points at both a key and the earlier entry it cannot be distinguished from.
fn indistinguishable_error(later: &Expr, earlier: &Expr) -> Error {
    let mut error =
        Error::new_spanned(later, "this key is indistinguishable from an earlier entry");
    error.combine(Error::new_spanned(earlier, "...the earlier entry is here"));
    error
}

/// When construction fails, finds the first key whose recorded hash matches an earlier key's and points at both.
/// Falls back to a generic message if no such pair exists.
fn locate_collision(hash_keys: &[HashKey], keys: &[&Expr]) -> Error {
    let mut seen: HashMap<Vec<u8>, &Expr> = HashMap::with_capacity(hash_keys.len());
    for (hash_key, key) in hash_keys.iter().zip(keys.iter().copied()) {
        let mut recorder = Recorder(Vec::new());
        hash_key.hash(&mut recorder);
        if let Some(&earlier) = seen.get(&recorder.0) {
            return indistinguishable_error(key, earlier);
        }
        seen.insert(recorder.0, key);
    }
    Error::new(
        Span::call_site(),
        "could not build a perfect hash function for these keys (are their `Hash` and `Eq` consistent?)",
    )
}

fn compute_parts(keys: &[&Expr]) -> SynResult<MapState> {
    if keys.len() > MAX_LEN {
        let message = format!("a map or set may have at most {MAX_LEN} entries");
        return Err(match keys.get(MAX_LEN) {
            Some(key) => Error::new_spanned(key, message),
            None => Error::new(Span::call_site(), message),
        });
    }

    let hash_keys: Vec<HashKey> = keys
        .iter()
        .map(|&key| expr_to_hash_key(key))
        .collect::<SynResult<_>>()?;

    let mut seen: HashMap<&HashKey, &Expr> = HashMap::with_capacity(hash_keys.len());
    for (hash_key, key) in hash_keys.iter().zip(keys.iter().copied()) {
        if let Some(&earlier) = seen.get(hash_key) {
            return Err(indistinguishable_error(key, earlier));
        }
        seen.insert(hash_key, key);
    }

    construct(&hash_keys).ok_or_else(|| locate_collision(&hash_keys, keys))
}

/// The path to the `lute` crate as referenced from generated code.
fn crate_path() -> TokenStream2 {
    if let Ok(FoundCrate::Name(name)) = crate_name("lute") {
        let ident = Ident::new(&name, Span::call_site());
        quote!(::#ident)
    } else {
        quote!(::lute)
    }
}

/// Renders a displacement table as array element tokens.
fn displacement_tokens(displacements: &[(u16, u16)]) -> impl Iterator<Item = TokenStream2> + '_ {
    displacements.iter().map(|&(d1, d2)| quote!((#d1, #d2)))
}

struct MapEntry {
    key: Expr,
    value: Expr,
}

impl Parse for MapEntry {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        let key = input.parse()?;
        input.parse::<Token![=>]>()?;
        let value = input.parse()?;
        Ok(MapEntry { key, value })
    }
}

struct MapInput {
    entries: Punctuated<MapEntry, Token![,]>,
}

impl Parse for MapInput {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        Ok(MapInput {
            entries: Punctuated::parse_terminated(input)?,
        })
    }
}

struct SetInput {
    elements: Punctuated<Expr, Token![,]>,
}

impl Parse for SetInput {
    fn parse(input: ParseStream<'_>) -> SynResult<Self> {
        // This is like `Punctuated::parse_terminated`, but if a `=>` follows an element then we surface
        // the likely map/set mixup instead of the "expected `,`" message from `syn`.
        let mut elements = Punctuated::new();
        while !input.is_empty() {
            elements.push_value(input.parse()?);
            if input.peek(Token![=>]) {
                return Err(input.error(
                    "`set!` takes a list of values, not `key => value` pairs; use `map!` for those",
                ));
            }
            if input.is_empty() {
                break;
            }
            elements.push_punct(input.parse::<Token![,]>()?);
        }
        Ok(SetInput { elements })
    }
}

/// Returns whether the input key's hash depends on the build host's pointer width.
fn hash_depends_on_pointer_width(expr: &Expr) -> bool {
    match expr {
        Expr::Lit(ExprLit { lit, .. }) => match lit {
            Lit::Int(int) => matches!(int.suffix(), "usize" | "isize"),
            Lit::ByteStr(_) | Lit::CStr(_) => true,
            _ => false,
        },
        Expr::Array(_) | Expr::Repeat(_) => true,
        Expr::Tuple(tuple) => tuple.elems.iter().any(hash_depends_on_pointer_width),
        Expr::Range(range) => {
            range
                .start
                .as_deref()
                .is_some_and(hash_depends_on_pointer_width)
                || range
                    .end
                    .as_deref()
                    .is_some_and(hash_depends_on_pointer_width)
        }
        Expr::Unary(unary) => hash_depends_on_pointer_width(&unary.expr),
        Expr::Paren(paren) => hash_depends_on_pointer_width(&paren.expr),
        Expr::Group(group) => hash_depends_on_pointer_width(&group.expr),
        Expr::Reference(reference) => hash_depends_on_pointer_width(&reference.expr),
        _ => false,
    }
}

fn with_pointer_width_guard(expr: TokenStream2, keys: &[&Expr]) -> TokenStream2 {
    if keys.iter().any(|&key| hash_depends_on_pointer_width(key)) {
        let host_width = size_of::<usize>();
        let message = format!(
            "a key's hash was computed for a {host_width}-byte pointer width, but this target's pointer width differs; \
             rebuild on a matching target or use keys with width-independent hashes"
        );
        quote! {{
            const _: () = ::core::assert!(::core::mem::size_of::<usize>() == #host_width, #message);
            #expr
        }}
    } else {
        expr
    }
}

fn expand_map(input: MapInput) -> SynResult<TokenStream2> {
    let entries: Vec<MapEntry> = input.entries.into_iter().collect();
    let keys: Vec<&Expr> = entries.iter().map(|entry| &entry.key).collect();

    let MapState {
        seed,
        displacements,
        indices,
    } = compute_parts(&keys)?;
    let krate = crate_path();
    let displacements = displacement_tokens(&displacements);
    let baked = indices.iter().map(|&i| {
        let MapEntry { key, value } = &entries[i];
        quote!((#key, #value))
    });

    let body = quote! {
        #krate::Map::from_baked_parts(#seed, &[#(#displacements),*], &[#(#baked),*])
    };
    Ok(with_pointer_width_guard(body, &keys))
}

fn expand_set(input: SetInput) -> SynResult<TokenStream2> {
    let elements: Vec<Expr> = input.elements.into_iter().collect();
    let keys: Vec<&Expr> = elements.iter().collect();

    let MapState {
        seed,
        displacements,
        indices,
    } = compute_parts(&keys)?;
    let krate = crate_path();
    let displacements = displacement_tokens(&displacements);
    let baked = indices.iter().map(|&i| {
        let element = &elements[i];
        quote!((#element, ()))
    });

    let body = quote! {
        #krate::Set::from_baked_map(
            #krate::Map::from_baked_parts(#seed, &[#(#displacements),*], &[#(#baked),*])
        )
    };
    Ok(with_pointer_width_guard(body, &keys))
}

/// Renders an error to tokens for the macro's expression position.
fn into_compile_errors(error: Error) -> TokenStream2 {
    let errors = error.into_compile_error();
    quote! {{ #errors }}
}

/// Builds an immutable [`Map`](lute_core::Map) at compile time from a comma-separated list of `key => value` entries.
///
/// The result is an expression that can be used for a `static` or `const`:
///
/// ```ignore
/// static PLANETS: lute::Map<&str, i32> = lute::map! {
///     "Mercury" => 1,
///     "Venus" => 2,
/// };
/// ```
///
/// Keys can be made of the following literals and inline-constructible types:
/// - integers with a type suffix (e.g. `1usize`, `-2_000i128`, `0xFFu8`)
/// - Booleans (i.e. `true`, `false`)
/// - bytes (e.g. `b'A'`, `b'\n'`)
/// - chars (e.g. `'h'`, `'\t'`, '\u{1F600}')
/// - strings and raw strings (e.g. `"hello"`, `r"hi"`, `r#"a "b" c"#`)
/// - byte strings and raw byte strings (e.g. `b"hello"`, `br"hi"`, `br#"a "b" c"#`)
/// - C strings and raw C strings (e.g. `c"hello"`, `cr"hi"`, `cr#"a "b" c"#`)
/// - tuples (e.g. `()`, `(1, "x", 'c')`)
/// - arrays (e.g. `[1, 2, 3]`, `[0u8; 16]`)
/// - ranges (e.g. `0..10`, `0..=10`, `..`, `0..`, `..10`, `..=10`)
///
/// Keys cannot be floats, named constants, custom structs, enum variants, closures,
/// or in general any expression that needs to be evaluated to determine its resulting type and value.
#[proc_macro]
#[rust_analyzer::macro_style(braces)]
pub fn map(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as MapInput);
    expand_map(input).unwrap_or_else(into_compile_errors).into()
}

/// Builds an immutable [`Set`](lute_core::Set) at compile time from a comma-separated list of entries.
///
/// The result is an expression that can be used for a `static` or `const`:
///
/// ```ignore
/// static PRIMES: lute::Set<u32> = lute::set! { 2u32, 3u32, 5u32, 7u32 };
/// ```
///
/// Entries can be made of the following literals and inline-constructible types:
/// - integers with a type suffix (e.g. `1usize`, `-2_000i128`, `0xFFu8`)
/// - Booleans (i.e. `true`, `false`)
/// - bytes (e.g. `b'A'`, `b'\n'`)
/// - chars (e.g. `'h'`, `'\t'`, '\u{1F600}')
/// - strings and raw strings (e.g. `"hello"`, `r"hi"`, `r#"a "b" c"#`)
/// - byte strings and raw byte strings (e.g. `b"hello"`, `br"hi"`, `br#"a "b" c"#`)
/// - C strings and raw C strings (e.g. `c"hello"`, `cr"hi"`, `cr#"a "b" c"#`)
/// - tuples (e.g. `()`, `(1, "x", 'c')`)
/// - arrays (e.g. `[1, 2, 3]`, `[0u8; 16]`)
/// - ranges (e.g. `0..10`, `0..=10`, `..`, `0..`, `..10`, `..=10`)
///
/// Entries cannot be floats, named constants, custom structs, enum variants, closures,
/// or in general any expression that needs to be evaluated to determine its resulting type and value.
#[proc_macro]
#[rust_analyzer::macro_style(braces)]
pub fn set(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as SetInput);
    expand_set(input).unwrap_or_else(into_compile_errors).into()
}

#[cfg(test)]
mod tests {
    use super::{HashKey, IntBits, SynResult, expr_to_hash_key, hash_depends_on_pointer_width};
    use std::ffi::CString;

    fn hash_key(src: &str) -> SynResult<HashKey> {
        expr_to_hash_key(&syn::parse_str::<syn::Expr>(src)?)
    }

    fn int(src: &str) -> (u8, u128) {
        match hash_key(src).unwrap() {
            HashKey::Int(int) => match int {
                IntBits::W8(v) => (1, u128::from(v)),
                IntBits::W16(v) => (2, u128::from(v)),
                IntBits::W32(v) => (4, u128::from(v)),
                IntBits::W64(v) => (8, u128::from(v)),
                IntBits::W128(v) => (16, v),
            },
            other => panic!("expected integer key from {src:?}, got {other:?}"),
        }
    }

    #[test]
    fn integer_radixes_and_underscores() {
        assert_eq!(int("255u8"), (1, 255));
        assert_eq!(int("0xFFu8"), (1, 255));
        assert_eq!(int("0o17u8"), (1, 0o17));
        assert_eq!(int("0b1010u8"), (1, 0b1010));
        assert_eq!(int("1_000u16"), (2, 1000));
        assert_eq!(int("0xDEAD_BEEFu32"), (4, 0xDEAD_BEEF));
    }

    #[test]
    fn integer_widths_and_signs() {
        assert_eq!(int("1u32"), (4, 1));
        assert_eq!(int("1i32"), (4, 1));
        assert_eq!(int("1u64"), (8, 1));
        assert_eq!(int("b'A'"), (1, 65));
        assert_eq!(int("2147483647i32"), (4, 0x7FFF_FFFF)); // i32::MAX
        assert_eq!(int("4294967295u32"), (4, 0xFFFF_FFFF)); // u32::MAX
    }

    #[test]
    fn integers_128_bit() {
        assert_eq!(int("0u128"), (16, 0));
        assert_eq!(
            int("340282366920938463463374607431768211455u128"),
            (16, u128::MAX)
        );
        assert_eq!(
            int("170141183460469231731687303715884105727i128"),
            (16, (1u128 << 127) - 1) // `i128::MAX`
        );
        assert_eq!(
            int("-170141183460469231731687303715884105728i128"),
            (16, 1u128 << 127) // `i128::MIN`
        );
    }

    #[test]
    fn negative_integers_use_twos_complement() {
        assert_eq!(int("-1i32"), (4, 0xFFFF_FFFF));
        assert_eq!(int("-128i8"), (1, 0x80));
        assert_eq!(int("-2147483648i32"), (4, 0x8000_0000));
    }

    #[test]
    fn non_integer_literals() {
        assert_eq!(hash_key("\"hi\"").unwrap(), HashKey::Str("hi".to_owned()));
        assert_eq!(hash_key("b\"hi\"").unwrap(), HashKey::Bytes(b"hi".to_vec()));
        assert_eq!(
            hash_key("c\"hi\"").unwrap(),
            HashKey::CStr(CString::new("hi").unwrap())
        );
        assert_eq!(hash_key("'a'").unwrap(), HashKey::Char('a'));
        assert_eq!(hash_key("true").unwrap(), HashKey::Bool(true));
    }

    #[test]
    fn composite_keys() {
        assert_eq!(
            hash_key("[1u8, 2u8, 3u8]").unwrap(),
            HashKey::Seq(vec![
                HashKey::Int(IntBits::W8(1)),
                HashKey::Int(IntBits::W8(2)),
                HashKey::Int(IntBits::W8(3)),
            ])
        );
        assert_eq!(
            hash_key("0u16..10u16").unwrap(),
            HashKey::Range {
                inclusive: false,
                start: Some(Box::new(HashKey::Int(IntBits::W16(0)))),
                end: Some(Box::new(HashKey::Int(IntBits::W16(10)))),
            }
        );
        assert_eq!(
            hash_key("'a'..='z'").unwrap(),
            HashKey::Range {
                inclusive: true,
                start: Some(Box::new(HashKey::Char('a'))),
                end: Some(Box::new(HashKey::Char('z'))),
            }
        );
        assert_eq!(
            hash_key("..5u8").unwrap(),
            HashKey::Range {
                inclusive: false,
                start: None,
                end: Some(Box::new(HashKey::Int(IntBits::W8(5)))),
            }
        );
        assert_eq!(hash_key("()").unwrap(), HashKey::Tuple(vec![]));
    }

    #[test]
    fn tuples_nest_and_are_transparent_to_references() {
        assert_eq!(
            hash_key("(\"a\", 1u32)").unwrap(),
            HashKey::Tuple(vec![
                HashKey::Str("a".to_owned()),
                HashKey::Int(IntBits::W32(1)),
            ])
        );
        assert_eq!(
            hash_key("&((1u8, 2u8), true)").unwrap(),
            HashKey::Tuple(vec![
                HashKey::Tuple(vec![
                    HashKey::Int(IntBits::W8(1)),
                    HashKey::Int(IntBits::W8(2)),
                ]),
                HashKey::Bool(true),
            ])
        );
    }

    #[test]
    fn repeat_arrays_and_parenthesized_negation() {
        assert_eq!(
            hash_key("[7u16; 3]").unwrap(),
            HashKey::Seq(vec![
                HashKey::Int(IntBits::W16(7)),
                HashKey::Int(IntBits::W16(7)),
                HashKey::Int(IntBits::W16(7)),
            ])
        );
        assert_eq!(hash_key("[0u8; 0]").unwrap(), HashKey::Bytes(vec![]));
        assert_eq!(hash_key("[0u128; 0]").unwrap(), HashKey::Bytes(vec![]));
        assert_eq!(hash_key("[false; 0]").unwrap(), HashKey::Seq(vec![]));
    }

    #[test]
    fn parenthesized_ints() {
        assert_eq!(hash_key("-(1i32)").unwrap(), hash_key("-1i32").unwrap());
        assert_eq!(hash_key("(5u8)").unwrap(), hash_key("5u8").unwrap());
    }

    #[test]
    fn pointer_width_dependence() {
        let depends =
            |src: &str| hash_depends_on_pointer_width(&syn::parse_str::<syn::Expr>(src).unwrap());

        for src in [
            "1usize",
            "-1isize",
            "b\"hi\"",
            "c\"hi\"",
            "[1u8, 2u8]",
            "&[1u32, 2u32]",
            "[0u8; 4]",
            "(1u8, [2u8, 3u8])",
            "(b\"x\", 1u16)",
        ] {
            assert!(depends(src), "{src} should depend on pointer width");
        }

        for src in [
            "1u64",
            "1u8",
            "\"hi\"",
            "'a'",
            "true",
            "b'A'",
            "(1u8, 2u16)",
            "0u16..10u16",
            "'a'..'z'",
        ] {
            assert!(!depends(src), "{src} should not depend on pointer width");
        }
    }

    #[test]
    fn rejected_keys() {
        assert!(hash_key("1").is_err(), "unsuffixed integer");
        assert!(hash_key("1.0f32").is_err(), "float");
        assert!(hash_key("-1u32").is_err(), "negated unsigned");
        assert!(hash_key("-b\"x\"").is_err(), "negated byte string");
        assert!(hash_key("256u8").is_err(), "out of range");
        assert!(hash_key("2147483648i32").is_err(), "positive overflow");
        assert!(hash_key("-2147483649i32").is_err(), "below i32::MIN");
        assert!(
            hash_key("340282366920938463463374607431768211456u128").is_err(),
            "exceeds u128"
        );
        assert!(hash_key("i32::MAX").is_err(), "associated constant");
        assert!(hash_key("SOME_CONST").is_err(), "named constant / path");
        assert!(hash_key("[]").is_err(), "empty array; no element type");
        assert!(hash_key("&[]").is_err(), "bare empty slice reference");
        assert!(hash_key("[0u8; SIZE]").is_err(), "non-literal repeat count");
        assert!(hash_key("(1, 2u8)").is_err(), "unsuffixed integer in tuple");
        assert!(hash_key("|x: u8| x").is_err(), "closure");
        assert!(hash_key("\"a\"..\"z\"").is_err(), "string range bounds");
        assert!(
            hash_key("0u8..5u16").is_err(),
            "mismatched range bound widths"
        );
    }
}
