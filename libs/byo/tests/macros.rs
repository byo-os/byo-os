use std::io;

use byo::byo_assert_eq;
use byo::byo_write;
use byo::emitter::Emitter;
use byo::parser::parse;
use byo::strip_apc;

/// Helper: run byo_write! inside a frame and return the output string.
fn emit(f: impl FnOnce(&mut Emitter<&mut Vec<u8>>) -> io::Result<()>) -> String {
    let mut buf = Vec::new();
    let mut em = Emitter::new(&mut buf);
    em.frame(f).unwrap();
    String::from_utf8(buf).unwrap()
}

// ---------------------------------------------------------------------------
// Static upsert
// ---------------------------------------------------------------------------

#[test]
fn static_upsert() {
    let out = emit(|em| byo_write!(em, +view sidebar class="w-64"));
    assert_eq!(strip_apc(&out), "\n+view sidebar class=w-64");
}

#[test]
fn static_upsert_boolean_flag() {
    let out = emit(|em| byo_write!(em, +view sidebar hidden));
    assert_eq!(strip_apc(&out), "\n+view sidebar hidden");
}

#[test]
fn static_upsert_no_props() {
    let out = emit(|em| byo_write!(em, +view sidebar));
    assert_eq!(strip_apc(&out), "\n+view sidebar");
}

#[test]
fn static_upsert_anonymous() {
    let out = emit(|em| byo_write!(em, +view _));
    assert_eq!(strip_apc(&out), "\n+view _");
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
    byo_assert_eq!(out,
        +view root class="p-4" {
            +text label content="Hello"
        }
    );
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
    byo_assert_eq!(out,
        +view root {
            +view child {
                +text leaf content="deep"
            }
        }
    );
}

// ---------------------------------------------------------------------------
// Destroy
// ---------------------------------------------------------------------------

#[test]
fn destroy() {
    let out = emit(|em| byo_write!(em, -view sidebar));
    assert_eq!(strip_apc(&out), "\n-view sidebar");
}

// ---------------------------------------------------------------------------
// Patch
// ---------------------------------------------------------------------------

#[test]
fn patch_with_remove() {
    let out = emit(|em| byo_write!(em, @view sidebar hidden ~tooltip));
    assert_eq!(strip_apc(&out), "\n@view sidebar hidden ~tooltip");
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
    byo_assert_eq!(out,
        @view sidebar {
            +view item4 class="px-4"
        }
    );
}

// ---------------------------------------------------------------------------
// Events
// ---------------------------------------------------------------------------

#[test]
fn event() {
    let out = emit(|em| byo_write!(em, !click 0 save));
    assert_eq!(strip_apc(&out), "\n!click 0 save");
}

#[test]
fn event_with_props() {
    let out = emit(|em| byo_write!(em, !keydown 0 editor key="a" mod="ctrl"));
    assert_eq!(strip_apc(&out), "\n!keydown 0 editor key=a mod=ctrl");
}

#[test]
fn ack() {
    let out = emit(|em| byo_write!(em, !ack click 0 handled=true));
    assert_eq!(strip_apc(&out), "\n!ack click 0 handled=true");
}

#[test]
fn claim_unclaim() {
    let out = emit(|em| {
        byo_write!(em,
            #claim button
            #unclaim slider
        )
    });
    byo_assert_eq!(out,
        #claim button
        #unclaim slider
    );
}

#[test]
fn observe_unobserve() {
    let out = emit(|em| {
        byo_write!(em,
            #observe view
            #unobserve text
        )
    });
    byo_assert_eq!(out,
        #observe view
        #unobserve text
    );
}

#[test]
fn redirect_unredirect() {
    let out = emit(|em| {
        byo_write!(em,
            #redirect term
        )
    });
    byo_assert_eq!(out,
        #redirect term
    );
    let out2 = emit(|em| {
        byo_write!(em,
            #unredirect
        )
    });
    byo_assert_eq!(out2,
        #unredirect
    );
}

#[test]
fn request_expand() {
    let out = emit(|em| byo_write!(em, ?expand 0 save kind=button label="Save"));
    byo_assert_eq!(out, ?expand 0 save kind=button label="Save");
}

#[test]
fn response_expand() {
    let out = emit(|em| {
        byo_write!(em,
            .expand 0 {
                +view root class="btn" {
                    +text label content="Save"
                }
            }
        )
    });
    byo_assert_eq!(out,
        .expand 0 {
            +view root class="btn" {
                +text label content="Save"
            }
        }
    );
}

#[test]
fn generic_request() {
    let out = emit(|em| byo_write!(em, ?"render-frame" 0 viewport));
    byo_assert_eq!(out, ?"render-frame" 0 viewport);
}

#[test]
fn generic_response_no_body() {
    let out = emit(|em| byo_write!(em, ."render-frame" 0 status=ok));
    byo_assert_eq!(out, ."render-frame" 0 status=ok);
}

#[test]
fn generic_response_with_body() {
    let out = emit(|em| {
        byo_write!(em,
            ."render-frame" 0 status=ok {
                +view frame
            }
        )
    });
    byo_assert_eq!(out,
        ."render-frame" 0 status=ok {
            +view frame
        }
    );
}

// ---------------------------------------------------------------------------
// Interpolation
// ---------------------------------------------------------------------------

#[test]
fn interpolated_value() {
    let cls = "w-64";
    let out = emit(|em| byo_write!(em, +view sidebar class={cls}));
    assert_eq!(strip_apc(&out), "\n+view sidebar class=w-64");
}

#[test]
fn interpolated_id() {
    let my_id = "sidebar";
    let out = emit(|em| byo_write!(em, +view {my_id}));
    assert_eq!(strip_apc(&out), "\n+view sidebar");
}

#[test]
fn interpolated_type() {
    let kind = "layer";
    let out = emit(|em| byo_write!(em, +{kind} content));
    assert_eq!(strip_apc(&out), "\n+layer content");
}

#[test]
fn interpolated_format_expr() {
    let n = 42;
    let out = emit(|em| byo_write!(em, +view sidebar class={format!("w-{n}")}));
    assert_eq!(strip_apc(&out), "\n+view sidebar class=w-42");
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
    byo::assert::assert_eq(&out, "\n+view root {\n+view child\n}");
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
    byo::assert::assert_eq(&out, "\n+view root {\n}");
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
    byo::assert::assert_eq(&out, "\n+view root {\n+view spinner\n}");
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
    byo::assert::assert_eq(&out, "\n+view sidebar class=w-64 disabled class=opacity-50");
}

#[test]
fn conditional_props_false() {
    let disabled = false;
    let out = emit(|em| {
        byo_write!(em,
            +view sidebar class="w-64" if disabled { disabled }
        )
    });
    byo::assert::assert_eq(&out, "\n+view sidebar class=w-64");
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
    byo::assert::assert_eq(
        &out,
        "\n+view list {\n+text _ content=alpha\n+text _ content=beta\n+text _ content=gamma\n}",
    );
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
    byo::assert::assert_eq(&out, "\n+view item-a\n+view item-b");
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
    byo_assert_eq!(out,
        +view a
        +view b
        -view c
    );
}

// ---------------------------------------------------------------------------
// String literal names (for hyphenated/dotted names)
// ---------------------------------------------------------------------------

#[test]
fn string_literal_type() {
    let out = emit(|em| byo_write!(em, +"org.example.Widget" myid label="Test"));
    byo_assert_eq!(out, +org.example.Widget myid label="Test");
}

#[test]
fn string_literal_id() {
    let out = emit(|em| byo_write!(em, +view "my-sidebar" class="w-64"));
    byo_assert_eq!(out, +view my-sidebar class="w-64");
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
            #claim button
        )
    });
    let cmds = parse(strip_apc(&out)).unwrap();
    // Upsert + Push + Text + Pop + Destroy + Patch + Event + Ack + Pragma = 9
    assert_eq!(cmds.len(), 9);
}

// ---------------------------------------------------------------------------
// Quoted string values (with spaces)
// ---------------------------------------------------------------------------

#[test]
fn quoted_value_with_spaces() {
    let out = emit(|em| byo_write!(em, +text label content="Hello, world"));
    byo_assert_eq!(out, +text label content="Hello, world");
}

// ---------------------------------------------------------------------------
// Compound names (hyphenated, dotted, qualified IDs)
// ---------------------------------------------------------------------------

#[test]
fn compound_type_name() {
    let out = emit(|em| byo_write!(em, +org.example.Widget myid label="Test"));
    byo_assert_eq!(out, +org.example.Widget myid label="Test");
}

#[test]
fn compound_id_with_hyphen() {
    let out = emit(|em| byo_write!(em, +view my-sidebar class="w-64"));
    byo_assert_eq!(out, +view my-sidebar class="w-64");
}

#[test]
fn qualified_id() {
    let out = emit(|em| byo_write!(em, +view notes-app:save));
    byo_assert_eq!(out, +view notes-app:save);
}

#[test]
fn compound_prop_value() {
    let out = emit(|em| byo_write!(em, +view sidebar class=w-64));
    byo_assert_eq!(out, +view sidebar class=w-64);
}

#[test]
fn tailwind_slash_value() {
    let out = emit(|em| byo_write!(em, +view sidebar class=bg-zinc-700/50));
    byo_assert_eq!(out, +view sidebar class=bg-zinc-700/50);
}

#[test]
fn destroy_compound_id() {
    let out = emit(|em| byo_write!(em, -view my-sidebar));
    byo_assert_eq!(out, -view my-sidebar);
}

#[test]
fn patch_qualified_id() {
    let out = emit(|em| byo_write!(em, @view notes-app:save hidden));
    byo_assert_eq!(out, @view notes-app:save hidden);
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
