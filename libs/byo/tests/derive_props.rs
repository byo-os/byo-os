//! Integration tests for `#[derive(FromProps)]`, `#[derive(ToProps)]`, and `{..spread}`.

use byo::emitter::Emitter;
use byo::props::{ReadProp, WriteProp};
use byo::{FromProps, Prop, ToProps, byo_assert_eq, byo_write};

// ---------------------------------------------------------------------------
// Test structs
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, FromProps, ToProps)]
struct ViewProps {
    class: Option<String>,
    hidden: bool,
    order: i32,
}

#[derive(Debug, PartialEq, FromProps, ToProps)]
struct RenamedProps {
    /// Auto-kebab would give "data-value", but we override to "val"
    #[prop(rename = "val")]
    data_value: String,
    label: String,
}

#[derive(Debug, PartialEq, FromProps, ToProps)]
struct AutoKebabProps {
    bg_color: String,
    font_size: i32,
}

#[derive(Debug, PartialEq, FromProps, ToProps)]
struct SkippedProps {
    label: String,
    #[prop(skip)]
    internal: u32,
}

#[derive(Debug, PartialEq, FromProps, ToProps)]
struct VecProps {
    #[prop(rename = "tag")]
    tags: Vec<String>,
    label: String,
}

// ---------------------------------------------------------------------------
// FromProps tests
// ---------------------------------------------------------------------------

#[test]
fn from_props_basic() {
    let props = [
        Prop::val("class", "w-64"),
        Prop::flag("hidden"),
        Prop::val("order", "5"),
    ];
    let v = ViewProps::from_props(&props);
    assert_eq!(v.class, Some("w-64".to_string()));
    assert!(v.hidden);
    assert_eq!(v.order, 5);
}

#[test]
fn from_props_missing() {
    let props = [Prop::val("class", "flex")];
    let v = ViewProps::from_props(&props);
    assert_eq!(v.class, Some("flex".to_string()));
    assert!(!v.hidden);
    assert_eq!(v.order, 0);
}

#[test]
fn from_props_repeated_vec() {
    let props = [
        Prop::val("label", "Hello"),
        Prop::val("tag", "blue"),
        Prop::val("tag", "green"),
    ];
    let v = VecProps::from_props(&props);
    assert_eq!(v.label, "Hello".to_string());
    assert_eq!(v.tags, vec!["blue".to_string(), "green".to_string()]);
}

// ---------------------------------------------------------------------------
// ToProps tests
// ---------------------------------------------------------------------------

#[test]
fn to_props_basic() {
    let v = ViewProps {
        class: Some("w-64".to_string()),
        hidden: true,
        order: 5,
    };
    let props = v.to_props();
    assert_eq!(
        props,
        vec![
            Prop::val("class", "w-64"),
            Prop::flag("hidden"),
            Prop::val("order", "5"),
        ]
    );
}

#[test]
fn to_props_omits_defaults() {
    let v = ViewProps {
        class: None,
        hidden: false,
        order: 0,
    };
    let props = v.to_props();
    // None → skipped, false → skipped, but 0 is always emitted for numeric
    assert_eq!(props, vec![Prop::val("order", "0")]);
}

#[test]
fn to_props_vec() {
    let v = VecProps {
        tags: vec!["a".to_string(), "b".to_string()],
        label: "Hi".to_string(),
    };
    let props = v.to_props();
    assert_eq!(
        props,
        vec![
            Prop::val("tag", "a"),
            Prop::val("tag", "b"),
            Prop::val("label", "Hi"),
        ]
    );
}

// ---------------------------------------------------------------------------
// Round-trip
// ---------------------------------------------------------------------------

#[test]
fn round_trip() {
    let original = ViewProps {
        class: Some("px-4".to_string()),
        hidden: true,
        order: 42,
    };
    let props = original.to_props();
    let restored = ViewProps::from_props(&props);
    assert_eq!(original, restored);
}

// ---------------------------------------------------------------------------
// Attributes
// ---------------------------------------------------------------------------

#[test]
fn rename_attribute() {
    let props = [Prop::val("val", "42"), Prop::val("label", "Hi")];
    let v = RenamedProps::from_props(&props);
    assert_eq!(v.data_value, "42");
    assert_eq!(v.label, "Hi");

    let encoded = v.to_props();
    assert_eq!(
        encoded,
        vec![Prop::val("val", "42"), Prop::val("label", "Hi")]
    );
}

#[test]
fn auto_kebab_case() {
    let props = [Prop::val("bg-color", "red"), Prop::val("font-size", "16")];
    let v = AutoKebabProps::from_props(&props);
    assert_eq!(v.bg_color, "red");
    assert_eq!(v.font_size, 16);

    let encoded = v.to_props();
    assert_eq!(
        encoded,
        vec![Prop::val("bg-color", "red"), Prop::val("font-size", "16")]
    );
}

#[test]
fn skip_attribute() {
    let props = [Prop::val("label", "Hello"), Prop::val("internal", "999")];
    let v = SkippedProps::from_props(&props);
    assert_eq!(v.label, "Hello");
    assert_eq!(v.internal, 0); // skipped, gets Default

    let encoded = v.to_props();
    assert_eq!(encoded, vec![Prop::val("label", "Hello")]);
}

// ---------------------------------------------------------------------------
// Spread syntax tests
// ---------------------------------------------------------------------------

fn emit(f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> std::io::Result<()>) -> String {
    let mut buf = Vec::new();
    {
        let mut em = Emitter::new(&mut buf);
        em.frame(|em| f(em)).unwrap();
    }
    String::from_utf8(buf).unwrap()
}

#[test]
fn spread_basic() {
    let props = ViewProps {
        class: Some("w-64".to_string()),
        hidden: false,
        order: 0,
    };
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar {..props}
        )
    });
    byo_assert_eq!(out, +view sidebar class="w-64" order="0");
}

#[test]
fn spread_with_override() {
    let defaults = ViewProps {
        class: Some("w-64".to_string()),
        hidden: false,
        order: 0,
    };
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar {..defaults} class="override"
        )
    });
    // Spread comes first, then override. Emitter writes all props sequentially.
    byo_assert_eq!(out, +view sidebar class="w-64" order="0" class="override");
}

#[test]
fn spread_with_children() {
    let props = ViewProps {
        class: Some("flex".to_string()),
        hidden: false,
        order: 1,
    };
    let out = emit(|em| {
        byo_write!(em,
            +view root {..props} {
                +text child content="Hello"
            }
        )
    });
    byo_assert_eq!(out,
        +view root class="flex" order="1" {
            +text child content="Hello"
        }
    );
}

// ---------------------------------------------------------------------------
// Enum ReadProp / WriteProp
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, byo::ReadProp, byo::WriteProp)]
enum Variant {
    #[default]
    Primary,
    Secondary,
    Danger,
}

#[derive(Debug, Default, PartialEq, byo::ReadProp, byo::WriteProp)]
enum PascalMultiWord {
    #[default]
    PrimaryOutline,
    DangerFilled,
}

#[derive(Debug, Default, PartialEq, byo::ReadProp, byo::WriteProp)]
enum RenamedVariant {
    #[default]
    Normal,
    #[prop(rename = "cta")]
    CallToAction,
}

#[test]
fn enum_read_prop() {
    let mut v = Variant::Primary;
    v.apply(&Prop::val("variant", "danger"));
    assert_eq!(v, Variant::Danger);
}

#[test]
fn enum_read_prop_unknown_ignored() {
    let mut v = Variant::Primary;
    v.apply(&Prop::val("variant", "nonexistent"));
    assert_eq!(v, Variant::Primary);
}

#[test]
fn enum_write_prop() {
    let mut out = Vec::new();
    Variant::Secondary.encode("variant", &mut out);
    assert_eq!(out, vec![Prop::val("variant", "secondary")]);
}

#[test]
fn enum_pascal_to_kebab() {
    let mut v = PascalMultiWord::PrimaryOutline;
    v.apply(&Prop::val("style", "danger-filled"));
    assert_eq!(v, PascalMultiWord::DangerFilled);

    let mut out = Vec::new();
    PascalMultiWord::PrimaryOutline.encode("style", &mut out);
    assert_eq!(out, vec![Prop::val("style", "primary-outline")]);
}

#[test]
fn enum_rename_variant() {
    let mut v = RenamedVariant::Normal;
    v.apply(&Prop::val("kind", "cta"));
    assert_eq!(v, RenamedVariant::CallToAction);

    let mut out = Vec::new();
    RenamedVariant::CallToAction.encode("kind", &mut out);
    assert_eq!(out, vec![Prop::val("kind", "cta")]);
}

#[test]
fn enum_round_trip() {
    let original = Variant::Danger;
    let mut out = Vec::new();
    original.encode("v", &mut out);
    let mut restored = Variant::default();
    for p in &out {
        restored.apply(p);
    }
    assert_eq!(original, restored);
}

#[test]
fn enum_in_struct() {
    #[derive(Debug, PartialEq, FromProps, ToProps)]
    struct ButtonProps {
        label: String,
        variant: Variant,
    }

    let props = [Prop::val("label", "Save"), Prop::val("variant", "danger")];
    let b = ButtonProps::from_props(&props);
    assert_eq!(b.label, "Save");
    assert_eq!(b.variant, Variant::Danger);

    let encoded = b.to_props();
    assert_eq!(
        encoded,
        vec![Prop::val("label", "Save"), Prop::val("variant", "danger")]
    );
}

// ---------------------------------------------------------------------------
// Newtype ReadProp / WriteProp
// ---------------------------------------------------------------------------

#[derive(Debug, Default, PartialEq, byo::ReadProp, byo::WriteProp)]
struct Pixels(f32);

#[derive(Debug, Default, PartialEq, byo::ReadProp, byo::WriteProp)]
struct Label(String);

#[test]
fn newtype_read_prop() {
    let mut v = Pixels::default();
    v.apply(&Prop::val("width", "42.5"));
    assert_eq!(v, Pixels(42.5));
}

#[test]
fn newtype_read_prop_bad_parse() {
    let mut v = Pixels(10.0);
    v.apply(&Prop::val("width", "not-a-number"));
    assert_eq!(v, Pixels(10.0)); // unchanged
}

#[test]
fn newtype_write_prop() {
    let mut out = Vec::new();
    Pixels(16.0).encode("size", &mut out);
    assert_eq!(out, vec![Prop::val("size", "16")]);
}

#[test]
fn newtype_string() {
    let mut v = Label::default();
    v.apply(&Prop::val("title", "Hello"));
    assert_eq!(v, Label("Hello".to_string()));

    let mut out = Vec::new();
    v.encode("title", &mut out);
    assert_eq!(out, vec![Prop::val("title", "Hello")]);
}

#[test]
fn newtype_in_struct() {
    #[derive(Debug, PartialEq, FromProps, ToProps)]
    struct BoxProps {
        width: Pixels,
        height: Pixels,
    }

    let props = [Prop::val("width", "100"), Prop::val("height", "50")];
    let b = BoxProps::from_props(&props);
    assert_eq!(b.width, Pixels(100.0));
    assert_eq!(b.height, Pixels(50.0));

    let encoded = b.to_props();
    assert_eq!(
        encoded,
        vec![Prop::val("width", "100"), Prop::val("height", "50")]
    );
}
