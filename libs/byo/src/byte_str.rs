//! Ref-counted string types for zero-copy command representation.
//!
//! [`ByteStr`] is a shared view into a ref-counted buffer (cheap clone via
//! atomic increment). [`ByteString`] is an independently-owned compact
//! allocation — sub-slicing a `ByteString` produces `ByteStr` views that
//! pin only the `ByteString`'s content, not a larger parse buffer.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ops::{Deref, Range};

use bytes::Bytes;

/// Shared view into a ref-counted buffer. Cheap clone (atomic increment).
/// May pin a larger source buffer.
#[derive(Clone)]
pub struct ByteStr(Bytes);

/// Independently-owned compact allocation backed by [`Bytes`].
/// Sub-slicing produces [`ByteStr`] views that only pin this allocation.
#[derive(Clone)]
pub struct ByteString(Bytes);

// -- ByteStr ------------------------------------------------------------------

impl ByteStr {
    /// Create from validated UTF-8 `Bytes`.
    ///
    /// # Errors
    /// Returns `Err` if `bytes` is not valid UTF-8.
    pub fn from_utf8(bytes: Bytes) -> Result<Self, std::str::Utf8Error> {
        std::str::from_utf8(&bytes)?;
        Ok(Self(bytes))
    }

    /// Create from a `&'static str` — zero allocation.
    pub fn from_static(s: &'static str) -> Self {
        Self(Bytes::from_static(s.as_bytes()))
    }

    /// Sub-slice this `ByteStr`. The result shares the same backing buffer.
    ///
    /// # Panics
    /// Panics if the range is out of bounds or splits a UTF-8 codepoint.
    pub fn slice(&self, range: Range<usize>) -> ByteStr {
        assert!(self.as_str().is_char_boundary(range.start));
        assert!(self.as_str().is_char_boundary(range.end));
        ByteStr(self.0.slice(range))
    }

    /// Consume into the underlying `Bytes`.
    pub fn into_bytes(self) -> Bytes {
        self.0
    }

    fn as_str(&self) -> &str {
        // SAFETY: we validated UTF-8 on construction.
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl Deref for ByteStr {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for ByteStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for ByteStr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for ByteStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for ByteStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl PartialEq for ByteStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ByteStr {}

impl PartialOrd for ByteStr {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ByteStr {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ByteStr {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<str> for ByteStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ByteStr {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for ByteStr {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<ByteString> for ByteStr {
    fn eq(&self, other: &ByteString) -> bool {
        self.as_str() == other.as_str()
    }
}

impl From<&str> for ByteStr {
    fn from(s: &str) -> Self {
        Self(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<String> for ByteStr {
    fn from(s: String) -> Self {
        Self(Bytes::from(s.into_bytes()))
    }
}

impl From<ByteString> for ByteStr {
    fn from(s: ByteString) -> Self {
        Self(s.0)
    }
}

// -- ByteString ---------------------------------------------------------------

impl ByteString {
    /// Sub-slice producing a `ByteStr` that pins only this allocation.
    ///
    /// # Panics
    /// Panics if the range is out of bounds or splits a UTF-8 codepoint.
    pub fn slice(&self, range: Range<usize>) -> ByteStr {
        assert!(self.as_str().is_char_boundary(range.start));
        assert!(self.as_str().is_char_boundary(range.end));
        ByteStr(self.0.slice(range))
    }

    fn as_str(&self) -> &str {
        // SAFETY: we validated UTF-8 on construction.
        unsafe { std::str::from_utf8_unchecked(&self.0) }
    }
}

impl Deref for ByteString {
    type Target = str;
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for ByteString {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for ByteString {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl fmt::Debug for ByteString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for ByteString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

impl PartialEq for ByteString {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for ByteString {}

impl PartialOrd for ByteString {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ByteString {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for ByteString {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state);
    }
}

impl PartialEq<str> for ByteString {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for ByteString {
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialEq<String> for ByteString {
    fn eq(&self, other: &String) -> bool {
        self.as_str() == other.as_str()
    }
}

impl PartialEq<ByteStr> for ByteString {
    fn eq(&self, other: &ByteStr) -> bool {
        self.as_str() == other.as_str()
    }
}

impl From<&str> for ByteString {
    fn from(s: &str) -> Self {
        Self(Bytes::copy_from_slice(s.as_bytes()))
    }
}

impl From<String> for ByteString {
    fn from(s: String) -> Self {
        Self(Bytes::from(s.into_bytes()))
    }
}

impl From<ByteStr> for ByteString {
    /// Compact copy — produces an independent allocation.
    fn from(s: ByteStr) -> Self {
        Self(Bytes::copy_from_slice(&s.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_str_from_str() {
        let s = ByteStr::from("hello");
        assert_eq!(&*s, "hello");
        assert_eq!(s, "hello");
    }

    #[test]
    fn byte_str_from_static() {
        let s = ByteStr::from_static("hello");
        assert_eq!(s, "hello");
    }

    #[test]
    fn byte_str_from_string() {
        let s = ByteStr::from(String::from("hello"));
        assert_eq!(s, "hello");
    }

    #[test]
    fn byte_str_slice() {
        let s = ByteStr::from("hello world");
        let sub = s.slice(0..5);
        assert_eq!(sub, "hello");
    }

    #[test]
    fn byte_str_from_utf8() {
        let b = Bytes::from_static(b"hello");
        let s = ByteStr::from_utf8(b).unwrap();
        assert_eq!(s, "hello");
    }

    #[test]
    fn byte_str_from_utf8_invalid() {
        let b = Bytes::from_static(b"\xff\xfe");
        assert!(ByteStr::from_utf8(b).is_err());
    }

    #[test]
    fn byte_string_from_str() {
        let s = ByteString::from("hello");
        assert_eq!(&*s, "hello");
        assert_eq!(s, "hello");
    }

    #[test]
    fn byte_string_slice() {
        let s = ByteString::from("hello world");
        let sub = s.slice(6..11);
        assert_eq!(sub, "world");
    }

    #[test]
    fn cross_type_eq() {
        let a = ByteStr::from("hello");
        let b = ByteString::from("hello");
        assert_eq!(a, b);
        assert_eq!(b, a);
    }

    #[test]
    fn byte_str_clone() {
        let a = ByteStr::from("hello");
        let b = a.clone();
        assert_eq!(a, b);
    }

    #[test]
    fn byte_string_to_byte_str() {
        let s = ByteString::from("hello");
        let bs: ByteStr = s.into();
        assert_eq!(bs, "hello");
    }

    #[test]
    fn byte_str_to_byte_string() {
        let bs = ByteStr::from("hello");
        let s: ByteString = bs.into();
        assert_eq!(s, "hello");
    }

    #[test]
    fn hash_consistent() {
        use std::collections::HashMap;
        let mut map: HashMap<ByteStr, i32> = HashMap::new();
        map.insert(ByteStr::from("key"), 42);
        assert_eq!(map.get("key"), Some(&42));
    }

    #[test]
    fn hash_consistent_bytestring() {
        use std::collections::HashMap;
        let mut map: HashMap<ByteString, i32> = HashMap::new();
        map.insert(ByteString::from("key"), 42);
        assert_eq!(map.get("key"), Some(&42));
    }

    #[test]
    fn display() {
        let s = ByteStr::from("hello");
        assert_eq!(format!("{s}"), "hello");
    }

    #[test]
    fn debug() {
        let s = ByteStr::from("hello");
        assert_eq!(format!("{s:?}"), "\"hello\"");
    }

    #[test]
    fn ord() {
        let a = ByteStr::from("aaa");
        let b = ByteStr::from("bbb");
        assert!(a < b);
    }
}
