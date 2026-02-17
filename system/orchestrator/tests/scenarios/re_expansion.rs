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
    // The re-expand format uses the reduced command with `+` stripped:
    //   `\n?expand SEQ type qid props...`
    // The label was merged from the patch (New replaces Save).
    let expand_msg = recv_byo_raw(&mut controls_rx);
    assert_eq!(expand_msg, "\n?expand 1 button app:save label=New");

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

    // Should contain a patch on the text object with new content.
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

    // The view object (save-root) should NOT be patched since its props didn't change.
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

/// Test 21: Re-expansion that adds a new object produces a create command.
///
/// Expected message flow:
/// Setup: same initial expansion as test 20 (view save-root + text save-label).
///
/// Re-expansion:
/// 1. App patches `@button save label="New"`
/// 2. Controls responds with expansion that includes a NEW object (icon):
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

    // Controls responds with expansion that adds a new `+view icon` object.
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view save-root class=\"btn\" { +text save-label content=\"New\" +view icon } }",
    )
    .await;

    // Compositor receives reconciliation delta.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a CREATE for the new icon object.
    let icon_create = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:icon")
    });
    assert!(
        icon_create,
        "expected +view controls:icon (new object) in delta, got: {s}"
    );

    // Should contain a PATCH for the text object with changed content.
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

/// Test 22: Re-expansion that removes an object produces a destroy command.
///
/// Expected message flow:
/// Setup: compositor observes view,text; controls claims button; app sends
/// +button save label="Save"; controls expands to view + text + icon.
/// (Different from tests 20/21: initial expansion includes an icon object.)
///
/// Re-expansion:
/// 1. App patches `@button save label="New"`
/// 2. Controls responds with expansion WITHOUT the icon object:
///    `+view save-root class="btn" { +text save-label content="New" }`
/// 3. Reconciliation detects controls:icon was removed.
/// 4. Compositor receives: patch on save-label + destroy for icon
#[tokio::test]
async fn re_expansion_removes_node() {
    // Custom setup: initial expansion includes an icon object.
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

    // Controls responds with expansion that includes an icon object.
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

    // Controls responds with expansion WITHOUT the icon object.
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view save-root class=\"btn\" { +text save-label content=\"New\" } }",
    )
    .await;

    // Compositor receives reconciliation delta.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a DESTROY for the removed icon object.
    let icon_destroy = cmds.iter().any(|c| {
        matches!(c, Command::Destroy { kind, id }
            if *kind == "view" && *id == "controls:icon")
    });
    assert!(
        icon_destroy,
        "expected -view controls:icon (removed object) in delta, got: {s}"
    );

    // Should contain a PATCH for the text object with changed content.
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

/// Helper: set up a router with compositor observing view,text, controls claiming
/// button, and an app. Creates `+button settings label="Settings"` fully expanded,
/// then returns everything ready for a mixed-expansion batch test.
///
/// Returns (router, compositor_rx, controls_rx, _app_rx) with the initial expansion
/// for `settings` fully complete and all messages consumed.
async fn setup_two_buttons() -> (
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

    // App creates settings button (initial expansion).
    send_byo(&mut router, pid(3), "+button settings label=\"Settings\"").await;

    // Controls receives ?expand 0.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // Controls responds with initial expansion.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view settings-root class=\"btn\" { +text settings-label content=\"Settings\" } }",
    )
    .await;

    // Compositor receives the initial expansion result.
    let _initial = recv_byo_raw(&mut compositor_rx);

    (router, compositor_rx, controls_rx, _app_rx)
}

/// Test 23b: Mixed batch with initial upsert + re-expansion patch, controls
/// responds to the initial expansion first, then the re-expansion.
///
/// This tests the per-expansion `is_re_expand` flag: the initial expansion
/// (for `extra`) must NOT be reconciled, while the re-expansion (for
/// `settings`) MUST be reconciled — regardless of completion order.
#[tokio::test]
async fn mixed_upsert_and_patch_completes_in_order() {
    let (mut router, mut compositor_rx, mut controls_rx, _app_rx) = setup_two_buttons().await;

    // App sends a single batch: create new button + patch existing one.
    send_byo(
        &mut router,
        pid(3),
        "+button extra label=\"Extra\" @button settings label=\"New\"",
    )
    .await;

    // Controls receives two ?expand requests.
    let expand1 = recv_byo_raw(&mut controls_rx);
    let expand2 = recv_byo_raw(&mut controls_rx);

    // One should be for extra (initial), one for settings (re-expand with reduced state).
    assert!(
        expand1.contains("app:extra") || expand2.contains("app:extra"),
        "expected ?expand for app:extra, got: {expand1} / {expand2}"
    );
    assert!(
        expand1.contains("app:settings") || expand2.contains("app:settings"),
        "expected ?expand for app:settings, got: {expand1} / {expand2}"
    );

    // Find the seq numbers.
    let (extra_seq, settings_seq) = if expand1.contains("app:extra") {
        let extra_seq = expand1.split_whitespace().nth(1).unwrap();
        let settings_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    } else {
        let settings_seq = expand1.split_whitespace().nth(1).unwrap();
        let extra_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    };

    // Controls responds to EXTRA first (initial expansion).
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {extra_seq} {{ +view extra-root class=\"btn\" {{ +text extra-label content=\"Extra\" }} }}"
        ),
    )
    .await;

    // Batch is not ready yet — settings still pending.
    assert_no_message(&mut compositor_rx);

    // Controls responds to SETTINGS (re-expansion).
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {settings_seq} {{ +view settings-root class=\"btn\" {{ +text settings-label content=\"New\" }} }}"
        ),
    )
    .await;

    // Compositor receives the combined output.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Extra should be created (initial expansion → upsert).
    let extra_created = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:extra-root")
    });
    assert!(
        extra_created,
        "expected +view controls:extra-root (initial expansion), got: {s}"
    );

    // Settings should be reconciled (re-expansion → patch delta only).
    // The content changed from "Settings" to "New" on the text object.
    let settings_patched = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:settings-label")
    });
    assert!(
        settings_patched,
        "expected @text controls:settings-label (reconciliation delta), got: {s}"
    );

    // Settings root should NOT be a full upsert (it should be a patch or absent).
    let settings_upserted = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:settings-root")
    });
    assert!(
        !settings_upserted,
        "settings-root should NOT be a full upsert in reconciliation, got: {s}"
    );

    assert_no_message(&mut compositor_rx);
}

/// Test 23c: Same as mixed_upsert_and_patch_completes_in_order but controls
/// responds to the re-expansion (settings) first, then the initial (extra).
///
/// This verifies that completion order doesn't change semantics.
#[tokio::test]
async fn mixed_upsert_and_patch_completes_out_of_order() {
    let (mut router, mut compositor_rx, mut controls_rx, _app_rx) = setup_two_buttons().await;

    // App sends a single batch: create new button + patch existing one.
    send_byo(
        &mut router,
        pid(3),
        "+button extra label=\"Extra\" @button settings label=\"New\"",
    )
    .await;

    // Controls receives two ?expand requests.
    let expand1 = recv_byo_raw(&mut controls_rx);
    let expand2 = recv_byo_raw(&mut controls_rx);

    let (extra_seq, settings_seq) = if expand1.contains("app:extra") {
        let extra_seq = expand1.split_whitespace().nth(1).unwrap();
        let settings_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    } else {
        let settings_seq = expand1.split_whitespace().nth(1).unwrap();
        let extra_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    };

    // Controls responds to SETTINGS first (re-expansion — out of order).
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {settings_seq} {{ +view settings-root class=\"btn\" {{ +text settings-label content=\"New\" }} }}"
        ),
    )
    .await;

    // Batch is not ready yet — extra still pending.
    assert_no_message(&mut compositor_rx);

    // Controls responds to EXTRA (initial expansion).
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {extra_seq} {{ +view extra-root class=\"btn\" {{ +text extra-label content=\"Extra\" }} }}"
        ),
    )
    .await;

    // Compositor receives the combined output.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Extra should be created (initial expansion → upsert).
    let extra_created = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:extra-root")
    });
    assert!(
        extra_created,
        "expected +view controls:extra-root (initial expansion), got: {s}"
    );

    // Settings should be reconciled (re-expansion → patch delta only).
    let settings_patched = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:settings-label")
    });
    assert!(
        settings_patched,
        "expected @text controls:settings-label (reconciliation delta), got: {s}"
    );

    // Settings root should NOT be a full upsert.
    let settings_upserted = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:settings-root")
    });
    assert!(
        !settings_upserted,
        "settings-root should NOT be a full upsert in reconciliation, got: {s}"
    );

    assert_no_message(&mut compositor_rx);
}

/// Test 23d: Mixed batch with patch first, then upsert — verifying command
/// order in the batch doesn't affect per-expansion flag semantics.
#[tokio::test]
async fn mixed_patch_and_upsert() {
    let (mut router, mut compositor_rx, mut controls_rx, _app_rx) = setup_two_buttons().await;

    // App sends a single batch: patch existing, then create new (reversed order).
    send_byo(
        &mut router,
        pid(3),
        "@button settings label=\"New\" +button extra label=\"Extra\"",
    )
    .await;

    // Controls receives two ?expand requests.
    let expand1 = recv_byo_raw(&mut controls_rx);
    let expand2 = recv_byo_raw(&mut controls_rx);

    let (extra_seq, settings_seq) = if expand1.contains("app:extra") {
        let extra_seq = expand1.split_whitespace().nth(1).unwrap();
        let settings_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    } else {
        let settings_seq = expand1.split_whitespace().nth(1).unwrap();
        let extra_seq = expand2.split_whitespace().nth(1).unwrap();
        (extra_seq.to_string(), settings_seq.to_string())
    };

    // Controls responds to both.
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {extra_seq} {{ +view extra-root class=\"btn\" {{ +text extra-label content=\"Extra\" }} }}"
        ),
    )
    .await;

    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {settings_seq} {{ +view settings-root class=\"btn\" {{ +text settings-label content=\"New\" }} }}"
        ),
    )
    .await;

    // Compositor receives the combined output.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Extra should be created (initial expansion).
    let extra_created = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:extra-root")
    });
    assert!(
        extra_created,
        "expected +view controls:extra-root (initial expansion), got: {s}"
    );

    // Settings should be reconciled (re-expansion).
    let settings_patched = cmds.iter().any(|c| {
        matches!(c, Command::Patch { kind, id, .. }
            if *kind == "text" && *id == "controls:settings-label")
    });
    assert!(
        settings_patched,
        "expected @text controls:settings-label (reconciliation delta), got: {s}"
    );

    // Settings root should NOT be a full upsert.
    let settings_upserted = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:settings-root")
    });
    assert!(
        !settings_upserted,
        "settings-root should NOT be a full upsert in reconciliation, got: {s}"
    );

    assert_no_message(&mut compositor_rx);
}

/// Test 23e: Destroy then create of the same claimed-type ID in one batch.
///
/// The new `+button save` should be treated as an initial expansion (NOT
/// a re-expansion), since the old one was destroyed in the same batch.
#[tokio::test]
async fn destroy_then_create_not_re_expansion() {
    let (mut router, mut compositor_rx, mut controls_rx, _app_rx) = setup_initial_expansion().await;

    // App destroys the old button and creates a new one with the same ID.
    send_byo(
        &mut router,
        pid(3),
        "-button save +button save label=\"New Save\"",
    )
    .await;

    // Controls receives a ?expand for the new save (initial, not re-expand).
    let expand_msg = recv_byo_raw(&mut controls_rx);
    assert!(
        expand_msg.contains("app:save"),
        "expected ?expand for app:save, got: {expand_msg}"
    );

    // Extract the seq number.
    let seq: &str = expand_msg.split_whitespace().nth(1).unwrap();

    // Controls responds with expansion.
    send_byo(
        &mut router,
        pid(2),
        &format!(
            ".expand {seq} {{ +view save-root class=\"new-btn\" {{ +text save-label content=\"New Save\" }} }}"
        ),
    )
    .await;

    // Compositor receives output.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should have destroys for OLD expansion children (cascade from -button save).
    let has_destroy = cmds.iter().any(|c| matches!(c, Command::Destroy { .. }));
    assert!(
        has_destroy,
        "expected destroy commands for old expansion children, got: {s}"
    );

    // Should have the NEW expansion created as upserts (initial expansion).
    let new_root = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "controls:save-root")
    });
    assert!(
        new_root,
        "expected +view controls:save-root (initial expansion for new save), got: {s}"
    );

    assert_no_message(&mut compositor_rx);
}
