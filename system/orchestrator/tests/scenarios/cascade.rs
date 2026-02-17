use byo::protocol::{Command, RequestKind};

use crate::{mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Helper: set up a standard expansion scenario.
///
/// Creates compositor (pid 1), controls (pid 2), app (pid 3).
/// Compositor observes `view,text`, controls claims `button`.
/// Returns (router, compositor_rx, controls_rx, _app_rx).
async fn setup_expansion_env() -> (
    Router,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
) {
    let mut router = Router::new();
    let (compositor, compositor_rx) = mock_process(1, "compositor");
    let (controls, controls_rx) = mock_process(2, "controls");
    let (app, app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view,text").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    (router, compositor_rx, controls_rx, app_rx)
}

/// Test 23: Destroying a claimed type cascades to expansion children.
///
/// Setup:
/// 1. Compositor observes `view,text`
/// 2. Controls claims `button`
/// 3. App sends `+button save label="Save"`, controls expands to
///    `+view save-root { +text save-label content="Save" }`
/// 4. After expansion, state tree has:
///    app:save (button, app) -> controls:save-root (view, controls)
///                                -> controls:save-label (text, controls)
///
/// Destroy:
/// 5. App sends `-button save`
/// 6. Router performs cascade destroy:
///    - Collects descendants: controls:save-root (view), controls:save-label (text)
///    - Emits `-view controls:save-root` (direct child) to compositor
///      -- compositor cascades the rest
///    - Notifies controls daemon about destroyed nodes:
///      `-view controls:save-root` and `-text controls:save-label`
///    - Destroys app:save from state tree (cascades children)
#[tokio::test]
async fn destroy_claimed_cascades() {
    let (mut router, mut compositor_rx, mut controls_rx, mut _app_rx) = setup_expansion_env().await;

    // App creates a button.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Controls receives ?expand.
    let expand_msg = recv_byo_raw(&mut controls_rx);
    let expand_cmds = byo::parser::parse(&expand_msg).unwrap();
    assert_eq!(expand_cmds.len(), 1);
    assert!(
        matches!(
            &expand_cmds[0],
            Command::Request {
                kind: RequestKind::Expand,
                seq: 0,
                ..
            }
        ),
        "expected ?expand 0, got: {expand_msg}"
    );

    // Controls responds with expansion.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root { +text save-label content=\"Save\" } }",
    )
    .await;

    // Compositor receives the expanded view tree.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "controls:save-root")),
        "expected controls:save-root in expansion output: {s}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "controls:save-label")),
        "expected controls:save-label in expansion output: {s}"
    );

    // Now destroy the button.
    send_byo(&mut router, pid(3), "-button save").await;

    // Compositor should receive destroy commands for the expansion children.
    // The router emits `-view controls:save-root` for the direct child;
    // the compositor cascades controls:save-label underneath it.
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();
    assert!(
        destroy_cmds.iter().any(|c| matches!(c, Command::Destroy { kind, id } if *kind == "view" && *id == "controls:save-root")),
        "expected -view controls:save-root for compositor, got: {destroy_s}"
    );

    // Controls daemon should be notified about its destroyed nodes.
    let notify_s = recv_byo_raw(&mut controls_rx);
    let notify_cmds = byo::parser::parse(&notify_s).unwrap();
    // Should contain destroys for controls:save-root and controls:save-label.
    assert!(
        notify_cmds
            .iter()
            .any(|c| matches!(c, Command::Destroy { id, .. } if *id == "controls:save-root")),
        "expected -view controls:save-root notification to controls, got: {notify_s}"
    );
    assert!(
        notify_cmds
            .iter()
            .any(|c| matches!(c, Command::Destroy { id, .. } if *id == "controls:save-label")),
        "expected -text controls:save-label notification to controls, got: {notify_s}"
    );
}

/// Test 24: Destroying a parent cascades through claimed children.
///
/// Setup:
/// 1. Compositor observes `view,text`
/// 2. Controls claims `button`
/// 3. App sends `+view root { +button save label="Save" }`, controls expands button
/// 4. State tree:
///    app:root (view) -> app:save (button) -> controls:save-root (view)
///                                              -> controls:save-label (text)
///
/// Destroy:
/// 5. App sends `-view root` (destroys the parent)
/// 6. Compositor gets `-view app:root` -- cascade destroys everything underneath
///    including expansion nodes
/// 7. State tree clears all 4 objects
#[tokio::test]
async fn destroy_parent_cascades_through_claimed() {
    let (mut router, mut compositor_rx, mut controls_rx, mut _app_rx) = setup_expansion_env().await;

    // App creates a view with a button child.
    send_byo(
        &mut router,
        pid(3),
        "+view root { +button save label=\"Save\" }",
    )
    .await;

    // Controls receives ?expand.
    let expand_msg = recv_byo_raw(&mut controls_rx);
    let expand_cmds = byo::parser::parse(&expand_msg).unwrap();
    assert_eq!(expand_cmds.len(), 1);
    assert!(
        matches!(
            &expand_cmds[0],
            Command::Request {
                kind: RequestKind::Expand,
                ..
            }
        ),
        "expected ?expand, got: {expand_msg}"
    );

    // Controls responds.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root { +text save-label content=\"Save\" } }",
    )
    .await;

    // Compositor receives the full rewritten tree.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "app:root")),
        "expected app:root in expansion output: {s}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "controls:save-root")),
        "expected controls:save-root in expansion output: {s}"
    );

    // Destroy the parent view (which contains the button and its expansion).
    send_byo(&mut router, pid(3), "-view root").await;

    // Compositor should receive `-view app:root`.
    // The compositor's own tree semantics cascade the rest (save-root, save-label).
    let destroy_s = recv_byo_raw(&mut compositor_rx);
    let destroy_cmds = byo::parser::parse(&destroy_s).unwrap();
    assert!(
        destroy_cmds.iter().any(
            |c| matches!(c, Command::Destroy { kind, id } if *kind == "view" && *id == "app:root")
        ),
        "expected -view app:root for compositor, got: {destroy_s}"
    );
}
