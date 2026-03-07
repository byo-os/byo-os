//! Tests for `byo_commands!` and `byo_vec!` macros.

use byo::byo_commands;
use byo::byo_vec;
use byo::emitter::Emitter;
use byo::protocol::{Command, EventKind, PragmaKind, Prop, RequestKind, ResponseKind};

/// Serialize commands via Emitter for round-trip comparison.
fn cmds_to_string(cmds: &[Command]) -> String {
    let mut buf = Vec::new();
    let mut em = Emitter::new(&mut buf);
    em.commands(cmds).unwrap();
    String::from_utf8(buf).unwrap()
}

#[test]
fn simple_upsert() {
    let cmds = byo_vec! { +view sidebar class="w-64" order=0 };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Upsert { kind, id, props }
        if kind.as_ref() == "view"
            && id.as_ref() == "sidebar"
            && props.len() == 2
    ));
}

#[test]
fn simple_destroy() {
    let cmds = byo_vec! { -view sidebar };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Destroy { kind, id }
        if kind.as_ref() == "view" && id.as_ref() == "sidebar"
    ));
}

#[test]
fn simple_patch() {
    let cmds = byo_vec! { @view sidebar hidden ~tooltip };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Patch { kind, id, props }
        if kind.as_ref() == "view"
            && id.as_ref() == "sidebar"
            && props.len() == 2
    ));
}

#[test]
fn children_produce_push_pop() {
    let cmds = byo_vec! {
        +view parent class="p-4" {
            +view child1
            +view child2
        }
    };
    assert_eq!(cmds.len(), 5); // Upsert, Push, Upsert, Upsert, Pop
    assert!(matches!(&cmds[0], Command::Upsert { .. }));
    assert!(matches!(&cmds[1], Command::Push { slot: None }));
    assert!(matches!(&cmds[2], Command::Upsert { .. }));
    assert!(matches!(&cmds[3], Command::Upsert { .. }));
    assert!(matches!(&cmds[4], Command::Pop));
}

#[test]
fn slot_block() {
    let cmds = byo_vec! {
        +view dialog {
            ::header {
                +text title content="Settings"
            }
            ::_ {
                +view body
            }
        }
    };
    // Upsert, Push(None), Push(Some("header")), Upsert, Pop, Push(Some("_")), Upsert, Pop, Pop
    assert!(matches!(
        &cmds[2],
        Command::Push { slot: Some(name) } if name.as_ref() == "header"
    ));
    assert!(matches!(
        &cmds[5],
        Command::Push { slot: Some(name) } if name.as_ref() == "_"
    ));
}

#[test]
fn event() {
    let cmds = byo_vec! { !click 0 save };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Event { kind: EventKind::Click, seq: 0, id, .. }
        if id.as_ref() == "save"
    ));
}

#[test]
fn ack() {
    let cmds = byo_vec! { !ack click 0 handled=true };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Ack {
            kind: EventKind::Click,
            seq: 0,
            ..
        }
    ));
}

#[test]
fn pragma() {
    let cmds = byo_vec! { #claim button,slider };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Pragma(PragmaKind::Claim(targets))
        if targets.len() == 2
    ));
}

#[test]
fn observe_pragma() {
    let cmds = byo_vec! { #observe timer };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Pragma(PragmaKind::Observe(targets))
        if targets.len() == 1 && targets[0].as_ref() == "timer"
    ));
}

#[test]
fn request() {
    let cmds = byo_vec! { ?expand 0 save kind=button label="Save" };
    assert_eq!(cmds.len(), 1);
    assert!(matches!(
        &cmds[0],
        Command::Request { kind: RequestKind::Expand, seq: 0, targets, props }
        if targets.len() == 1 && props.len() == 2
    ));
}

#[test]
fn response_with_body() {
    let cmds = byo_vec! {
        .expand 0 {
            +view root class="p-4"
        }
    };
    assert_eq!(cmds.len(), 1);
    if let Command::Response {
        kind: ResponseKind::Expand,
        seq: 0,
        body: Some(body),
        ..
    } = &cmds[0]
    {
        assert_eq!(body.len(), 1);
    } else {
        panic!("expected Response::Expand with body");
    }
}

#[test]
fn interpolation() {
    let name = "sidebar";
    let width = "w-64";
    let cmds = byo_vec! { +view {name} class={width} };
    assert_eq!(cmds.len(), 1);
    if let Command::Upsert { id, props, .. } = &cmds[0] {
        assert_eq!(id.as_ref(), "sidebar");
        assert_eq!(props[0], Prop::val("class", "w-64"));
    } else {
        panic!("expected Upsert");
    }
}

#[test]
fn conditional() {
    let show = true;
    let cmds = byo_vec! {
        if show {
            +view visible
        } else {
            +view hidden
        }
    };
    assert_eq!(cmds.len(), 1);
    if let Command::Upsert { id, .. } = &cmds[0] {
        assert_eq!(id.as_ref(), "visible");
    } else {
        panic!("expected Upsert");
    }
}

#[test]
fn for_loop() {
    let cmds = byo_vec! {
        for i in 0..3 {
            +view {format!("v{i}")}
        }
    };
    assert_eq!(cmds.len(), 3);
}

#[test]
fn byo_commands_splice() {
    let cmds: Vec<Command> = vec![byo_commands! { +view a }, byo_commands! { -view b }];
    assert_eq!(cmds.len(), 2);
    assert!(matches!(&cmds[0], Command::Upsert { .. }));
    assert!(matches!(&cmds[1], Command::Destroy { .. }));
}

#[test]
fn handle_pragma() {
    let cmds = byo_vec! { #handle view?measure };
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Pragma(PragmaKind::Handle(targets)) => {
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].0.as_ref(), "view");
            assert_eq!(targets[0].1.as_ref(), "measure");
        }
        _ => panic!("expected Handle pragma"),
    }
}

#[test]
fn handle_pragma_multiple() {
    let cmds = byo_vec! { #handle view?measure,text?measure,layer?measure };
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Pragma(PragmaKind::Handle(targets)) => {
            assert_eq!(targets.len(), 3);
            assert_eq!(targets[0].0.as_ref(), "view");
            assert_eq!(targets[1].0.as_ref(), "text");
            assert_eq!(targets[2].0.as_ref(), "layer");
            // All request kinds are "measure"
            for t in targets {
                assert_eq!(t.1.as_ref(), "measure");
            }
        }
        _ => panic!("expected Handle pragma"),
    }
}

#[test]
fn unhandle_pragma() {
    let cmds = byo_vec! { #unhandle view?measure };
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Pragma(PragmaKind::Unhandle(targets)) => {
            assert_eq!(targets.len(), 1);
            assert_eq!(targets[0].0.as_ref(), "view");
            assert_eq!(targets[0].1.as_ref(), "measure");
        }
        _ => panic!("expected Unhandle pragma"),
    }
}

#[test]
fn measure_request() {
    let cmds = byo_vec! { ?measure 0 sidebar };
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Request {
            kind, seq, targets, ..
        } => {
            assert_eq!(*kind, RequestKind::Other("measure".into()));
            assert_eq!(*seq, 0);
            assert_eq!(targets[0].as_ref(), "sidebar");
        }
        _ => panic!("expected Request"),
    }
}

#[test]
fn measure_response() {
    let w = "120.0";
    let h = "80.0";
    let cmds = byo_vec! { .measure 0 width={w} height={h} };
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Response {
            kind,
            seq,
            props,
            body,
        } => {
            assert_eq!(*kind, ResponseKind::Other("measure".into()));
            assert_eq!(*seq, 0);
            assert_eq!(props[0], Prop::val("width", "120.0"));
            assert_eq!(props[1], Prop::val("height", "80.0"));
            assert!(body.is_none());
        }
        _ => panic!("expected Response"),
    }
}

#[test]
fn handle_round_trip() {
    let cmds = byo_vec! {
        #handle view?measure,text?measure
        #unhandle layer?measure
    };
    assert_eq!(cmds.len(), 2);

    let serialized = cmds_to_string(&cmds);
    let reparsed = byo::parser::parse(&serialized).unwrap();
    assert_eq!(cmds, reparsed);
}

#[test]
fn measure_round_trip() {
    let cmds = byo_vec! { .measure 42 width=100.0 height=50.0 };
    let serialized = cmds_to_string(&cmds);
    let reparsed = byo::parser::parse(&serialized).unwrap();
    assert_eq!(cmds, reparsed);
}

#[test]
fn round_trip_parity() {
    // byo_vec! → Emitter::commands() → parse() should round-trip.
    let cmds = byo_vec! {
        +view sidebar class="w-64" order=0 {
            +text label content="Hello"
        }
        -view old
        @view sidebar hidden
    };

    let serialized = cmds_to_string(&cmds);
    let reparsed = byo::parser::parse(&serialized).unwrap();
    assert_eq!(cmds, reparsed);
}

#[test]
fn multiple_commands() {
    let cmds = byo_vec! {
        +view a
        +view b
        -view c
    };
    assert_eq!(cmds.len(), 3);
}

#[test]
fn fire_event() {
    let seq = 5u64;
    let qid = "app:timer1";
    let cmds = byo_vec! {
        !fire {seq} {qid}
        -timer {qid}
    };
    assert_eq!(cmds.len(), 2);
    if let Command::Event {
        kind: EventKind::Fire,
        seq: s,
        id,
        ..
    } = &cmds[0]
    {
        assert_eq!(*s, 5);
        assert_eq!(id.as_ref(), "app:timer1");
    } else {
        panic!("expected Fire event");
    }
}
