use byo::byo_assert_eq;
use byo::protocol::Command;

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 25: Disjoint observers — each observer only gets types it subscribed to.
///
/// Expected message flow:
/// 1. Compositor observes `view` only
/// 2. TextSvc observes `text` only
/// 3. App sends `+view root { +text label content="Hello" }`
/// 4. Router qualifies IDs and projects per-observer:
///    - Compositor gets `+view app:root` (text not observed, so no children block)
///    - TextSvc gets `+text app:label content="Hello"` (view not observed, text is top-level)
/// 5. Neither observer receives the other's types
#[tokio::test]
async fn disjoint_observers() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (textsvc, mut textsvc_rx) = mock_process(2, "textsvc");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(textsvc);
    router.add_process(app);

    // Step 1: Compositor observes view only.
    send_byo(&mut router, pid(1), "#observe view").await;

    // Step 2: TextSvc observes text only.
    send_byo(&mut router, pid(2), "#observe text").await;

    // Step 3: App sends a view with a text child.
    send_byo(
        &mut router,
        pid(3),
        "+view root { +text label content=\"Hello\" }",
    )
    .await;

    // Step 4a: Compositor should receive only the view, with no children block
    // since text is not in its observed set.
    let comp_msg = recv_byo_raw(&mut compositor_rx);
    byo_assert_eq!(comp_msg, +view app:root);

    // Step 4b: TextSvc should receive only the text object at top level.
    // View is not observed, so it's skipped. Text floats to top level.
    let text_msg = recv_byo_raw(&mut textsvc_rx);
    byo_assert_eq!(text_msg, +text app:label content="Hello");

    // No further messages to either observer.
    assert_no_message(&mut compositor_rx);
    assert_no_message(&mut textsvc_rx);
}

/// Test 26: Semi-disjoint observers — one sees everything, one sees a subset.
///
/// Expected message flow:
/// 1. Compositor observes `view,text` (both types)
/// 2. A11y observes `view` only
/// 3. App sends `+view root { +text label content="Hello" +view child }`
/// 4. Router qualifies IDs and projects per-observer:
///    - Compositor gets full tree: `+view app:root { +text app:label content="Hello" +view app:child }`
///    - A11y gets: `+view app:root { +view app:child }` (text stripped out, but view child remains)
#[tokio::test]
async fn semi_disjoint_observers() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (a11y, mut a11y_rx) = mock_process(2, "a11y");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(a11y);
    router.add_process(app);

    // Step 1: Compositor observes both view and text.
    send_byo(&mut router, pid(1), "#observe view,text").await;

    // Step 2: A11y observes view only.
    send_byo(&mut router, pid(2), "#observe view").await;

    // Step 3: App sends a view with text and view children.
    send_byo(
        &mut router,
        pid(3),
        "+view root { +text label content=\"Hello\" +view child }",
    )
    .await;

    // Step 4a: Compositor gets the full tree with both types.
    let comp_msg = recv_byo_raw(&mut compositor_rx);
    let comp_cmds = byo::parser::parse(&comp_msg).unwrap();
    // Expected: Upsert(root), Push, Upsert(label), Upsert(child), Pop = 5 commands
    assert_eq!(
        comp_cmds.len(),
        5,
        "compositor should get 5 commands, got {}: {comp_msg}",
        comp_cmds.len()
    );
    assert!(
        matches!(&comp_cmds[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:root"),
        "expected +view app:root, got: {comp_msg}"
    );
    assert!(matches!(&comp_cmds[1], Command::Push));
    assert!(
        matches!(
            &comp_cmds[2],
            Command::Upsert { kind, id, props }
            if *kind == "text" && *id == "app:label"
               && props.iter().any(|p| matches!(p, byo::Prop::Value { key, value } if *key == "content" && value.as_ref() == "Hello"))
        ),
        "expected +text app:label content=Hello, got: {comp_msg}"
    );
    assert!(
        matches!(&comp_cmds[3], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child"),
        "expected +view app:child, got: {comp_msg}"
    );
    assert!(matches!(&comp_cmds[4], Command::Pop));

    // Step 4b: A11y gets view root with only view child (text stripped).
    let a11y_msg = recv_byo_raw(&mut a11y_rx);
    let a11y_cmds = byo::parser::parse(&a11y_msg).unwrap();
    // Expected: Upsert(root), Push, Upsert(child), Pop = 4 commands
    assert_eq!(
        a11y_cmds.len(),
        4,
        "a11y should get 4 commands, got {}: {a11y_msg}",
        a11y_cmds.len()
    );
    assert!(
        matches!(&a11y_cmds[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:root"),
        "expected +view app:root, got: {a11y_msg}"
    );
    assert!(matches!(&a11y_cmds[1], Command::Push));
    assert!(
        matches!(&a11y_cmds[2], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child"),
        "expected +view app:child, got: {a11y_msg}"
    );
    assert!(matches!(&a11y_cmds[3], Command::Pop));

    assert_no_message(&mut compositor_rx);
    assert_no_message(&mut a11y_rx);
}

/// Test 27: Unobserved types at batch top level wrap under nearest observed ancestor.
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. App sends `+view root { +panel container { +view child class="inner" } }`
///    - First batch builds state tree: app:root (view) -> app:container (panel) -> app:child (view)
///    - Compositor gets: `+view app:root { +view app:child class="inner" }` (panel flattened,
///      child re-parented under root since panel is inside root's observed context)
/// 3. App patches: `@panel container { +view child2 }`
///    - Panel is at the top level of this batch. It's not observed.
///    - Router looks up state tree: panel's nearest observed ancestor is app:root (view).
///    - Compositor gets: `@view app:root { +view app:child2 }` (wrapped under ancestor)
#[tokio::test]
async fn unobserved_wraps_under_ancestor() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "#observe view").await;

    // Step 2: App sends a view with a non-observed panel containing a view child.
    // This establishes the state tree: root -> container -> child
    send_byo(
        &mut router,
        pid(2),
        "+view root { +panel container { +view child class=\"inner\" } }",
    )
    .await;

    // Compositor gets the first batch: panel is inside root's children (observed context),
    // so it's flattened and child is re-parented under root.
    let s1 = recv_byo_raw(&mut compositor_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    // Expected: +view app:root { +view app:child class="inner" }
    // = Upsert(root), Push, Upsert(child), Pop = 4 commands
    assert_eq!(
        cmds1.len(),
        4,
        "expected 4 commands for first batch, got {}: {s1}",
        cmds1.len()
    );
    assert!(
        matches!(&cmds1[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:root"),
        "expected +view app:root, got: {s1}"
    );
    assert!(matches!(&cmds1[1], Command::Push));
    assert!(
        matches!(
            &cmds1[2],
            Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "app:child"
               && props.iter().any(|p| matches!(p, byo::Prop::Value { key, value } if *key == "class" && value.as_ref() == "inner"))
        ),
        "expected +view app:child class=inner, got: {s1}"
    );
    assert!(matches!(&cmds1[3], Command::Pop));

    // Step 3: App patches the panel to add a new view child.
    // Panel is at top level of this batch, not observed.
    // The state tree knows panel is under root (view), so the projection
    // wraps children under @view app:root.
    send_byo(&mut router, pid(2), "@panel container { +view child2 }").await;

    let s2 = recv_byo_raw(&mut compositor_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();

    // Expected: @view app:root { +view app:child2 }
    // = Patch(root), Push, Upsert(child2), Pop = 4 commands
    assert_eq!(
        cmds2.len(),
        4,
        "expected 4 commands for second batch, got {}: {s2}",
        cmds2.len()
    );
    assert!(
        matches!(&cmds2[0], Command::Patch { kind, id, .. } if *kind == "view" && *id == "app:root"),
        "expected @view app:root, got: {s2}"
    );
    assert!(matches!(&cmds2[1], Command::Push));
    assert!(
        matches!(&cmds2[2], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child2"),
        "expected +view app:child2, got: {s2}"
    );
    assert!(matches!(&cmds2[3], Command::Pop));

    assert_no_message(&mut compositor_rx);
}
