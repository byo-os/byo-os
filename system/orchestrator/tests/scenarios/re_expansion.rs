use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Helper: set up a router with compositor observing view,text, controls claiming button,
/// and an app. Performs the initial expansion cycle for `+button save label="Save"`.
///
/// Returns (router, compositor_rx, controls_rx, _app_rx) with the initial expansion
/// fully complete and all messages consumed.
async fn setup_initial_expansion() -> (
    Router,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
    tokio::sync::mpsc::Receiver<byo_orchestrator::process::WriteMsg>,
) {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    // Compositor observes view and text.
    send_byo(&mut router, pid(1), "?observe 0 view,text").await;

    // Controls claims button.
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends a button.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Controls receives ?expand 0.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // Controls responds with initial expansion.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root class=\"btn\" { +text save-label content=\"Save\" } }",
    )
    .await;

    // Compositor receives the initial expansion result.
    let _initial = recv_byo_raw(&mut compositor_rx);

    (router, compositor_rx, controls_rx, _app_rx)
}

/// Test 20: Patch on a claimed type triggers re-expansion with reconciliation.
///
/// Expected message flow:
/// Setup: compositor observes view,text; controls claims button; app sends
/// +button save label="Save"; controls expands to view + text.
///
/// Re-expansion:
/// 1. App patches `@button save label="New"`
/// 2. Router merges patch into state: reduced state is now
///    kind=button, label="New" (label replaced).
/// 3. Router sends `?expand` to controls with the full reduced state.
/// 4. Controls responds with new expansion where only content changed:
///    `.expand SEQ { +view save-root class="btn" { +text save-label content="New" } }`
/// 5. Router reconciles old vs new expansion:
///    - controls:save-root: class="btn" unchanged -> no delta
///    - controls:save-label: content changed "Save" -> "New" -> emit patch
/// 6. Compositor receives minimal delta: `@text controls:save-label content="New"`
#[tokio::test]
async fn patch_triggers_re_expansion() {
    let (mut router, mut compositor_rx, mut controls_rx, mut _app_rx) =
        setup_initial_expansion().await;

    // Step 1: App patches the button label.
    send_byo(&mut router, pid(3), "@button save label=\"New\"").await;

    // Step 3: Controls should receive a new ?expand with reduced state.
    // The re-expand format uses the reduced_command (not parseable by the
    // standard parser due to qualified ID in props position), so we check raw.
    let expand_msg = recv_byo_raw(&mut controls_rx);
    assert!(
        expand_msg.contains("?expand"),
        "expected ?expand in re-expansion, got: {expand_msg}"
    );
    // Sequence number should be 1 (0 was used for initial expansion).
    assert!(
        expand_msg.contains("?expand 1"),
        "expected ?expand 1, got: {expand_msg}"
    );
    // Should contain the merged label (New, not Save).
    assert!(
        expand_msg.contains("label=New") || expand_msg.contains("label=\"New\""),
        "expected label=New in reduced state, got: {expand_msg}"
    );

    // Step 4: Controls responds with new expansion (only text content changed).
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view save-root class=\"btn\" { +text save-label content=\"New\" } }",
    )
    .await;

    // Step 5-6: Compositor should receive minimal reconciliation delta.
    // The only change was content "Save" -> "New" on controls:save-label.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a patch on the text node with new content.
    let patch = cmds.iter().find(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:save-label")
    });
    assert!(
        patch.is_some(),
        "expected @text controls:save-label in delta, got: {s}"
    );

    if let Some(Command::Patch { props, .. }) = patch {
        let has_content = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "content" && value.as_ref() == "New")
        });
        assert!(has_content, "expected content=New in patch props, got: {s}");
    }

    // The view node (save-root) should NOT be patched since its props didn't change.
    let view_patch = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "view" && *id == "controls:save-root")
    });
    assert!(
        !view_patch,
        "save-root should not be patched (unchanged), got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 21: Re-expansion that adds a new node produces a create command.
///
/// Expected message flow:
/// Setup: same initial expansion as test 20 (view save-root + text save-label).
///
/// Re-expansion:
/// 1. App patches `@button save label="New"`
/// 2. Controls responds with expansion that includes a NEW node (icon):
///    `+view save-root class="btn" { +text save-label content="New" +view icon }`
/// 3. Reconciliation detects:
///    - controls:save-root unchanged
///    - controls:save-label content changed
///    - controls:icon is NEW (not in old expansion)
/// 4. Compositor receives: patch on save-label + create for icon
#[tokio::test]
async fn re_expansion_adds_node() {
    let (mut router, mut compositor_rx, mut controls_rx, mut _app_rx) =
        setup_initial_expansion().await;

    // Patch triggers re-expansion.
    send_byo(&mut router, pid(3), "@button save label=\"New\"").await;

    // Consume the ?expand message.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // Controls responds with expansion that adds a new `+view icon` node.
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view save-root class=\"btn\" { +text save-label content=\"New\" +view icon } }",
    )
    .await;

    // Compositor receives reconciliation delta.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a CREATE for the new icon node.
    let icon_create = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:icon")
    });
    assert!(
        icon_create,
        "expected +view controls:icon (new node) in delta, got: {s}"
    );

    // Should contain a PATCH for the text node with changed content.
    let text_patch = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:save-label")
    });
    assert!(
        text_patch,
        "expected @text controls:save-label in delta, got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 22: Re-expansion that removes a node produces a destroy command.
///
/// Expected message flow:
/// Setup: compositor observes view,text; controls claims button; app sends
/// +button save label="Save"; controls expands to view + text + icon.
/// (Different from tests 20/21: initial expansion includes an icon node.)
///
/// Re-expansion:
/// 1. App patches `@button save label="New"`
/// 2. Controls responds with expansion WITHOUT the icon node:
///    `+view save-root class="btn" { +text save-label content="New" }`
/// 3. Reconciliation detects controls:icon was removed.
/// 4. Compositor receives: patch on save-label + destroy for icon
#[tokio::test]
async fn re_expansion_removes_node() {
    // Custom setup: initial expansion includes an icon node.
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view,text").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends button.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Controls receives ?expand 0.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // Controls responds with expansion that includes an icon node.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root class=\"btn\" { +text save-label content=\"Save\" +view icon class=\"icon\" } }",
    )
    .await;

    // Compositor receives initial expansion.
    let initial = recv_byo_raw(&mut compositor_rx);
    let initial_cmds = byo::parser::parse(&initial).unwrap();
    // Verify icon is present in initial output.
    assert!(
        initial_cmds.iter().any(|c| {
            matches!(c, Command::Upsert { kind, id, .. }
                if *kind == "view" && *id == "controls:icon")
        }),
        "initial expansion should include controls:icon, got: {initial}"
    );

    // Now trigger re-expansion: patch button.
    send_byo(&mut router, pid(3), "@button save label=\"New\"").await;

    // Controls receives ?expand 1.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // Controls responds with expansion WITHOUT the icon node.
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view save-root class=\"btn\" { +text save-label content=\"New\" } }",
    )
    .await;

    // Compositor receives reconciliation delta.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a DESTROY for the removed icon node.
    let icon_destroy = cmds.iter().any(|c| {
        matches!(c, Command::Destroy { kind, id }
            if *kind == "view" && *id == "controls:icon")
    });
    assert!(
        icon_destroy,
        "expected -view controls:icon (removed node) in delta, got: {s}"
    );

    // Should contain a PATCH for the text node with changed content.
    let text_patch = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:save-label")
    });
    assert!(
        text_patch,
        "expected @text controls:save-label in delta, got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}
