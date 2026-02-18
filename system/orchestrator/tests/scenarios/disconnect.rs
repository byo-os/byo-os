use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::{Router, RouterMsg};

/// Test 38: App disconnect notifies compositor and cleans state.
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. App sends `+view sidebar class="w-64"`, compositor receives it
/// 3. App disconnects via `RouterMsg::Disconnected { process: app_pid }`
/// 4. Compositor receives `-view app:sidebar` (destroy notification)
/// 5. State tree is cleaned up
/// 6. A new observer registers — it should get NO resync for disconnected app
#[tokio::test]
async fn app_disconnect_cleans_state() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Step 2: App sends a view upsert.
    send_byo(&mut router, pid(2), "+view sidebar class=\"w-64\"").await;

    // Compositor receives the upsert.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1);
    assert!(
        matches!(
            &cmds[0],
            Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:sidebar"
        ),
        "expected +view app:sidebar, got: {s}"
    );

    // Step 3: App disconnects.
    router
        .handle(RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Step 4: Compositor receives destroy notification.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();
    assert_eq!(
        destroy_cmds.len(),
        1,
        "expected 1 destroy command, got: {destroy_s}"
    );
    assert!(
        matches!(
            &destroy_cmds[0],
            Command::Destroy { kind, id } if *kind == "view" && *id == "app:sidebar"
        ),
        "expected -view app:sidebar, got: {destroy_s}"
    );

    // Step 5: Register a NEW observer. State was cleaned, so no resync.
    let (a11y, mut a11y_rx) = mock_process(3, "a11y");
    router.add_process(a11y);
    send_byo(&mut router, pid(3), "?observe 0 view").await;

    assert_no_message(&mut a11y_rx);
}

/// Test 39: Daemon disconnect removes claims — subsequent app commands bypass expansion.
///
/// Expected message flow:
/// 1. Compositor observes `view,button`
/// 2. Controls daemon claims `button`
/// 3. Controls disconnects via `RouterMsg::Disconnected { process: controls_pid }`
/// 4. Router removes the claim for `button`
/// 5. App sends `+button save label="Save"` — button is no longer claimed
/// 6. Compositor receives `+button app:save label="Save"` as native (no expansion)
#[tokio::test]
async fn daemon_disconnect_removes_claims() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut _controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    // Step 1: Compositor observes view and button.
    send_byo(&mut router, pid(1), "?observe 0 view,button").await;

    // Step 2: Controls daemon claims button.
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // Step 3: Controls disconnects.
    router
        .handle(RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Step 4: Claim for button should be removed.

    // Step 5: App sends a button. Since button is no longer claimed, no expansion occurs.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Step 6: Compositor should receive the button as a native type with qualified ID.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got {}: {s}", cmds.len());
    assert!(
        matches!(
            &cmds[0],
            Command::Upsert { kind, id, props }
            if *kind == "button" && *id == "app:save"
               && props.iter().any(|p| matches!(p, byo::Prop::Value { key, value } if *key == "label" && value.as_ref() == "Save"))
        ),
        "expected +button app:save label=Save, got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 40: App disconnect with expansion — compositor receives destroys for
/// expansion children, and daemon is notified.
///
/// Expected flow:
/// 1. Controls daemon claims `button`, compositor observes `view,text`
/// 2. App creates `+button save label="Save"` → daemon expands
/// 3. App disconnects
/// 4. Compositor receives `-view controls:save-root` (expansion child destroy)
/// 5. Controls daemon receives destroy notifications for its expansion objects
#[tokio::test]
async fn app_disconnect_with_expansion_notifies_observers() {
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

    // Controls daemon receives expand request.
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

    // App disconnects.
    router
        .handle(RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Compositor should receive destroy for the expansion objects.
    // The source `-button app:save` is not observed (compositor observes view,text),
    // so projection emits destroys for observed descendants.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();

    // Should have destroys for observed descendants of the button.
    // Deepest first: -text controls:save-label, then -view controls:save-root.
    let destroy_kinds: Vec<(&str, &str)> = destroy_cmds
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Destroy { kind, id } => Some((&**kind, &**id)),
            _ => None,
        })
        .collect();

    assert!(
        destroy_kinds
            .iter()
            .any(|(k, id)| *k == "view" && *id == "controls:save-root"),
        "expected -view controls:save-root in destroys, got: {destroy_s}"
    );
    assert!(
        destroy_kinds
            .iter()
            .any(|(k, id)| *k == "text" && *id == "controls:save-label"),
        "expected -text controls:save-label in destroys, got: {destroy_s}"
    );

    // Controls daemon should be notified about its expansion objects.
    let daemon_msg = recv_byo_raw(&mut controls_rx);
    let daemon_cmds = byo::parser::parse(&daemon_msg).unwrap();
    let daemon_destroys: Vec<(&str, &str)> = daemon_cmds
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Destroy { kind, id } => Some((&**kind, &**id)),
            _ => None,
        })
        .collect();

    assert!(
        daemon_destroys
            .iter()
            .any(|(k, id)| *k == "view" && *id == "controls:save-root"),
        "daemon should be notified about -view controls:save-root, got: {daemon_msg}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 41: Daemon disconnect removes expansion objects from compositor.
///
/// Expected flow:
/// 1. Controls daemon claims `button`, compositor observes `view,text`
/// 2. App creates `+button save label="Save"` → daemon expands
/// 3. Controls daemon disconnects
/// 4. Compositor receives destroys for daemon's expansion objects
/// 5. App's source object (`button save`) remains in state for re-expansion
#[tokio::test]
async fn daemon_disconnect_removes_expansion_from_compositor() {
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

    // Controls daemon receives expand request.
    let _ = recv_byo_raw(&mut controls_rx);

    // Controls daemon responds with expansion.
    send_byo(
        &mut router,
        pid(3),
        ".expand 0 { +view save-root class=btn { +text save-label content=Save } }",
    )
    .await;

    // Consume compositor's rewritten output.
    let _ = recv_byo_raw(&mut compositor_rx);

    // Controls daemon disconnects.
    router
        .handle(RouterMsg::Disconnected { process: pid(3) })
        .await;

    // Compositor should receive destroy for the daemon's expansion objects.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();

    let destroy_kinds: Vec<(&str, &str)> = destroy_cmds
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Destroy { kind, id } => Some((&**kind, &**id)),
            _ => None,
        })
        .collect();

    assert!(
        destroy_kinds
            .iter()
            .any(|(k, id)| *k == "view" && *id == "controls:save-root"),
        "expected -view controls:save-root, got: {destroy_s}"
    );

    assert_no_message(&mut compositor_rx);

    // Verify source object survives: a new daemon claiming button should
    // receive a re-expand for the app's existing button.
    let (controls2, mut controls2_rx) = mock_process(4, "controls2");
    router.add_process(controls2);
    send_byo(&mut router, pid(4), "?claim 0 button").await;

    let replay_msg = recv_byo_raw(&mut controls2_rx);
    assert!(
        replay_msg.contains("?expand") && replay_msg.contains("app:save"),
        "new daemon should receive re-expand for app:save, got: {replay_msg}"
    );
}

/// Test 42: Disconnect with multiple objects — all are cleaned up.
///
/// App has multiple root objects. All should get destroy notifications.
#[tokio::test]
async fn disconnect_multiple_objects() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // App creates multiple views.
    send_byo(
        &mut router,
        pid(2),
        "+view sidebar class=w-64 +view content class=flex-1 +view footer class=h-12",
    )
    .await;
    let _ = recv_byo_raw(&mut compositor_rx);

    // App disconnects.
    router
        .handle(RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Compositor should receive destroy for all objects.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();

    let destroy_ids: Vec<&str> = destroy_cmds
        .iter()
        .filter_map(|cmd| match cmd {
            Command::Destroy { id, .. } => Some(&**id),
            _ => None,
        })
        .collect();

    assert_eq!(
        destroy_ids.len(),
        3,
        "expected 3 destroys, got {}: {destroy_s}",
        destroy_ids.len()
    );
    assert!(destroy_ids.contains(&"app:sidebar"), "missing app:sidebar");
    assert!(destroy_ids.contains(&"app:content"), "missing app:content");
    assert!(destroy_ids.contains(&"app:footer"), "missing app:footer");
}
