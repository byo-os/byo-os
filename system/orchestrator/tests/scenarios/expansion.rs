use byo::protocol::{Command, Prop, RequestKind};

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo, try_recv_byo};
use byo_orchestrator::router::Router;

/// Test 8: Claim registration is fire-and-forget.
///
/// Expected flow:
/// 1. Controls daemon sends `?claim 0 button`
/// 2. Router registers the claim internally
/// 3. No message is sent back to controls (fire-and-forget)
/// 4. With an empty state tree, no replay occurs either
#[tokio::test]
async fn claim_registration() {
    let mut router = Router::new();
    let (controls, mut controls_rx) = mock_process(1, "controls");
    router.add_process(controls);

    send_byo(&mut router, pid(1), "?claim 0 button").await;

    // Fire-and-forget: no echo, no replay (state tree is empty).
    assert_no_message(&mut controls_rx);
}

/// Test 9: Basic expansion cycle.
///
/// Expected flow:
/// 1. Compositor observes `view,text`
/// 2. Controls daemon claims `button`
/// 3. App sends `+button save label="Save"`
/// 4. Router sees button is claimed by controls, sends
///    `?expand 0 app:save kind=button label="Save"` to controls
/// 5. Controls responds with
///    `.expand 0 { +view save-root class="btn" { +text save-label content="Save" } }`
/// 6. Router qualifies expansion IDs under "controls", rewrites batch
/// 7. Compositor receives:
///    `+view controls:save-root class="btn" { +text controls:save-label content="Save" }`
#[tokio::test]
async fn basic_expansion() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    // Step 1: Compositor observes view and text.
    send_byo(&mut router, pid(1), "?observe 0 view,text").await;

    // Step 2: Controls daemon claims button.
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // Step 3: App sends a button upsert.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Step 4: Controls daemon should receive a ?expand request.
    // Check what messages are available on each channel.
    let compositor_msg = try_recv_byo(&mut compositor_rx);
    assert!(
        compositor_msg.is_none(),
        "compositor should have no message yet, got: {:?}",
        compositor_msg
    );

    let expand_msg_opt = try_recv_byo(&mut controls_rx);
    assert!(
        expand_msg_opt.is_some(),
        "controls should have received ?expand, but channel is empty"
    );
    let expand_msg = expand_msg_opt.unwrap();
    // The ?expand message uses a direct format from the router:
    // `\n?expand SEQ QID kind=TYPE props...`
    // This raw format might not parse cleanly with the BYO parser, so
    // verify content via string assertions.
    assert!(
        expand_msg.contains("?expand"),
        "expected ?expand in message, got: {expand_msg}"
    );
    assert!(
        expand_msg.contains("app:save"),
        "expected qualified ID app:save, got: {expand_msg}"
    );
    assert!(
        expand_msg.contains("kind=button"),
        "expected kind=button, got: {expand_msg}"
    );
    assert!(
        expand_msg.contains("label=Save") || expand_msg.contains("label=\"Save\""),
        "expected label=Save, got: {expand_msg}"
    );

    // Step 5: Controls responds with expansion.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root class=\"btn\" { +text save-label content=\"Save\" } }",
    )
    .await;

    // Step 6-7: Compositor should now receive the rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expect: +view controls:save-root class="btn" { +text controls:save-label content="Save" }
    // That is: Upsert, Push, Upsert, Pop
    assert_eq!(
        cmds.len(),
        4,
        "expected 4 commands, got {}: {s}",
        cmds.len()
    );
    assert!(
        matches!(
            &cmds[0],
            Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "controls:save-root"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "class" && value.as_ref() == "btn"))
        ),
        "expected +view controls:save-root, got: {s}"
    );
    assert!(matches!(&cmds[1], Command::Push), "expected Push, got: {s}");
    assert!(
        matches!(
            &cmds[2],
            Command::Upsert { kind, id, props }
            if *kind == "text" && *id == "controls:save-label"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "content" && value.as_ref() == "Save"))
        ),
        "expected +text controls:save-label, got: {s}"
    );
    assert!(matches!(&cmds[3], Command::Pop), "expected Pop, got: {s}");

    // No further messages.
    assert_no_message(&mut compositor_rx);
}

/// Test 10: Expansion with native siblings preserved in order.
///
/// Expected flow:
/// 1. Compositor observes `view,text`
/// 2. Controls claims `button`
/// 3. App sends `+view root { +view child1 +button save label="Save" +view child3 }`
/// 4. Router expands button, sends `?expand` to controls
/// 5. Controls responds `.expand 0 { +view save-root class="btn" }`
/// 6. Compositor gets:
///    `+view app:root { +view app:child1 +view controls:save-root class="btn" +view app:child3 }`
///    (button replaced in-place, native siblings preserved)
#[tokio::test]
async fn expansion_with_native_siblings() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view,text").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends a view with native children and one button.
    send_byo(
        &mut router,
        pid(3),
        "+view root { +view child1 +button save label=\"Save\" +view child3 }",
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
        ".expand 0 { +view save-root class=\"btn\" }",
    )
    .await;

    // Compositor receives the rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expected structure:
    // +view app:root { +view app:child1 +view controls:save-root class="btn" +view app:child3 }
    // = Upsert(root) Push Upsert(child1) Upsert(save-root) Upsert(child3) Pop
    assert_eq!(
        cmds.len(),
        6,
        "expected 6 commands, got {}: {s}",
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
        matches!(
            &cmds[3],
            Command::Upsert { kind, id, props }
            if *kind == "view" && *id == "controls:save-root"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "class" && value.as_ref() == "btn"))
        ),
        "expected +view controls:save-root, got: {s}"
    );
    assert!(
        matches!(&cmds[4], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:child3"),
        "expected +view app:child3, got: {s}"
    );
    assert!(matches!(&cmds[5], Command::Pop));

    // No +button should appear anywhere.
    assert!(
        !s.contains("+button"),
        "button should not appear in output: {s}"
    );
}

/// Test 11: Expansion blocks subsequent output from the same app.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. Controls claims `button`
/// 3. App sends `+button save label="Save"` (triggers expansion, blocks queue)
/// 4. App sends `+view sidebar` (should be queued, not delivered yet)
/// 5. Controls responds `.expand 0 { +view save-root }`
/// 6. THEN compositor gets both: first the expansion result, then `+view app:sidebar`
#[tokio::test]
async fn expansion_blocks_output() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends a button (triggers expansion, blocks output queue).
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Consume the ?expand from controls.
    let _expand_msg = recv_byo_raw(&mut controls_rx);

    // App sends another message while blocked.
    send_byo(&mut router, pid(3), "+view sidebar").await;

    // Compositor should have received nothing yet.
    assert_no_message(&mut compositor_rx);

    // Controls responds with expansion.
    send_byo(&mut router, pid(2), ".expand 0 { +view save-root }").await;

    // Now compositor should receive the expansion result first.
    let s1 = recv_byo_raw(&mut compositor_rx);
    let cmds1 = byo::parser::parse(&s1).unwrap();
    assert!(
        cmds1.iter().any(|c| matches!(c, Command::Upsert { kind, id, .. } if *kind == "view" && *id == "controls:save-root")),
        "expected expansion result with controls:save-root, got: {s1}"
    );

    // Then the queued sidebar message.
    let s2 = recv_byo_raw(&mut compositor_rx);
    let cmds2 = byo::parser::parse(&s2).unwrap();
    assert_eq!(cmds2.len(), 1, "expected 1 command for sidebar, got: {s2}");
    assert!(
        matches!(&cmds2[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "app:sidebar"),
        "expected +view app:sidebar, got: {s2}"
    );
}

/// Test 12: Multiple expansions in one batch.
///
/// Expected flow:
/// 1. Compositor observes `view`
/// 2. Controls claims `button`
/// 3. App sends `+button save label="Save" +button cancel label="Cancel"`
///    (two expansions in one batch)
/// 4. Router sends `?expand 0 ...` and `?expand 1 ...` to controls
/// 5. Controls responds to both
/// 6. Only after BOTH are complete does compositor get the rewritten batch
#[tokio::test]
async fn multiple_expansions_one_batch() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends two buttons in one batch.
    send_byo(
        &mut router,
        pid(3),
        "+button save label=\"Save\" +button cancel label=\"Cancel\"",
    )
    .await;

    // Controls should receive two ?expand requests.
    let expand1 = recv_byo_raw(&mut controls_rx);
    let expand2 = recv_byo_raw(&mut controls_rx);

    let cmds1 = byo::parser::parse(&expand1).unwrap();
    let cmds2 = byo::parser::parse(&expand2).unwrap();
    assert_eq!(cmds1.len(), 1);
    assert_eq!(cmds2.len(), 1);
    assert!(
        matches!(
            &cmds1[0],
            Command::Request {
                kind: RequestKind::Expand,
                seq: 0,
                ..
            }
        ),
        "expected ?expand 0, got: {expand1}"
    );
    assert!(
        matches!(
            &cmds2[0],
            Command::Request {
                kind: RequestKind::Expand,
                seq: 1,
                ..
            }
        ),
        "expected ?expand 1, got: {expand2}"
    );

    // Respond to first only.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root class=\"btn-save\" }",
    )
    .await;

    // Compositor should NOT have received anything yet (second expansion still pending).
    assert_no_message(&mut compositor_rx);

    // Respond to second.
    send_byo(
        &mut router,
        pid(2),
        ".expand 1 { +view cancel-root class=\"btn-cancel\" }",
    )
    .await;

    // Now compositor should receive the full rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Both buttons should be replaced by their expansions.
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "controls:save-root")),
        "expected controls:save-root in output: {s}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "controls:cancel-root")),
        "expected controls:cancel-root in output: {s}"
    );
    assert!(
        !s.contains("+button"),
        "button should not appear in output: {s}"
    );
}

/// Test 13: Anonymous objects bypass expansion entirely.
///
/// Expected flow:
/// 1. Compositor observes `view,button`
/// 2. Controls claims `button`
/// 3. App sends `+button _ label="Save"` (anonymous, _ ID)
/// 4. Anonymous objects bypass expansion entirely
/// 5. Compositor gets `+button _ label="Save"` directly (ID stays `_`)
///
/// Note: `_` doesn't get qualified or expanded.
#[tokio::test]
async fn anonymous_not_expanded() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    // Compositor observes both view and button so it receives the anonymous button directly.
    send_byo(&mut router, pid(1), "?observe 0 view,button").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;

    // App sends anonymous button.
    send_byo(&mut router, pid(3), "+button _ label=\"Save\"").await;

    // Controls should NOT receive any ?expand request.
    assert_no_message(&mut controls_rx);

    // Compositor should receive the button directly with _ ID.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(
            &cmds[0],
            Command::Upsert { kind, id, props }
            if *kind == "button" && *id == "_"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "label" && value.as_ref() == "Save"))
        ),
        "expected +button _ label=Save, got: {s}"
    );
}

/// Test 14: Unclaim stops future expansion.
///
/// Expected flow:
/// 1. Compositor observes `view,button`
/// 2. Controls claims `button`
/// 3. Controls unclaims `button` via `?unclaim 0 button`
/// 4. App sends `+button save label="Save"`
/// 5. Since button is no longer claimed, it's forwarded as native to compositor
/// 6. Compositor gets `+button app:save label="Save"`
#[tokio::test]
async fn unclaim_stops_expansion() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(app);

    send_byo(&mut router, pid(1), "?observe 0 view,button").await;
    send_byo(&mut router, pid(2), "?claim 0 button").await;
    send_byo(&mut router, pid(2), "?unclaim 0 button").await;

    // App sends a button. Since unclaimed, no expansion should happen.
    send_byo(&mut router, pid(3), "+button save label=\"Save\"").await;

    // Controls should NOT receive any ?expand.
    assert_no_message(&mut controls_rx);

    // Compositor should receive the button as a native type with qualified ID.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1, "expected 1 command, got: {s}");
    assert!(
        matches!(
            &cmds[0],
            Command::Upsert { kind, id, props }
            if *kind == "button" && *id == "app:save"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "label" && value.as_ref() == "Save"))
        ),
        "expected +button app:save label=Save, got: {s}"
    );
}
