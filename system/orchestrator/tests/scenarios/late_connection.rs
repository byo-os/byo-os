use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 17: Late claim replays reduced state as `?expand` to the daemon.
///
/// Expected message flow:
/// 1. App sends `+button save label="Save"` -- no daemon has claimed `button`
/// 2. No claim exists, so button is treated as native and forwarded to any
///    observers. (There are none in this test, so nothing is delivered.)
/// 3. Controls daemon connects and claims `button` via `?claim 0 button`.
/// 4. Router replays reduced state for all `button` objects: sends a
///    `?expand` message to the controls daemon containing the reduced state.
///    The message format from handle_claim is:
///    `?expand SEQ <reduced_command_without_leading_+>`
///    which serializes as: `?expand 0 button app:save label=Save`
///    (Note: this replay uses the reduced_command format and sends directly
///    via send_to without creating a PendingBatch.)
/// 5. Verify the controls daemon receives the ?expand replay containing
///    the object type, qualified ID, and props.
#[tokio::test]
async fn late_claim_replays() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    router.add_process(app);
    router.add_process(controls);

    // Step 1: App sends a button before any daemon claims it.
    // No observers, so this just updates internal state.
    send_byo(&mut router, pid(1), "+button save label=\"Save\"").await;

    // No messages should have been sent to controls yet.
    assert_no_message(&mut controls_rx);

    // Step 3: Controls daemon claims `button`.
    send_byo(&mut router, pid(2), "#claim button").await;

    // Step 4: Router should replay reduced state as `?expand` to controls.
    //   `\n?expand SEQ qid kind=type props...`
    let s = recv_byo_raw(&mut controls_rx);
    assert_eq!(s, "\n?expand 0 app:save kind=button label=Save");

    // No further messages.
    assert_no_message(&mut controls_rx);
}

/// Test 18: Late observer receives resync of existing state tree.
///
/// Expected message flow:
/// 1. App sends `+view sidebar class="w-64"` -- no observers yet.
///    State tree is updated but nothing is delivered.
/// 2. Compositor connects and observes `view` via `?observe 0 view`.
/// 3. Router calls `resync_observer()`, which projects the full state tree
///    filtered by the observer's types. Since sidebar is a view, it's included.
/// 4. Compositor receives `+view app:sidebar class="w-64"`.
#[tokio::test]
async fn late_observer_resync() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (compositor, mut compositor_rx) = mock_process(2, "compositor");
    router.add_process(app);
    router.add_process(compositor);

    // Step 1: App sends a view before anyone observes.
    send_byo(&mut router, pid(1), "+view sidebar class=\"w-64\"").await;

    // No messages yet -- no observers.
    assert_no_message(&mut compositor_rx);

    // Step 2: Compositor observes `view`.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 3-4: Router resyncs -- compositor gets the full state tree replay.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain the reduced upsert for sidebar.
    let upsert = cmds.iter().find(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
                if *kind == "view" && *id == "app:sidebar")
    });
    assert!(
        upsert.is_some(),
        "expected +view app:sidebar in resync, got: {s}"
    );

    // Verify props include class="w-64".
    if let Some(Command::Upsert { props, .. }) = upsert {
        let has_class = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "w-64")
        });
        assert!(has_class, "expected class=w-64 in props, got: {s}");
    }

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 19: Additional observe triggers a resync with expanded type coverage.
///
/// Expected message flow:
/// 1. Compositor observes `view`.
/// 2. App sends `+view root { +text label content="Hello" }`.
/// 3. Compositor receives the view, but NOT the text child (text is not observed).
///    The forwarded message is `+view app:root` (no children since text is filtered).
/// 4. Compositor additionally observes `text` via `?observe 1 text`.
/// 5. Router resyncs: compositor gets full projected tree including both
///    `+view app:root` and `+text app:label` as a tree structure.
/// 6. Verify resync includes both types properly nested.
#[tokio::test]
async fn additional_observe_resync() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Step 1: Compositor observes view only.
    send_byo(&mut router, pid(1), "#observe view").await;

    // Step 2: App sends a view with a text child.
    send_byo(
        &mut router,
        pid(2),
        "+view root { +text label content=\"Hello\" }",
    )
    .await;

    // Step 3: Compositor receives the view but NOT the text child.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:root")),
        "expected +view app:root, got: {s}"
    );
    // text should NOT be present since it's not observed.
    assert!(
        !cmds
            .iter()
            .any(|c| matches!(c, Command::Upsert { kind, .. } if *kind == "text")),
        "text should not be present when not observed, got: {s}"
    );

    // Step 4: Compositor additionally observes `text`.
    send_byo(&mut router, pid(1), "#observe text").await;

    // Step 5: Router resyncs with both view and text.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain both view and text.
    let has_view = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:root")
    });
    let has_text = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "text" && *id == "app:label")
    });
    assert!(has_view, "resync should include +view app:root, got: {s}");
    assert!(has_text, "resync should include +text app:label, got: {s}");

    // Verify text has content prop.
    let text_cmd = cmds
        .iter()
        .find(|c| matches!(c, Command::Upsert { kind, .. } if *kind == "text"))
        .unwrap();
    if let Command::Upsert { props, .. } = text_cmd {
        let has_content = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "content" && value.as_ref() == "Hello")
        });
        assert!(
            has_content,
            "expected content=Hello in text props, got: {s}"
        );
    }

    // Verify tree structure: text should be nested under view (Push/Pop present).
    assert!(
        cmds.iter().any(|c| matches!(c, Command::Push)),
        "expected Push (children block) in resync, got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}
