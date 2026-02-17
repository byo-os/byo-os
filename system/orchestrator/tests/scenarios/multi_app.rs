use byo::protocol::{Command, Prop};

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo};
use byo_orchestrator::router::Router;

/// Test 15: Two independent apps' output is qualified under different names.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. App1 ("notes") sends `+view sidebar class="w-64"`
/// 3. App2 ("settings") sends `+view panel class="p-4"`
/// 4. Compositor receives both:
///    - `+view notes:sidebar class="w-64"`
///    - `+view settings:panel class="p-4"`
///    (qualified under different client names)
#[tokio::test]
async fn two_apps_independent() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (notes, mut _notes_rx) = mock_process(2, "notes");
    let (settings, mut _settings_rx) = mock_process(3, "settings");
    router.add_process(compositor);
    router.add_process(notes);
    router.add_process(settings);

    // Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // App1 sends a view.
    send_byo(&mut router, pid(2), "+view sidebar class=\"w-64\"").await;
    let s1 = recv_byo_raw(&mut compositor_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert_eq!(cmds1.len(), 1, "expected 1 command from notes, got: {s1}");
    assert!(
        matches!(
            &cmds1[0],
            Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "notes:sidebar"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "class" && value.as_ref() == "w-64"))
        ),
        "expected +view notes:sidebar, got: {s1}"
    );

    // App2 sends a view.
    send_byo(&mut router, pid(3), "+view panel class=\"p-4\"").await;
    let s2 = recv_byo_raw(&mut compositor_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert_eq!(
        cmds2.len(),
        1,
        "expected 1 command from settings, got: {s2}"
    );
    assert!(
        matches!(
            &cmds2[0],
            Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "settings:panel"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "class" && value.as_ref() == "p-4"))
        ),
        "expected +view settings:panel, got: {s2}"
    );
}

/// Test 16: One app expanding does not block another app's output.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. Controls claims `button`
/// 3. App1 sends `+button save label="Save"` (triggers expansion, blocks app1's queue)
/// 4. App2 sends `+view panel` (independent, should arrive immediately)
/// 5. Compositor receives `+view app2:panel` before expansion completes
/// 6. Controls responds `.expand 0 { +view save-root }`, compositor gets it
#[tokio::test]
async fn one_expanding_does_not_block_other() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app1, mut _app1_rx) = mock_process(3, "app1");
    let (app2, mut _app2_rx) = mock_process(4, "app2");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app1);
    router.add_process(app2);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App1 sends a button (triggers expansion, blocks app1's queue).
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Consume the ?expand from controls.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // App1's expansion is pending, so compositor has nothing yet.
    assert_no_message(&mut compositor_rx);

    // App2 sends a view (independent stream, should not be blocked).
    send_byo(&mut router, pid(4), "+view panel").await;

    // Compositor should receive app2's view immediately, despite app1 being blocked.
    let s1 = recv_byo_raw(&mut compositor_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert_eq!(cmds1.len(), 1, "expected 1 command from app2, got: {s1}");
    assert!(
        matches!(&cmds1[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app2:panel"),
        "expected +view app2:panel, got: {s1}"
    );

    // Now controls responds to the expansion.
    send_byo(&mut router, pid(2), ".expand 0 { +view save-root }").await;

    // Compositor should now receive the expansion result from app1.
    let s2 = recv_byo_raw(&mut compositor_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert!(
        cmds2.iter().any(|c| matches!(c, Command::Upsert { kind, id, .. } if *kind == "view" && *id == "controls:save-root")),
        "expected expansion result with controls:save-root, got: {s2}"
    );
}
