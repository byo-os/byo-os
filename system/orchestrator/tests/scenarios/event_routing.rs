use byo::protocol::{Command, EventKind, Prop, RequestKind, ResponseKind};

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 1: Compositor sends a click event targeting an app's object.
///
/// Expected flow:
/// 1. App creates `+view btn` (state tree gets app:btn owned by app)
/// 2. Compositor sends `!click 0 app:btn`
/// 3. Router qualifies target, looks up owner → app (PID 2)
/// 4. Remaps seq (first event → 0), dequalifies target → `btn`
/// 5. App receives `!click 0 btn`
#[tokio::test]
async fn event_basic() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // App creates an object.
    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx); // consume the upsert

    // Compositor sends a click event targeting the app's object.
    send_byo(&mut router, pid(1), "!click 0 app:btn").await;

    // App should receive the event with dequalified target.
    let s = recv_byo_raw(&mut app_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got {}: {s}", cmds.len());
    assert!(
        matches!(&cmds[0], Command::Event { kind: EventKind::Click, seq, id, .. }
            if *seq == 0 && *id == "btn"),
        "expected !click 0 btn, got: {s}"
    );
}

/// Test 2: Two events to the same app get incrementing seq numbers.
///
/// Expected flow:
/// 1. Compositor sends `!click 0 app:btn` → app gets seq 0
/// 2. Compositor sends `!click 1 app:btn` → app gets seq 1
#[tokio::test]
async fn event_seq_remapping() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // First event.
    send_byo(&mut router, pid(1), "!click 0 app:btn").await;
    let s1 = recv_byo_raw(&mut app_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert!(
        matches!(&cmds1[0], Command::Event { seq, .. } if *seq == 0),
        "first event should have seq 0, got: {s1}"
    );

    // Second event.
    send_byo(&mut router, pid(1), "!click 1 app:btn").await;
    let s2 = recv_byo_raw(&mut app_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert!(
        matches!(&cmds2[0], Command::Event { seq, .. } if *seq == 1),
        "second event should have seq 1, got: {s2}"
    );
}

/// Test 3: Events to different apps get independent seq counters.
///
/// Expected flow:
/// 1. Compositor sends `!click 0 app1:btn` → app1 gets seq 0
/// 2. Compositor sends `!click 1 app2:btn` → app2 also gets seq 0 (independent)
#[tokio::test]
async fn event_per_destination_isolation() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app1, mut app1_rx) = mock_process(2, "app1");
    let (app2, mut app2_rx) = mock_process(3, "app2");
    router.add_process(compositor);
    router.add_process(app1);
    router.add_process(app2);

    send_byo(&mut router, pid(1), "?observe 0 view").await;

    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    send_byo(&mut router, pid(3), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Event to app1.
    send_byo(&mut router, pid(1), "!click 0 app1:btn").await;
    let s1 = recv_byo_raw(&mut app1_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert!(
        matches!(&cmds1[0], Command::Event { seq, .. } if *seq == 0),
        "app1 should get seq 0, got: {s1}"
    );

    // Event to app2 — should also start at seq 0.
    send_byo(&mut router, pid(1), "!click 1 app2:btn").await;
    let s2 = recv_byo_raw(&mut app2_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert!(
        matches!(&cmds2[0], Command::Event { seq, .. } if *seq == 0),
        "app2 should get seq 0 (independent counter), got: {s2}"
    );
}

/// Test 4: App ACKs an event, original sender receives ACK with original seq.
///
/// Expected flow:
/// 1. Compositor sends `!click 5 app:btn` (compositor's seq 5)
/// 2. App receives `!click 0 btn` (remapped seq)
/// 3. App sends `!ack click 0`
/// 4. Compositor receives `!ack click 5` (original seq restored)
#[tokio::test]
async fn ack_routing() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Compositor sends event with seq 5.
    send_byo(&mut router, pid(1), "!click 5 app:btn").await;
    let s = recv_byo_raw(&mut app_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    let remapped_seq = match &cmds[0] {
        Command::Event { seq, .. } => *seq,
        _ => panic!("expected event, got: {s}"),
    };

    // App ACKs with the remapped seq.
    send_byo(
        &mut router,
        pid(2),
        &format!("!ack click {remapped_seq} handled=true"),
    )
    .await;

    // Compositor should receive ACK with original seq 5.
    let ack_s = recv_byo_raw(&mut compositor_rx);
    let ack_cmds = byo::parser::parse(&ack_s).unwrap();
    assert_eq!(ack_cmds.len(), 1, "expected 1 ack command, got: {ack_s}");
    assert!(
        matches!(&ack_cmds[0], Command::Ack { kind: EventKind::Click, seq, props }
            if *seq == 5
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "handled" && *value == "true"))
        ),
        "expected !ack click 5 handled=true, got: {ack_s}"
    );
}

/// Test 5: The `handled` prop (true/false) is forwarded correctly in ACKs.
#[tokio::test]
async fn ack_with_handled_prop() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    send_byo(&mut router, pid(1), "!click 0 app:btn").await;
    let _ = recv_byo_raw(&mut app_rx);

    // ACK with handled=false.
    send_byo(&mut router, pid(2), "!ack click 0 handled=false").await;

    let ack_s = recv_byo_raw(&mut compositor_rx);
    let ack_cmds = byo::parser::parse(&ack_s).unwrap();
    assert!(
        matches!(&ack_cmds[0], Command::Ack { kind: EventKind::Click, seq, props }
            if *seq == 0
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "handled" && *value == "false"))
        ),
        "expected !ack click 0 handled=false, got: {ack_s}"
    );
}

/// Test 6: Event targeting a non-existent object is silently dropped.
#[tokio::test]
async fn event_target_not_found() {
    let mut router = Router::new();
    let (compositor, mut _compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Compositor sends event for an object that doesn't exist.
    send_byo(&mut router, pid(1), "!click 0 app:nonexistent").await;

    // App should NOT receive anything.
    assert_no_message(&mut app_rx);
}

/// Test 7: Daemon sends event targeting another daemon's expansion child,
/// routes to the correct owner.
///
/// Expected flow:
/// 1. App creates `+button save label=Save` → controls daemon expands
/// 2. Controls daemon creates expansion children owned by controls
/// 3. Another process sends event targeting `controls:save-root`
/// 4. Event routes to controls daemon (PID 3)
#[tokio::test]
async fn event_cross_daemon() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    let (controls, mut controls_rx) = mock_process(3, "controls");
    router.add_process(compositor);
    router.add_process(app);
    router.add_process(controls);

    send_byo(&mut router, pid(1), "?observe 0 view,text").await;
    send_byo(&mut router, pid(3), "?claim 0 button").await;

    // App creates a button (triggers expansion).
    send_byo(&mut router, pid(2), "+button save label=Save").await;

    // Controls daemon receives the expand request.
    let expand_msg = recv_byo_raw(&mut controls_rx);
    assert!(
        expand_msg.contains("?expand"),
        "expected expand request, got: {expand_msg}"
    );

    // Controls daemon responds with expansion.
    send_byo(
        &mut router,
        pid(3),
        ".expand 0 { +view save-root class=btn { +text save-label content=Save } }",
    )
    .await;

    // Consume compositor's rewritten output.
    let _ = recv_byo_raw(&mut compositor_rx);

    // Now compositor sends a click event targeting the expansion child.
    send_byo(&mut router, pid(1), "!click 0 controls:save-root").await;

    // Controls daemon should receive the event with dequalified target.
    let s = recv_byo_raw(&mut controls_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(&cmds[0], Command::Event { kind: EventKind::Click, seq, id, .. }
            if *seq == 0 && *id == "save-root"),
        "expected !click 0 save-root, got: {s}"
    );
}

/// Test 8: Pending ACKs are cleaned up when a process disconnects.
///
/// Expected flow:
/// 1. Compositor sends `!click 0 app:btn` → routed to app
/// 2. App disconnects before ACKing
/// 3. No crash, pending ACK is cleaned up
/// 4. Compositor receives destroy notification (not a stale ACK)
#[tokio::test]
async fn disconnect_cleans_pending_acks() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view btn").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Compositor sends event.
    send_byo(&mut router, pid(1), "!click 0 app:btn").await;
    let _ = recv_byo_raw(&mut app_rx); // app receives the event

    // App disconnects before ACKing.
    router
        .handle(byo_orchestrator::router::RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Compositor receives destroy notification for the app's object.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&destroy_s).unwrap();
    assert!(
        matches!(&cmds[0], Command::Destroy { kind, id } if *kind == "view" && *id == "app:btn"),
        "expected -view app:btn, got: {destroy_s}"
    );

    // No stale ACK — just the destroy notification above.
    assert_no_message(&mut compositor_rx);
}

/// Test 9: Custom request is routed to the target object's owner.
///
/// Expected flow:
/// 1. App creates `+view surface`
/// 2. Compositor sends `?render-frame 0 app:surface` targeting app's object
/// 3. App receives `?render-frame 0 surface` (remapped seq, dequalified target)
#[tokio::test]
async fn request_basic() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view surface").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Compositor sends a custom request targeting the app's object.
    send_byo(&mut router, pid(1), "?render-frame 0 app:surface").await;

    let s = recv_byo_raw(&mut app_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(&cmds[0], Command::Request { kind: RequestKind::Other(k), seq, targets, .. }
            if *k == "render-frame" && *seq == 0 && targets.first().map(|t| &**t) == Some("surface")),
        "expected ?render-frame 0 surface, got: {s}"
    );
}

/// Test 10: Custom response is routed back with original seq.
///
/// Expected flow:
/// 1. Compositor sends `?render-frame 3 app:surface`
/// 2. App receives `?render-frame 0 surface` (remapped)
/// 3. App responds `.render-frame 0 status=ok`
/// 4. Compositor receives `.render-frame 3 status=ok` (original seq)
#[tokio::test]
async fn response_routing() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view surface").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Compositor sends request with seq 3.
    send_byo(&mut router, pid(1), "?render-frame 3 app:surface").await;
    let s = recv_byo_raw(&mut app_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    let remapped_seq = match &cmds[0] {
        Command::Request { seq, .. } => *seq,
        _ => panic!("expected request, got: {s}"),
    };

    // App responds with the remapped seq.
    send_byo(
        &mut router,
        pid(2),
        &format!(".render-frame {remapped_seq} status=ok"),
    )
    .await;

    // Compositor should receive response with original seq 3.
    let resp_s = recv_byo_raw(&mut compositor_rx);
    let resp_cmds = byo::parser::parse(&resp_s).unwrap();
    assert_eq!(resp_cmds.len(), 1, "expected 1 response, got: {resp_s}");
    assert!(
        matches!(&resp_cmds[0], Command::Response { kind: ResponseKind::Other(k), seq, props, .. }
            if *k == "render-frame" && *seq == 3
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "status" && *value == "ok"))
        ),
        "expected .render-frame 3 status=ok, got: {resp_s}"
    );
}

/// Test 11: Custom response with body is forwarded correctly.
#[tokio::test]
async fn response_with_body() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view surface").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    send_byo(&mut router, pid(1), "?snapshot 0 app:surface").await;
    let _ = recv_byo_raw(&mut app_rx);

    // App responds with a body.
    send_byo(
        &mut router,
        pid(2),
        ".snapshot 0 { +view frame class=captured }",
    )
    .await;

    let resp_s = recv_byo_raw(&mut compositor_rx);
    let resp_cmds = byo::parser::parse(&resp_s).unwrap();
    assert_eq!(resp_cmds.len(), 1, "expected 1 response, got: {resp_s}");
    match &resp_cmds[0] {
        Command::Response {
            kind: ResponseKind::Other(k),
            seq,
            body: Some(body),
            ..
        } => {
            assert_eq!(*k, "snapshot");
            assert_eq!(*seq, 0);
            assert!(
                matches!(&body[0], Command::Upsert { kind, id, .. }
                    if *kind == "view" && *id == "frame"),
                "expected body +view frame, got: {resp_s}"
            );
        }
        _ => panic!("expected response with body, got: {resp_s}"),
    }
}

/// Test 12: Request seq isolation — different destinations get independent counters.
#[tokio::test]
async fn request_seq_isolation() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app1, mut app1_rx) = mock_process(2, "app1");
    let (app2, mut app2_rx) = mock_process(3, "app2");
    router.add_process(compositor);
    router.add_process(app1);
    router.add_process(app2);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view s1").await;
    let _ = recv_byo_raw(&mut compositor_rx);
    send_byo(&mut router, pid(3), "+view s2").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // Request to app1.
    send_byo(&mut router, pid(1), "?render-frame 0 app1:s1").await;
    let s1 = recv_byo_raw(&mut app1_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert!(
        matches!(&cmds1[0], Command::Request { seq, .. } if *seq == 0),
        "app1 should get seq 0, got: {s1}"
    );

    // Request to app2 — should also start at seq 0.
    send_byo(&mut router, pid(1), "?render-frame 1 app2:s2").await;
    let s2 = recv_byo_raw(&mut app2_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert!(
        matches!(&cmds2[0], Command::Request { seq, .. } if *seq == 0),
        "app2 should get seq 0 (independent), got: {s2}"
    );
}

/// Test 13: Pending responses are cleaned up on disconnect.
#[tokio::test]
async fn disconnect_cleans_pending_responses() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view surface").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    send_byo(&mut router, pid(1), "?render-frame 0 app:surface").await;
    let _ = recv_byo_raw(&mut app_rx);

    // App disconnects before responding.
    router
        .handle(byo_orchestrator::router::RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Compositor receives destroy notification for the app's object.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&destroy_s).unwrap();
    assert!(
        matches!(&cmds[0], Command::Destroy { kind, id } if *kind == "view" && *id == "app:surface"),
        "expected -view app:surface, got: {destroy_s}"
    );

    // No stale response — just the destroy notification above.
    assert_no_message(&mut compositor_rx);
}

/// Test 14: Request with props is forwarded correctly.
#[tokio::test]
async fn request_with_props() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "+view surface").await;
    let _ = recv_byo_raw(&mut compositor_rx);

    send_byo(
        &mut router,
        pid(1),
        "?render-frame 0 app:surface format=png quality=high",
    )
    .await;

    let s = recv_byo_raw(&mut app_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    match &cmds[0] {
        Command::Request {
            kind: RequestKind::Other(k),
            props,
            ..
        } => {
            assert_eq!(*k, "render-frame");
            assert!(
                props
                    .iter()
                    .any(|p| matches!(p, Prop::Value { key, value } if *key == "format" && *value == "png")),
                "expected format=png, got: {s}"
            );
            assert!(
                props
                    .iter()
                    .any(|p| matches!(p, Prop::Value { key, value } if *key == "quality" && *value == "high")),
                "expected quality=high, got: {s}"
            );
        }
        _ => panic!("expected request, got: {s}"),
    }
}
