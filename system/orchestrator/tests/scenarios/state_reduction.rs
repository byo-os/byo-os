use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 33: Create then patch, late observer gets single reduced upsert.
///
/// Expected message flow:
/// 1. App sends `+view sidebar class="w-64" order=0` -- state tree stores it
/// 2. App patches `@view sidebar order=1` -- state merges: class="w-64", order=1
/// 3. Late compositor observes `view`
/// 4. Resync delivers a single reduced upsert:
///    `+view app:sidebar class="w-64" order=1`
///    The patch merged into the original upsert, producing one idempotent command.
#[tokio::test]
async fn create_patch_replay() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (compositor, mut compositor_rx) = mock_process(2, "compositor");
    router.add_process(app);
    router.add_process(compositor);

    // Step 1: App creates a view.
    send_byo(&mut router, pid(1), "+view sidebar class=\"w-64\" order=0").await;

    // Step 2: App patches the view (change order).
    send_byo(&mut router, pid(1), "@view sidebar order=1").await;

    // No observers yet -- nothing should be sent.
    assert_no_message(&mut compositor_rx);

    // Step 3: Compositor observes view.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 4: Resync should deliver a single reduced upsert.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should be exactly one upsert with the merged props.
    let upsert = cmds.iter().find(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:sidebar")
    });
    assert!(
        upsert.is_some(),
        "expected +view app:sidebar in resync, got: {s}"
    );

    if let Some(Command::Upsert { props, .. }) = upsert {
        // class should be preserved from original upsert.
        let has_class = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "w-64")
        });
        assert!(has_class, "expected class=w-64 in reduced props, got: {s}");

        // order should be the patched value (1), not the original (0).
        let has_order = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "order" && value.as_ref() == "1")
        });
        assert!(has_order, "expected order=1 in reduced props, got: {s}");

        // Verify order is NOT 0 (the original value should be overwritten).
        let has_old_order = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "order" && value.as_ref() == "0")
        });
        assert!(
            !has_old_order,
            "order should be 1 (patched), not 0 (original), got: {s}"
        );
    }

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 34: Create multiple, destroy one, late observer gets survivors.
///
/// Expected message flow:
/// 1. App sends `+view a`, `+view b`, `+view c` -- three objects in state
/// 2. App destroys `-view b` -- state removes b
/// 3. Late compositor observes `view`
/// 4. Resync delivers 2 survivors: `+view app:a` and `+view app:c`
///    Object b should not appear.
#[tokio::test]
async fn create_destroy_replay() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (compositor, mut compositor_rx) = mock_process(2, "compositor");
    router.add_process(app);
    router.add_process(compositor);

    // Step 1: App creates three views.
    send_byo(&mut router, pid(1), "+view a").await;
    send_byo(&mut router, pid(1), "+view b").await;
    send_byo(&mut router, pid(1), "+view c").await;

    // Step 2: App destroys b.
    send_byo(&mut router, pid(1), "-view b").await;

    // Step 3: Compositor observes view.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 4: Resync should deliver survivors a and c.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain a and c.
    let has_a = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:a")
    });
    let has_c = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:c")
    });
    assert!(has_a, "expected +view app:a in resync, got: {s}");
    assert!(has_c, "expected +view app:c in resync, got: {s}");

    // Should NOT contain b.
    let has_b = cmds.iter().any(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:b")
    });
    assert!(
        !has_b,
        "destroyed object app:b should not appear in resync, got: {s}"
    );

    // Count upserts -- should be exactly 2.
    let upsert_count = cmds
        .iter()
        .filter(|c| matches!(c, Command::Upsert { .. }))
        .count();
    assert_eq!(
        upsert_count, 2,
        "expected 2 upserts (a and c), got {upsert_count}: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 35: Patch removes a prop, late observer gets reduced state without it.
///
/// Expected message flow:
/// 1. App sends `+view sidebar class="w-64" tooltip="hi"` -- two props
/// 2. App patches `@view sidebar ~tooltip` -- removes tooltip
/// 3. Late compositor observes `view`
/// 4. Resync delivers: `+view app:sidebar class="w-64"` (no tooltip)
///    The ~tooltip removal was folded into the reduced state.
#[tokio::test]
async fn patch_removes_prop_replay() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (compositor, mut compositor_rx) = mock_process(2, "compositor");
    router.add_process(app);
    router.add_process(compositor);

    // Step 1: App creates a view with two props.
    send_byo(
        &mut router,
        pid(1),
        "+view sidebar class=\"w-64\" tooltip=\"hi\"",
    )
    .await;

    // Step 2: App patches to remove tooltip.
    send_byo(&mut router, pid(1), "@view sidebar ~tooltip").await;

    // Step 3: Compositor observes view.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 4: Resync should deliver view without tooltip.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    let upsert = cmds.iter().find(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:sidebar")
    });
    assert!(
        upsert.is_some(),
        "expected +view app:sidebar in resync, got: {s}"
    );

    if let Some(Command::Upsert { props, .. }) = upsert {
        // class should still be present.
        let has_class = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "w-64")
        });
        assert!(has_class, "expected class=w-64 in reduced props, got: {s}");

        // tooltip should NOT be present (it was removed by the patch).
        let has_tooltip = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, .. } | byo::Prop::Boolean { key }
                if *key == "tooltip")
        });
        assert!(
            !has_tooltip,
            "tooltip should have been removed by ~tooltip patch, got: {s}"
        );

        // There should also be no ~tooltip remove marker in the upsert
        // (reduced state is a full upsert, not a patch).
        let has_remove_tooltip = props
            .iter()
            .any(|p| matches!(p, byo::Prop::Remove { key } if *key == "tooltip"));
        assert!(
            !has_remove_tooltip,
            "reduced upsert should not contain ~tooltip remove, got: {s}"
        );
    }

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 36: Children preserved across parent patch in replay.
///
/// Expected message flow:
/// 1. App sends `+view root class="p-4" { +view child class="inner" }`
/// 2. App patches `@view root class="p-8"` (change parent prop only)
/// 3. Late compositor observes `view`
/// 4. Resync delivers the full tree with updated parent props:
///    `+view app:root class="p-8" { +view app:child class="inner" }`
///    The child remains with original props, parent has the patched class.
#[tokio::test]
async fn children_preserved_replay() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (compositor, mut compositor_rx) = mock_process(2, "compositor");
    router.add_process(app);
    router.add_process(compositor);

    // Step 1: App creates a view tree.
    send_byo(
        &mut router,
        pid(1),
        "+view root class=\"p-4\" { +view child class=\"inner\" }",
    )
    .await;

    // Step 2: App patches parent prop.
    send_byo(&mut router, pid(1), "@view root class=\"p-8\"").await;

    // Step 3: Compositor observes view.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 4: Resync should deliver the full tree.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Should contain root with updated class.
    let root = cmds.iter().find(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:root")
    });
    assert!(
        root.is_some(),
        "expected +view app:root in resync, got: {s}"
    );

    if let Some(Command::Upsert { props, .. }) = root {
        let has_new_class = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "p-8")
        });
        assert!(
            has_new_class,
            "expected class=p-8 (patched) on root, got: {s}"
        );

        // Should NOT have the old class value.
        let has_old_class = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "p-4")
        });
        assert!(
            !has_old_class,
            "root should have class=p-8 not p-4, got: {s}"
        );
    }

    // Should contain child with original props.
    let child = cmds.iter().find(|c| {
        matches!(c, Command::Upsert { kind, id, .. }
            if *kind == "view" && *id == "app:child")
    });
    assert!(
        child.is_some(),
        "expected +view app:child in resync (children preserved), got: {s}"
    );

    if let Some(Command::Upsert { props, .. }) = child {
        let has_inner = props.iter().any(|p| {
            matches!(p, byo::Prop::Value { key, value }
                if *key == "class" && value.as_ref() == "inner")
        });
        assert!(
            has_inner,
            "expected class=inner on child (original prop), got: {s}"
        );
    }

    // Should have tree structure (Push/Pop for children).
    assert!(
        cmds.iter().any(|c| matches!(c, Command::Push { .. })),
        "expected Push (children block) in resync, got: {s}"
    );

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 37: Daemon replay uses fully reduced state (merged upsert + patches).
///
/// Expected message flow:
/// 1. App sends `+button save label="Save" variant="primary"` -- no daemon yet
/// 2. App patches `@button save label="New"` -- state reduced:
///    label="New", variant="primary" (variant from upsert, label from patch)
/// 3. Late controls daemon claims `button`
/// 4. Controls gets `?expand` with the fully reduced state.
///    The expand message should contain both label="New" (patched) and
///    variant="primary" (from original upsert).
#[tokio::test]
async fn daemon_replay_reduced() {
    let mut router = Router::new();
    let (app, mut _app_rx) = mock_process(1, "app");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    router.add_process(app);
    router.add_process(controls);

    // Step 1: App creates a button with two props.
    send_byo(
        &mut router,
        pid(1),
        "+button save label=\"Save\" variant=\"primary\"",
    )
    .await;

    // Step 2: App patches the label.
    send_byo(&mut router, pid(1), "@button save label=\"New\"").await;

    // No messages to controls yet.
    assert_no_message(&mut controls_rx);

    // Step 3: Controls claims button.
    send_byo(&mut router, pid(2), "#claim button").await;

    // Step 4: Controls should receive ?expand with fully reduced state.
    //   `\n?expand SEQ qid kind=type props...`
    // Props are in IndexMap insertion order (label first, variant second),
    // with label updated to "New" by the patch.
    let s = recv_byo_raw(&mut controls_rx);
    assert_eq!(
        s,
        "\n?expand 0 app:save kind=button label=New variant=primary"
    );

    // No further messages.
    assert_no_message(&mut controls_rx);
}
