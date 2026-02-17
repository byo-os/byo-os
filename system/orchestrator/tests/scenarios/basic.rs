use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 1: Observer registration is fire-and-forget — no echo back.
///
/// Expected flow:
/// 1. Compositor sends `?observe 0 view`
/// 2. Router registers the observation — no message sent back
#[tokio::test]
async fn observer_registration() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    router.add_process(compositor);

    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Fire-and-forget: no message should be sent back to the compositor.
    // (A resync may be sent, but with empty state tree it should be empty/absent.)
    assert_no_message(&mut compositor_rx);
}

/// Test 2: A native upsert is forwarded to the observer with qualified IDs.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. App sends `+view sidebar class="w-64"`
/// 3. Router qualifies ID → `app:sidebar`, forwards to compositor
/// 4. Compositor receives `+view app:sidebar class="w-64"`
#[tokio::test]
async fn native_upsert_forwarded() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // App sends a view upsert.
    send_byo(&mut router, pid(2), "+view sidebar class=\"w-64\"").await;

    // Compositor should receive the qualified upsert.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got {}: {s}", cmds.len());
    assert!(
        matches!(&cmds[0], Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "app:sidebar"
               && props.iter().any(|p| matches!(p, byo::Prop::Value { key, value } if *key == "class" && value.as_ref() == "w-64"))
        ),
        "unexpected command: {s}"
    );
}

/// Test 3: If no observer is registered for a type, no delivery occurs.
///
/// Expected flow:
/// 1. App sends `+view sidebar` (no one is observing `view`)
/// 2. Router has no observers → nothing is delivered anywhere
#[tokio::test]
async fn no_observer_no_delivery() {
    let mut router = Router::new();
    let (app, mut app_rx) = mock_process(1, "app");
    router.add_process(app);

    send_byo(&mut router, pid(1), "+view sidebar class=\"w-64\"").await;

    // No observers — nothing should be sent to anyone (including back to app).
    assert_no_message(&mut app_rx);
}

/// Test 4: Upsert with children block forwarded correctly.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. App sends `+view root { +view child1 +view child2 }`
/// 3. Router qualifies all IDs, forwards tree structure to compositor
/// 4. Compositor receives: `+view app:root { +view app:child1 +view app:child2 }`
#[tokio::test]
async fn upsert_with_children() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(
        &mut router,
        pid(2),
        "+view root { +view child1 +view child2 }",
    )
    .await;

    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expected structure: Upsert(root), Push, Upsert(child1), Upsert(child2), Pop
    assert_eq!(
        cmds.len(),
        5,
        "expected 5 commands, got {}: {s}",
        cmds.len()
    );
    assert!(
        matches!(&cmds[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:root"),
        "expected +view app:root, got: {s}"
    );
    assert!(matches!(&cmds[1], Command::Push));
    assert!(
        matches!(&cmds[2], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child1"),
        "expected +view app:child1, got: {s}"
    );
    assert!(
        matches!(&cmds[3], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child2"),
        "expected +view app:child2, got: {s}"
    );
    assert!(matches!(&cmds[4], Command::Pop));
}

/// Test 5: Destroy command is qualified and forwarded.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. App creates `+view sidebar`, compositor receives it
/// 3. App sends `-view sidebar`
/// 4. Compositor receives `-view app:sidebar`
#[tokio::test]
async fn destroy_forwarded() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Create then destroy.
    send_byo(&mut router, pid(2), "+view sidebar").await;
    let _ = recv_byo_raw(&mut compositor_rx); // consume the upsert

    send_byo(&mut router, pid(2), "-view sidebar").await;
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(&cmds[0], Command::Destroy { kind, id } if *kind == "view" && *id == "app:sidebar"),
        "expected -view app:sidebar, got: {s}"
    );
}

/// Test 6: Patch is forwarded with set + remove props.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. App creates `+view sidebar class="w-64"`
/// 3. App patches `@view sidebar hidden ~class`
/// 4. Compositor receives `@view app:sidebar hidden ~class`
#[tokio::test]
async fn patch_forwarded() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;

    send_byo(&mut router, pid(2), "+view sidebar class=\"w-64\"").await;
    let _ = recv_byo_raw(&mut compositor_rx); // consume upsert

    send_byo(&mut router, pid(2), "@view sidebar hidden ~class").await;
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(&cmds[0], Command::Patch { kind, id, props }
            if *kind == "view" && *id == "app:sidebar"
               && props.iter().any(|p| matches!(p, byo::Prop::Boolean { key } if *key == "hidden"))
               && props.iter().any(|p| matches!(p, byo::Prop::Remove { key } if *key == "class"))
        ),
        "unexpected patch command: {s}"
    );
}

/// Test 7: Multiple observers of the same type both receive the message.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. A11y service also observes `view`
/// 3. App sends `+view sidebar`
/// 4. Both compositor and a11y receive `+view app:sidebar`
#[tokio::test]
async fn multiple_observers_same_type() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (a11y, mut a11y_rx) = mock_process(2, "a11y");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(a11y);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "?observe 0 view").await;

    send_byo(&mut router, pid(3), "+view sidebar class=\"w-64\"").await;

    // Both should receive the message.
    let s1 = recv_byo_raw(&mut compositor_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert_eq!(cmds1.len(), 1);
    assert!(
        matches!(&cmds1[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:sidebar"),
        "compositor got wrong message: {s1}"
    );

    let s2 = recv_byo_raw(&mut a11y_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert_eq!(cmds2.len(), 1);
    assert!(
        matches!(&cmds2[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:sidebar"),
        "a11y got wrong message: {s2}"
    );
}
