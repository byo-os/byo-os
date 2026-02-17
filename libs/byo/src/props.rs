//! Typed prop conversion traits and built-in implementations.
//!
//! Two levels of traits:
//!
//! - **Per-field**: [`ReadProp`] / [`WriteProp`] — convert individual typed values
//!   to/from [`Prop`]s. One field may produce zero, one, or many props.
//! - **Per-struct**: [`FromProps`] / [`IntoProps`] — convert a whole struct to/from
//!   a prop slice. Typically derived via `#[derive(FromProps)]` / `#[derive(IntoProps)]`.

use crate::protocol::Prop;

/// Per-field trait: read a typed value from a [`Prop`].
///
/// Requires `Default` so fields can be initialized before any props are applied.
/// `apply` is called once per matching prop — for scalar types, later calls
/// overwrite; for `Vec<T>`, each call appends.
pub trait ReadProp: Default {
    /// Apply a single prop with a matching key to this field.
    fn apply(&mut self, prop: &Prop);
}

/// Per-field trait: write a typed value as props.
///
/// Scalar types push 0 or 1 prop. `Vec<T>` pushes one per element.
pub trait WriteProp {
    /// Append zero or more props for this field to `out`.
    fn encode(&self, key: &str, out: &mut Vec<Prop>);
}

/// Struct-level: build a typed struct from a prop slice.
pub trait FromProps: Sized {
    fn from_props(props: &[Prop]) -> Self;
}

/// Struct-level: convert a typed struct into a prop list.
pub trait ToProps {
    fn to_props(&self) -> Vec<Prop>;
}

// ---------------------------------------------------------------------------
// Built-in ReadProp impls
// ---------------------------------------------------------------------------

impl ReadProp for bool {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Boolean { .. } => *self = true,
            Prop::Value { value, .. } => match value.as_ref() {
                "true" | "1" => *self = true,
                "false" | "0" => *self = false,
                _ => {}
            },
            Prop::Remove { .. } => *self = false,
        }
    }
}

impl ReadProp for String {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => *self = value.to_string(),
            Prop::Boolean { .. } => *self = "true".to_string(),
            Prop::Remove { .. } => *self = String::new(),
        }
    }
}

impl ReadProp for Option<String> {
    fn apply(&mut self, prop: &Prop) {
        match prop {
            Prop::Value { value, .. } => *self = Some(value.to_string()),
            Prop::Boolean { .. } => *self = Some("true".to_string()),
            Prop::Remove { .. } => *self = None,
        }
    }
}

macro_rules! impl_read_prop_numeric {
    ($($ty:ty),*) => {
        $(
            impl ReadProp for $ty {
                fn apply(&mut self, prop: &Prop) {
                    if let Prop::Value { value, .. } = prop {
                        if let Ok(v) = value.parse::<$ty>() {
                            *self = v;
                        }
                    }
                }
            }

            impl ReadProp for Option<$ty> {
                fn apply(&mut self, prop: &Prop) {
                    match prop {
                        Prop::Value { value, .. } => {
                            if let Ok(v) = value.parse::<$ty>() {
                                *self = Some(v);
                            }
                        }
                        Prop::Remove { .. } => *self = None,
                        _ => {}
                    }
                }
            }
        )*
    };
}

impl_read_prop_numeric!(f32, f64, i32, u32, i64, u64);

impl<T: ReadProp> ReadProp for Vec<T> {
    fn apply(&mut self, prop: &Prop) {
        if matches!(prop, Prop::Remove { .. }) {
            self.clear();
            return;
        }
        let mut elem = T::default();
        elem.apply(prop);
        self.push(elem);
    }
}

// ---------------------------------------------------------------------------
// Built-in WriteProp impls
// ---------------------------------------------------------------------------

impl WriteProp for bool {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        if *self {
            out.push(Prop::flag(key));
        }
    }
}

impl WriteProp for String {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        out.push(Prop::val(key, self.as_str()));
    }
}

impl WriteProp for Option<String> {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        if let Some(v) = self {
            out.push(Prop::val(key, v.as_str()));
        }
    }
}

macro_rules! impl_write_prop_numeric {
    ($($ty:ty),*) => {
        $(
            impl WriteProp for $ty {
                fn encode(&self, key: &str, out: &mut Vec<Prop>) {
                    out.push(Prop::val(key, self.to_string()));
                }
            }

            impl WriteProp for Option<$ty> {
                fn encode(&self, key: &str, out: &mut Vec<Prop>) {
                    if let Some(v) = self {
                        out.push(Prop::val(key, v.to_string()));
                    }
                }
            }
        )*
    };
}

impl_write_prop_numeric!(f32, f64, i32, u32, i64, u64);

impl<T: WriteProp> WriteProp for Vec<T> {
    fn encode(&self, key: &str, out: &mut Vec<Prop>) {
        for elem in self {
            elem.encode(key, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // -----------------------------------------------------------------------
    // ReadProp tests
    // -----------------------------------------------------------------------

    #[test]
    fn read_bool_flag() {
        let mut v = false;
        v.apply(&Prop::flag("hidden"));
        assert!(v);
    }

    #[test]
    fn read_bool_value_true() {
        let mut v = false;
        v.apply(&Prop::val("hidden", "true"));
        assert!(v);
    }

    #[test]
    fn read_bool_value_one() {
        let mut v = false;
        v.apply(&Prop::val("hidden", "1"));
        assert!(v);
    }

    #[test]
    fn read_bool_value_false() {
        let mut v = true;
        v.apply(&Prop::val("hidden", "false"));
        assert!(!v);
    }

    #[test]
    fn read_bool_value_zero() {
        let mut v = true;
        v.apply(&Prop::val("hidden", "0"));
        assert!(!v);
    }

    #[test]
    fn read_bool_remove() {
        let mut v = true;
        v.apply(&Prop::remove("hidden"));
        assert!(!v);
    }

    #[test]
    fn read_string_value() {
        let mut v = String::new();
        v.apply(&Prop::val("class", "w-64"));
        assert_eq!(v, "w-64");
    }

    #[test]
    fn read_string_boolean() {
        let mut v = String::new();
        v.apply(&Prop::flag("hidden"));
        assert_eq!(v, "true");
    }

    #[test]
    fn read_string_remove() {
        let mut v = "hello".to_string();
        v.apply(&Prop::remove("class"));
        assert_eq!(v, "");
    }

    #[test]
    fn read_option_string_value() {
        let mut v: Option<String> = None;
        v.apply(&Prop::val("class", "w-64"));
        assert_eq!(v, Some("w-64".to_string()));
    }

    #[test]
    fn read_option_string_remove() {
        let mut v: Option<String> = Some("hello".to_string());
        v.apply(&Prop::remove("class"));
        assert_eq!(v, None);
    }

    #[test]
    fn read_f32_value() {
        let mut v: f32 = 0.0;
        v.apply(&Prop::val("opacity", "0.5"));
        assert!((v - 0.5).abs() < f32::EPSILON);
    }

    #[test]
    fn read_f32_bad_parse_ignored() {
        let mut v: f32 = 1.0;
        v.apply(&Prop::val("opacity", "not-a-number"));
        assert!((v - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn read_option_f32_value() {
        let mut v: Option<f32> = None;
        v.apply(&Prop::val("opacity", "0.75"));
        assert_eq!(v, Some(0.75));
    }

    #[test]
    fn read_option_f32_remove() {
        let mut v: Option<f32> = Some(1.0);
        v.apply(&Prop::remove("opacity"));
        assert_eq!(v, None);
    }

    #[test]
    fn read_i32_value() {
        let mut v: i32 = 0;
        v.apply(&Prop::val("order", "42"));
        assert_eq!(v, 42);
    }

    #[test]
    fn read_vec_string_repeated() {
        let mut v: Vec<String> = Vec::new();
        v.apply(&Prop::val("tag", "blue"));
        v.apply(&Prop::val("tag", "green"));
        assert_eq!(v, vec!["blue".to_string(), "green".to_string()]);
    }

    #[test]
    fn read_vec_string_remove_clears() {
        let mut v: Vec<String> = vec!["a".to_string()];
        v.apply(&Prop::remove("tag"));
        assert!(v.is_empty());
    }

    #[test]
    fn read_vec_f32_repeated() {
        let mut v: Vec<f32> = Vec::new();
        v.apply(&Prop::val("point", "1.5"));
        v.apply(&Prop::val("point", "2.5"));
        assert_eq!(v, vec![1.5, 2.5]);
    }

    // -----------------------------------------------------------------------
    // WriteProp tests
    // -----------------------------------------------------------------------

    #[test]
    fn write_bool_true() {
        let mut out = Vec::new();
        true.encode("hidden", &mut out);
        assert_eq!(out, vec![Prop::flag("hidden")]);
    }

    #[test]
    fn write_bool_false() {
        let mut out = Vec::new();
        false.encode("hidden", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn write_string() {
        let mut out = Vec::new();
        "w-64".to_string().encode("class", &mut out);
        assert_eq!(out, vec![Prop::val("class", "w-64")]);
    }

    #[test]
    fn write_option_string_some() {
        let mut out = Vec::new();
        Some("w-64".to_string()).encode("class", &mut out);
        assert_eq!(out, vec![Prop::val("class", "w-64")]);
    }

    #[test]
    fn write_option_string_none() {
        let mut out = Vec::new();
        None::<String>.encode("class", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn write_f32() {
        let mut out = Vec::new();
        (0.5f32).encode("opacity", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].key(), "opacity");
    }

    #[test]
    fn write_option_f32_some() {
        let mut out = Vec::new();
        Some(0.75f32).encode("opacity", &mut out);
        assert_eq!(out.len(), 1);
    }

    #[test]
    fn write_option_f32_none() {
        let mut out = Vec::new();
        None::<f32>.encode("opacity", &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn write_i32() {
        let mut out = Vec::new();
        42i32.encode("order", &mut out);
        assert_eq!(out, vec![Prop::val("order", "42")]);
    }

    #[test]
    fn write_vec_string() {
        let v = vec!["blue".to_string(), "green".to_string()];
        let mut out = Vec::new();
        v.encode("tag", &mut out);
        assert_eq!(
            out,
            vec![Prop::val("tag", "blue"), Prop::val("tag", "green")]
        );
    }

    #[test]
    fn write_vec_f32() {
        let v = vec![1.5f32, 2.5f32];
        let mut out = Vec::new();
        v.encode("point", &mut out);
        assert_eq!(out.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Round-trip tests
    // -----------------------------------------------------------------------

    #[test]
    fn round_trip_bool() {
        let mut out = Vec::new();
        true.encode("hidden", &mut out);
        let mut v = false;
        for p in &out {
            v.apply(p);
        }
        assert!(v);
    }

    #[test]
    fn round_trip_string() {
        let original = "px-4 py-2".to_string();
        let mut out = Vec::new();
        original.encode("class", &mut out);
        let mut v = String::new();
        for p in &out {
            v.apply(p);
        }
        assert_eq!(v, original);
    }

    #[test]
    fn round_trip_option_string() {
        let original = Some("hello".to_string());
        let mut out = Vec::new();
        original.encode("label", &mut out);
        let mut v: Option<String> = None;
        for p in &out {
            v.apply(p);
        }
        assert_eq!(v, original);
    }

    #[test]
    fn round_trip_vec_string() {
        let original = vec!["a".to_string(), "b".to_string()];
        let mut out = Vec::new();
        original.encode("tag", &mut out);
        let mut v: Vec<String> = Vec::new();
        for p in &out {
            v.apply(p);
        }
        assert_eq!(v, original);
    }
}
