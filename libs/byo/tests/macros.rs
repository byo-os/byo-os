use std::io;

use byo::byo_write;
use byo::emitter::Emitter;
use byo::parser::parse;

/// Helper: run byo_write! inside a frame and return the output string.
fn emit(f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> io::Result<()>) -> String {
    let mut buf = Vec::new();
    let mut em = Emitter::new(&mut buf);
    em.frame(f).unwrap();
    String::from_utf8(buf).unwrap()
}

/// Extract the payload (between APC start and ST) from emitter output.
fn payload(s: &str) -> &str {
    s.strip_prefix("\x1b_B")
        .unwrap()
        .strip_suffix("\x1b\\")
        .unwrap()
        .strip_suffix('\n')
        .unwrap()
}

// ---------------------------------------------------------------------------
// Static upsert
// ---------------------------------------------------------------------------

#[test]
fn static_upsert() {
    let out = emit(|em| byo_write!(em, +view sidebar class="w-64"));
    assert_eq!(payload(&out), "\n+view sidebar class=w-64");
}

#[test]
fn static_upsert_boolean_flag() {
    let out = emit(|em| byo_write!(em, +view sidebar hidden));
    assert_eq!(payload(&out), "\n+view sidebar hidden");
}

#[test]
fn static_upsert_no_props() {
    let out = emit(|em| byo_write!(em, +view sidebar));
    assert_eq!(payload(&out), "\n+view sidebar");
}

#[test]
fn static_upsert_anonymous() {
    let out = emit(|em| byo_write!(em, +view _));
    assert_eq!(payload(&out), "\n+view _");
}

// ---------------------------------------------------------------------------
// Children
// ---------------------------------------------------------------------------

#[test]
fn upsert_with_children() {
    let out = emit(|em| {
        byo_write!(em,
            +view root class="p-4" {
                +text label content="Hello"
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view root class=p-4 {"));
    assert!(p.contains("+text label content=Hello"));
    assert!(p.contains("}"));
}

#[test]
fn nested_children() {
    let out = emit(|em| {
        byo_write!(em,
            +view root {
                +view child {
                    +text leaf content="deep"
                }
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view root {"));
    assert!(p.contains("+view child {"));
    assert!(p.contains("+text leaf content=deep"));
}

// ---------------------------------------------------------------------------
// Destroy
// ---------------------------------------------------------------------------

#[test]
fn destroy() {
    let out = emit(|em| byo_write!(em, -view sidebar));
    assert_eq!(payload(&out), "\n-view sidebar");
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

#[test]
fn patch_with_remove() {
    let out = emit(|em| byo_write!(em, @view sidebar hidden ~tooltip));
    assert_eq!(payload(&out), "\n@view sidebar hidden ~tooltip");
}

#[test]
fn patch_with_children() {
    let out = emit(|em| {
        byo_write!(em,
            @view sidebar {
                +view item4 class="px-4"
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("@view sidebar {"));
    assert!(p.contains("+view item4 class=px-4"));
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn event() {
    let out = emit(|em| byo_write!(em, !click 0 save));
    assert_eq!(payload(&out), "\n!click 0 save");
}

#[test]
fn event_with_props() {
    let out = emit(|em| byo_write!(em, !keydown 0 editor key="a" mod="ctrl"));
    assert_eq!(payload(&out), "\n!keydown 0 editor key=a mod=ctrl");
}

#[test]
fn ack() {
    let out = emit(|em| byo_write!(em, !ack click 0 handled=true));
    assert_eq!(payload(&out), "\n!ack click 0 handled=true");
}

#[test]
fn sub_unsub() {
    let out = emit(|em| {
        byo_write!(em,
            !sub 0 button
            !unsub 1 slider
        )
    });
    let p = payload(&out);
    assert!(p.contains("!sub 0 button"));
    assert!(p.contains("!unsub 1 slider"));
}

// ---------------------------------------------------------------------------
// Interpolation
// ---------------------------------------------------------------------------

#[test]
fn interpolated_value() {
    let cls = "w-64";
    let out = emit(|em| byo_write!(em, +view sidebar class={cls}));
    assert_eq!(payload(&out), "\n+view sidebar class=w-64");
}

#[test]
fn interpolated_id() {
    let my_id = "sidebar";
    let out = emit(|em| byo_write!(em, +view {my_id}));
    assert_eq!(payload(&out), "\n+view sidebar");
}

#[test]
fn interpolated_type() {
    let kind = "layer";
    let out = emit(|em| byo_write!(em, +{kind} content));
    assert_eq!(payload(&out), "\n+layer content");
}

#[test]
fn interpolated_format_expr() {
    let n = 42;
    let out = emit(|em| byo_write!(em, +view sidebar class={format!("w-{n}")}));
    assert_eq!(payload(&out), "\n+view sidebar class=w-42");
}

// ---------------------------------------------------------------------------
// Conditional commands
// ---------------------------------------------------------------------------

#[test]
fn if_command_true() {
    let show = true;
    let out = emit(|em| {
        byo_write!(em,
            +view root {
                if show {
                    +view child
                }
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view child"));
}

#[test]
fn if_command_false() {
    let show = false;
    let out = emit(|em| {
        byo_write!(em,
            +view root {
                if show {
                    +view child
                }
            }
        )
    });
    let p = payload(&out);
    assert!(!p.contains("+view child"));
}

#[test]
fn if_else_command() {
    let loading = true;
    let out = emit(|em| {
        byo_write!(em,
            +view root {
                if loading {
                    +view spinner
                } else {
                    +view content
                }
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view spinner"));
    assert!(!p.contains("+view content"));
}

// ---------------------------------------------------------------------------
// Conditional props
// ---------------------------------------------------------------------------

#[test]
fn conditional_props_true() {
    let disabled = true;
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar class="w-64" if disabled { disabled class="opacity-50" }
        )
    });
    let p = payload(&out);
    assert!(p.contains("class=w-64")); // hmm this gets overridden; actually emitter emits both
    assert!(p.contains("disabled"));
    assert!(p.contains("class=opacity-50"));
}

#[test]
fn conditional_props_false() {
    let disabled = false;
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar class="w-64" if disabled { disabled }
        )
    });
    let p = payload(&out);
    assert!(p.contains("class=w-64"));
    assert!(!p.contains("disabled"));
}

// ---------------------------------------------------------------------------
// For loops
// ---------------------------------------------------------------------------

#[test]
fn for_loop() {
    let items = vec!["alpha", "beta", "gamma"];
    let out = emit(|em| {
        byo_write!(em,
            +view list {
                for item in &items {
                    +text _ content={*item}
                }
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("content=alpha"));
    assert!(p.contains("content=beta"));
    assert!(p.contains("content=gamma"));
}

#[test]
fn for_loop_with_format_id() {
    let ids = vec!["a", "b"];
    let out = emit(|em| {
        byo_write!(em,
            for id in &ids {
                +view {format!("item-{id}")}
            }
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view item-a"));
    assert!(p.contains("+view item-b"));
}

// ---------------------------------------------------------------------------
// Multiple commands
// ---------------------------------------------------------------------------

#[test]
fn multiple_commands() {
    let out = emit(|em| {
        byo_write!(em,
            +view a
            +view b
            -view c
        )
    });
    let p = payload(&out);
    assert!(p.contains("+view a"));
    assert!(p.contains("+view b"));
    assert!(p.contains("-view c"));
}

// ---------------------------------------------------------------------------
// String literal names (for hyphenated/dotted names)
// ---------------------------------------------------------------------------

#[test]
fn string_literal_type() {
    let out = emit(|em| byo_write!(em, +"org.example.Widget" myid label="Test"));
    let p = payload(&out);
    assert!(p.contains("+org.example.Widget myid label=Test"));
}

#[test]
fn string_literal_id() {
    let out = emit(|em| byo_write!(em, +view "my-sidebar" class="w-64"));
    let p = payload(&out);
    assert!(p.contains("+view my-sidebar class=w-64"));
}

// ---------------------------------------------------------------------------
// Round-trip: macro output → parser → verify
// ---------------------------------------------------------------------------

#[test]
fn round_trip_parse() {
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar class="w-64" {
                +text label content="Hello"
            }
            -view old
            @view sidebar hidden
            !click 0 save
            !ack click 0 handled=true
            !sub 0 button
        )
    });
    let p = payload(&out);
    let cmds = parse(p).unwrap();
    // Upsert + Push + Text + Pop + Destroy + Patch + Event + Ack + Sub = 9
    assert_eq!(cmds.len(), 9);
}

// ---------------------------------------------------------------------------
// Quoted string values (with spaces)
// ---------------------------------------------------------------------------

#[test]
fn quoted_value_with_spaces() {
    let out = emit(|em| byo_write!(em, +text label content="Hello, world"));
    let p = payload(&out);
    assert!(p.contains("content=\"Hello, world\""));
}

// ---------------------------------------------------------------------------
// Compound names (hyphenated, dotted, qualified IDs)
// ---------------------------------------------------------------------------

#[test]
fn compound_type_name() {
    let out = emit(|em| byo_write!(em, +org.example.Widget myid label="Test"));
    let p = payload(&out);
    assert!(p.contains("+org.example.Widget myid label=Test"));
}

#[test]
fn compound_id_with_hyphen() {
    let out = emit(|em| byo_write!(em, +view my-sidebar class="w-64"));
    let p = payload(&out);
    assert!(p.contains("+view my-sidebar class=w-64"));
}

#[test]
fn qualified_id() {
    let out = emit(|em| byo_write!(em, +view notes-app:save));
    let p = payload(&out);
    assert!(p.contains("+view notes-app:save"));
}

#[test]
fn compound_prop_value() {
    let out = emit(|em| byo_write!(em, +view sidebar class=w-64));
    let p = payload(&out);
    assert!(p.contains("class=w-64"));
}

#[test]
fn tailwind_slash_value() {
    let out = emit(|em| byo_write!(em, +view sidebar class=bg-zinc-700/50));
    let p = payload(&out);
    assert!(p.contains("class=bg-zinc-700/50"));
}

#[test]
fn destroy_compound_id() {
    let out = emit(|em| byo_write!(em, -view my-sidebar));
    let p = payload(&out);
    assert!(p.contains("-view my-sidebar"));
}

#[test]
fn patch_qualified_id() {
    let out = emit(|em| byo_write!(em, @view notes-app:save hidden));
    let p = payload(&out);
    assert!(p.contains("@view notes-app:save hidden"));
}

// ---------------------------------------------------------------------------
// byo! macro (stdout framing)
// ---------------------------------------------------------------------------

// We can't easily capture stdout in a test, but we can verify it compiles.
// The actual stdout test would need process-level capture.

#[test]
fn byo_macro_compiles() {
    // Just verify the macro expands without error.
    // We redirect stdout to avoid actual output.
    // This test primarily checks compilation.
    fn _check() {
        byo::byo! {
            +view sidebar class="w-64" {
                +text label content="Hello"
            }
        };
    }
}
