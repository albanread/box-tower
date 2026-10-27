// JavaScript semantics the translation needs (TRANSLATION.md): numbers,
// missing values, and JsObject, a map in JavaScript property order.
//
// Nothing here is a translation of an upstream file; each helper is named
// for the JavaScript operation it stands for, and TRANSLATION.md says where
// each one is used.

use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;

use indexmap::IndexMap;
use serde::de::{self, MapAccess, Visitor};
use serde::ser::SerializeMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

// ─── Numbers ────────────────────────────────────────────────────────────────

/// `Math.floor(a / b)` for integers.  `b` must not be 0 (JavaScript would
/// give an infinity or NaN; the simulation never divides by zero).
pub fn floor_div(a: i64, b: i64) -> i64 {
    let q = a / b;
    if a % b != 0 && ((a < 0) != (b < 0)) {
        q - 1
    } else {
        q
    }
}

/// `Math.trunc(a / b)` for integers (Rust's own `/`).  `b` must not be 0.
pub fn trunc_div(a: i64, b: i64) -> i64 {
    a / b
}

/// `Math.round(x)`: the nearest integer, halves rounded towards +infinity
/// (`Math.round(-2.5)` is -2).  NaN gives 0 (JavaScript would give NaN; the
/// simulation never rounds a NaN).
pub fn round(x: f64) -> i64 {
    let r = x.floor();
    if x - r >= 0.5 {
        r as i64 + 1
    } else {
        r as i64
    }
}

/// `x | 0`: ToInt32, the value modulo 2^32 as a signed 32-bit integer.
pub fn to_int32(x: i64) -> i64 {
    x as u32 as i32 as i64
}

/// `x >>> 0`: ToUint32, the value modulo 2^32 as an unsigned 32-bit integer.
pub fn to_uint32(x: i64) -> i64 {
    x as u32 as i64
}

/// ToInt32 of a double (`x | 0` where `x` may hold a fraction): truncated,
/// then taken modulo 2^32; NaN and the infinities give 0.
pub fn f64_to_int32(x: f64) -> i64 {
    if !x.is_finite() {
        return 0;
    }
    let t = x.trunc();
    let m = t.rem_euclid(4_294_967_296.0);
    to_int32(m as i64)
}

/// `Math.imul(a, b)`: the low 32 bits of the product, signed.
pub fn imul(a: i64, b: i64) -> i64 {
    (to_int32(a) as i32).wrapping_mul(to_int32(b) as i32) as i64
}

/// `a << b`.
pub fn shl(a: i64, b: i64) -> i64 {
    (to_int32(a) as i32).wrapping_shl((b as u32) & 31) as i64
}

/// `a >> b` (sign-propagating).
pub fn sar(a: i64, b: i64) -> i64 {
    ((to_int32(a) as i32) >> ((b as u32) & 31)) as i64
}

/// `a >>> b` (zero-filling).
pub fn shr(a: i64, b: i64) -> i64 {
    ((to_uint32(a) as u32) >> ((b as u32) & 31)) as i64
}

// ─── Missing values ─────────────────────────────────────────────────────────

/// `arr[i] ?? default`: the element at `i`, or `default` where JavaScript
/// would read `undefined` (a negative index or one past the end).
pub fn at_or<T: Copy>(arr: &[T], i: i64, default: T) -> T {
    if i < 0 {
        return default;
    }
    arr.get(i as usize).copied().unwrap_or(default)
}

/// A constant `Record<K, V>` written as a slice of pairs in JavaScript
/// property order (resources.ts's tables, FAMILY_PROLOGUE_TABLE and the
/// like): `TABLE[key]`, `None` where JavaScript reads `undefined`.  The
/// tables hold no duplicate keys.
pub fn record_get<K: PartialEq + Copy, V>(table: &[(K, V)], key: K) -> Option<&V> {
    table.iter().find(|(k, _)| *k == key).map(|(_, v)| v)
}

/// One element of a JavaScript array of integers that upstream lets grow
/// by writing past its end (`ledger.incomeLedger[FAMILY_CINEMA] += x` on a
/// ten-element array).  The array then has holes, and a hole is
/// `undefined`: `undefined + x` is NaN, `undefined ?? 0` is 0.  A saved
/// game written by JSON.stringify holds `null` for both holes and NaN, and
/// JSON.parse gives back `null`, which is different again: `null + x` is
/// `x`.  All four are kept apart so that the arithmetic and the JSON come
/// out as upstream's do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum JsSlot {
    Number(i64),
    NaN,
    /// A hole (or an explicit `undefined`).
    Hole,
    /// `null`, as JSON.parse gives back for a hole or a NaN.
    Null,
}

impl Default for JsSlot {
    fn default() -> Self {
        JsSlot::Number(0)
    }
}

impl JsSlot {
    /// ToNumber: `None` is NaN.  `null` is 0, `undefined` is NaN.
    pub fn to_number(self) -> Option<i64> {
        match self {
            JsSlot::Number(n) => Some(n),
            JsSlot::NaN | JsSlot::Hole => None,
            JsSlot::Null => Some(0),
        }
    }

    /// The slot holding a number, or NaN.
    pub fn from_number(n: Option<i64>) -> Self {
        match n {
            Some(n) => JsSlot::Number(n),
            None => JsSlot::NaN,
        }
    }

    /// `slot ?? d`: `d` for `undefined` and `null`, else the number (which
    /// may be NaN: `None`).
    pub fn or(self, d: i64) -> Option<i64> {
        match self {
            JsSlot::Hole | JsSlot::Null => Some(d),
            other => other.to_number(),
        }
    }

    /// `slot += x`.
    pub fn add_assign(&mut self, x: i64) {
        *self = JsSlot::from_number(self.to_number().map(|v| v + x));
    }
}

/// `arr[i]` for writing, growing the array with holes as JavaScript does
/// when `i` is at or past its end.  `i` must not be negative (a negative
/// index would make a named property, which upstream never does).
pub fn slot_mut(arr: &mut Vec<JsSlot>, i: i64) -> &mut JsSlot {
    assert!(i >= 0, "negative array index {i}");
    let i = i as usize;
    if i >= arr.len() {
        arr.resize(i + 1, JsSlot::Hole);
    }
    &mut arr[i]
}

/// `arr[i]` for reading: `Hole` past the end or for a negative index.
pub fn slot_at(arr: &[JsSlot], i: i64) -> JsSlot {
    if i < 0 {
        return JsSlot::Hole;
    }
    arr.get(i as usize).copied().unwrap_or(JsSlot::Hole)
}

impl Serialize for JsSlot {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            JsSlot::Number(n) => s.serialize_i64(*n),
            // JSON.stringify writes holes, undefined elements and NaN as null.
            JsSlot::NaN | JsSlot::Hole | JsSlot::Null => s.serialize_unit(),
        }
    }
}

impl<'de> Deserialize<'de> for JsSlot {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        Ok(match Option::<i64>::deserialize(d)? {
            Some(n) => JsSlot::Number(n),
            None => JsSlot::Null,
        })
    }
}

// ─── JsObject ───────────────────────────────────────────────────────────────

/// Whether `key` is an array index: the canonical decimal string of an
/// integer in 0..=2^32-2 ("0", "17"; not "01", "-1", "+1", "1.0" or
/// "4294967295").
pub fn is_array_index(key: &str) -> Option<u32> {
    let b = key.as_bytes();
    if b.is_empty() || b.len() > 10 || !b.iter().all(u8::is_ascii_digit) {
        return None;
    }
    if b.len() > 1 && b[0] == b'0' {
        return None;
    }
    let v: u64 = key.parse().ok()?;
    if v <= 4_294_967_294 {
        Some(v as u32)
    } else {
        None
    }
}

/// A plain JavaScript object used as a map (`Record<string, T>`), with
/// JavaScript's own-property order: keys that are array indices first, in
/// ascending numeric order, then the other keys in the order they were
/// first added.  Assigning to an existing key keeps its place; removing a
/// key and adding it again puts a non-index key last.
#[derive(Clone, PartialEq, Eq)]
pub struct JsObject<T> {
    indices: BTreeMap<u32, (String, T)>,
    named: IndexMap<String, T>,
}

impl<T> Default for JsObject<T> {
    fn default() -> Self {
        JsObject {
            indices: BTreeMap::new(),
            named: IndexMap::new(),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for JsObject<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<T> JsObject<T> {
    /// `{}`.
    pub fn new() -> Self {
        Self::default()
    }

    /// `Object.keys(o).length`.
    pub fn len(&self) -> usize {
        self.indices.len() + self.named.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `o[key]` (`None` for `undefined`).
    pub fn get(&self, key: &str) -> Option<&T> {
        match is_array_index(key) {
            Some(i) => self.indices.get(&i).map(|(_, v)| v),
            None => self.named.get(key),
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        match is_array_index(key) {
            Some(i) => self.indices.get_mut(&i).map(|(_, v)| v),
            None => self.named.get_mut(key),
        }
    }

    /// `key in o` (for objects whose values are never `undefined`).
    pub fn contains_key(&self, key: &str) -> bool {
        self.get(key).is_some()
    }

    /// `o[key] = value`; returns the value it replaced.  An existing key
    /// keeps its place.
    pub fn insert(&mut self, key: impl Into<String>, value: T) -> Option<T> {
        let key = key.into();
        match is_array_index(&key) {
            Some(i) => self.indices.insert(i, (key, value)).map(|(_, v)| v),
            None => self.named.insert(key, value),
        }
    }

    /// `delete o[key]`; returns the value removed.
    pub fn remove(&mut self, key: &str) -> Option<T> {
        match is_array_index(key) {
            Some(i) => self.indices.remove(&i).map(|(_, v)| v),
            None => self.named.shift_remove(key),
        }
    }

    /// `o[key] ??= f()` and then `o[key]`: the value at `key`, added at the
    /// end (or in index order) if absent.
    pub fn get_or_insert_with(&mut self, key: &str, f: impl FnOnce() -> T) -> &mut T {
        match is_array_index(key) {
            Some(i) => &mut self.indices.entry(i).or_insert_with(|| (key.to_string(), f())).1,
            None => self.named.entry(key.to_string()).or_insert_with(f),
        }
    }

    /// `Object.entries(o)`, in property order.
    pub fn iter(&self) -> impl DoubleEndedIterator<Item = (&str, &T)> + '_ {
        self.indices
            .values()
            .map(|(k, v)| (k.as_str(), v))
            .chain(self.named.iter().map(|(k, v)| (k.as_str(), v)))
    }

    /// `Object.entries(o)` with the values writable.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (&str, &mut T)> + '_ {
        self.indices
            .values_mut()
            .map(|(k, v)| (k.as_str(), v))
            .chain(self.named.iter_mut().map(|(k, v)| (k.as_str(), v)))
    }

    /// `Object.keys(o)`.
    pub fn keys(&self) -> impl DoubleEndedIterator<Item = &str> + '_ {
        self.iter().map(|(k, _)| k)
    }

    /// `Object.values(o)`.
    pub fn values(&self) -> impl DoubleEndedIterator<Item = &T> + '_ {
        self.iter().map(|(_, v)| v)
    }

    pub fn values_mut(&mut self) -> impl Iterator<Item = &mut T> + '_ {
        self.iter_mut().map(|(_, v)| v)
    }

    /// `Object.keys(o)` as owned strings, for loops that change `o` while
    /// they walk it (as `for (const key of Object.keys(o))` may).
    pub fn key_list(&self) -> Vec<String> {
        self.keys().map(str::to_string).collect()
    }

    /// Keep only the entries `f` accepts (a loop of `delete`s), in order.
    pub fn retain(&mut self, mut f: impl FnMut(&str, &mut T) -> bool) {
        self.indices.retain(|_, (k, v)| f(k, v));
        self.named.retain(|k, v| f(k, v));
    }

    /// `for (const k in o) delete o[k]`.
    pub fn clear(&mut self) {
        self.indices.clear();
        self.named.clear();
    }
}

impl<T> std::ops::Index<&str> for JsObject<T> {
    type Output = T;
    fn index(&self, key: &str) -> &T {
        self.get(key)
            .unwrap_or_else(|| panic!("JsObject has no key {key:?}"))
    }
}

impl<K: Into<String>, T> FromIterator<(K, T)> for JsObject<T> {
    fn from_iter<I: IntoIterator<Item = (K, T)>>(iter: I) -> Self {
        let mut o = JsObject::new();
        for (k, v) in iter {
            o.insert(k, v);
        }
        o
    }
}

impl<T: Serialize> Serialize for JsObject<T> {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut m = s.serialize_map(Some(self.len()))?;
        for (k, v) in self.iter() {
            m.serialize_entry(k, v)?;
        }
        m.end()
    }
}

impl<'de, T: Deserialize<'de>> Deserialize<'de> for JsObject<T> {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V<T>(PhantomData<T>);
        impl<'de, T: Deserialize<'de>> Visitor<'de> for V<T> {
            type Value = JsObject<T>;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a JSON object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut a: A) -> Result<Self::Value, A::Error> {
                // JSON.parse: properties in document order, a repeated key
                // replacing the value but keeping the first place.
                let mut o = JsObject::new();
                while let Some((k, v)) = a.next_entry::<String, T>()? {
                    o.insert(k, v);
                }
                Ok(o)
            }
        }
        d.deserialize_map(V(PhantomData)).map_err(de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn division_and_rounding() {
        assert_eq!(floor_div(7, 2), 3);
        assert_eq!(floor_div(-7, 2), -4);
        assert_eq!(floor_div(7, -2), -4);
        assert_eq!(floor_div(-7, -2), 3);
        assert_eq!(floor_div(-8, 2), -4);
        assert_eq!(trunc_div(-7, 2), -3);
        assert_eq!(round(2.5), 3);
        assert_eq!(round(-2.5), -2);
        assert_eq!(round(-2.6), -3);
        assert_eq!(round(0.49999999999999994), 0);
        assert_eq!(round(1.4), 1);
    }

    #[test]
    fn bit_operations() {
        assert_eq!(to_int32(0xffff_ffff), -1);
        assert_eq!(to_int32(0x8000_0000), -2_147_483_648);
        assert_eq!(to_int32(1 << 40 | 5), 5);
        assert_eq!(to_uint32(-1), 0xffff_ffff);
        assert_eq!(f64_to_int32(-1.9), -1);
        assert_eq!(f64_to_int32(4_294_967_297.5), 1);
        assert_eq!(f64_to_int32(f64::NAN), 0);
        assert_eq!(imul(0x7fff_ffff, 2), -2);
        assert_eq!(imul(1, 0x15a4e35), 0x15a4e35);
        // The world LCG's first steps from 1, as upstream computes them.
        let mut s = 1i64;
        for _ in 0..3 {
            s = to_int32(imul(s, 0x15a4e35) + 1);
        }
        assert_eq!(s, -1_427_733_316); // (Math.imul(s, 0x15a4e35) + 1) | 0, three times from 1
        assert_eq!(shl(1, 31), -2_147_483_648);
        assert_eq!(shl(1, 32), 1);
        assert_eq!(sar(-8, 1), -4);
        assert_eq!(shr(-8, 28), 15);
        assert_eq!(shr(-1, 0), 0xffff_ffff);
    }

    #[test]
    fn at_or_and_records() {
        let a = [1i64, 2, 3];
        assert_eq!(at_or(&a, 0, 9), 1);
        assert_eq!(at_or(&a, 3, 9), 9);
        assert_eq!(at_or(&a, -1, 9), 9);
        const T: &[(&str, i64)] = &[("a", 1), ("b", 2)];
        assert_eq!(record_get(T, "b"), Some(&2));
        assert_eq!(record_get(T, "c"), None);
    }

    #[test]
    fn slots_behave_as_javascript() {
        let mut v = vec![JsSlot::Number(0); 10];
        slot_mut(&mut v, 12).add_assign(5); // hole += 5 is NaN
        assert_eq!(v.len(), 13);
        assert_eq!(v[10], JsSlot::Hole);
        assert_eq!(v[12], JsSlot::NaN);
        let mut w = vec![JsSlot::Number(0); 10];
        let s = slot_mut(&mut w, 18);
        *s = JsSlot::from_number(s.or(0).map(|x| x + 7)); // (a[i] ?? 0) + 7
        assert_eq!(w[18], JsSlot::Number(7));
        let mut n = JsSlot::Null;
        n.add_assign(3); // null + 3 is 3
        assert_eq!(n, JsSlot::Number(3));
        assert_eq!(serde_json::to_string(&v).unwrap(), "[0,0,0,0,0,0,0,0,0,0,null,null,null]");
        let back: Vec<JsSlot> = serde_json::from_str("[1,null]").unwrap();
        assert_eq!(back, vec![JsSlot::Number(1), JsSlot::Null]);
    }

    #[test]
    fn array_index_keys() {
        assert_eq!(is_array_index("0"), Some(0));
        assert_eq!(is_array_index("17"), Some(17));
        assert_eq!(is_array_index("4294967294"), Some(4_294_967_294));
        for k in ["", "01", "-1", "+1", "1.0", " 1", "1e3", "4294967295", "99999999999", "0x1", "-0"] {
            assert_eq!(is_array_index(k), None, "{k:?}");
        }
    }

    fn keys<T>(o: &JsObject<T>) -> Vec<&str> {
        o.keys().collect()
    }

    #[test]
    fn property_order() {
        let mut o = JsObject::new();
        for k in ["b", "10", "a", "2", "01", "-1", "4294967295", "4294967294", "0"] {
            o.insert(k, k.len());
        }
        assert_eq!(
            keys(&o),
            ["0", "2", "10", "4294967294", "b", "a", "01", "-1", "4294967295"]
        );
        // Assigning an existing key keeps its place.
        o.insert("b", 99);
        assert_eq!(keys(&o)[4], "b");
        assert_eq!(o["b"], 99);
        // Delete and re-add: a named key goes last, an index key back in order.
        assert_eq!(o.remove("b"), Some(99));
        assert_eq!(o.remove("2"), Some(1));
        o.insert("b", 1);
        o.insert("2", 1);
        assert_eq!(
            keys(&o),
            ["0", "2", "10", "4294967294", "a", "01", "-1", "4294967295", "b"]
        );
        assert!(o.contains_key("01"));
        assert!(!o.contains_key("1"));
        assert_eq!(o.len(), 9);
        assert_eq!(o.remove("nope"), None);
    }

    #[test]
    fn helpers() {
        let mut o: JsObject<i64> = [("x", 1), ("5", 2)].into_iter().collect();
        *o.get_or_insert_with("y", || 3) += 1;
        *o.get_or_insert_with("x", || 100) += 1;
        assert_eq!(keys(&o), ["5", "x", "y"]);
        assert_eq!(o.values().copied().collect::<Vec<_>>(), [2, 2, 4]);
        for (_, v) in o.iter_mut() {
            *v *= 10;
        }
        o.retain(|k, _| k != "x");
        assert_eq!(o.iter().map(|(k, v)| (k.to_string(), *v)).collect::<Vec<_>>(),
            [("5".to_string(), 20), ("y".to_string(), 40)]);
        assert_eq!(o.key_list(), ["5", "y"]);
        o.clear();
        assert!(o.is_empty());
    }

    #[test]
    fn json_round_trip_in_property_order() {
        let src = r#"{"b":1,"7":2,"a":3,"01":4,"3":5,"-1":6}"#;
        let o: JsObject<i64> = serde_json::from_str(src).unwrap();
        assert_eq!(keys(&o), ["3", "7", "b", "a", "01", "-1"]);
        assert_eq!(
            serde_json::to_string(&o).unwrap(),
            r#"{"3":5,"7":2,"b":1,"a":3,"01":4,"-1":6}"#
        );
        // JSON.parse keeps a repeated key's first place and its last value.
        let o: JsObject<i64> = serde_json::from_str(r#"{"a":1,"b":2,"a":3}"#).unwrap();
        assert_eq!(serde_json::to_string(&o).unwrap(), r#"{"a":3,"b":2}"#);
    }
}
