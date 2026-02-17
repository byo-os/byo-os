use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::{Router, RouterMsg};

/// Test 38: App disconnect cleans state — new observer gets no resync for disconnected app.
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. App sends `+view sidebar class="w-64"`, compositor receives it
/// 3. App disconnects via `RouterMsg::Disconnected { process: app_pid }`
/// 4. Router clears app's state objects (app:sidebar removed from state tree)
/// 5. A new observer registers — it should get NO resync for app's objects
///    because they were cleaned up
///
/// This verifies that disconnect properly removes state, so late-joining
/// observers don't see stale objects from disconnected processes.
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

    // Step 4: State should be cleaned up. No messages to compositor from disconnect itself
    // (the router removes state but doesn't actively send destroy commands to observers
    // on disconnect in the current implementation).

    // Step 5: Register a NEW observer. If state was properly cleaned, the resync
    // should NOT include app:sidebar.
    let (a11y, mut a11y_rx) = mock_process(3, "a11y");
    router.add_process(a11y);
    send_byo(&mut router, pid(3), "?observe 0 view").await;

    // The new observer should receive NO resync (state tree for app's objects is empty).
    // If state was NOT cleaned, a11y would receive +view app:sidebar in the resync.
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
///
/// This verifies that daemon disconnect properly clears claims, so types
/// are no longer routed for expansion after the daemon disappears.
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
    // No expansion happened — it's forwarded directly.
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
