use byo::byo_assert_eq;
use byo::protocol::{Command, Prop, RequestKind};

use crate::{assert_no_message, mock_process, pid, recv_byo_raw, send_byo, try_recv_byo};
use byo_orchestrator::router::Router;

/// Test 28: Two-daemon nested expansion (A -> B).
///
/// Expected message flow:
/// 1. Compositor observes `view,image`
/// 2. Controls claims `button`, Icons claims `icon`
/// 3. App sends `+button save label="Save"`
/// 4. Router sees button is claimed by Controls, sends:
///    `?expand 0 app:save kind=button label="Save"` to Controls
/// 5. Controls responds: `.expand 0 { +view save-root class="btn" { +icon save-icon name="check" } }`
/// 6. Router sees `+icon` in Controls' expansion body, which is claimed by Icons.
///    Router triggers nested expansion: `?expand 0 controls:save-icon kind=icon name="check"` to Icons
/// 7. Icons responds: `.expand 0 { +image check-img src="check.png" }`
/// 8. Router rewrites: all expansions spliced in, final result sent to compositor:
///    `+view controls:save-root class="btn" { +image icons:check-img src="check.png" }`
///    (No +button or +icon in output)
#[tokio::test]
async fn two_daemon_a_to_b() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (controls, mut controls_rx) = mock_process(2, "controls");
    let (icons, mut icons_rx) = mock_process(3, "icons");
    let (app, mut _app_rx) = mock_process(4, "app");
    router.add_process(compositor);
    router.add_process(controls);
    router.add_process(icons);
    router.add_process(app);

    // Step 1: Compositor observes view and image.
    send_byo(&mut router, pid(1), "?observe 0 view,image").await;

    // Step 2: Controls claims button, Icons claims icon.
    send_byo(&mut router, pid(2), "?claim 0 button").await;
    send_byo(&mut router, pid(3), "?claim 0 icon").await;

    // Step 3: App sends a button upsert.
    send_byo(&mut router, pid(4), "+button save label=\"Save\"").await;

    // Step 4: Controls daemon should receive ?expand for the button.
    // Compositor should NOT have received anything yet.
    assert_no_message(&mut compositor_rx);

    let expand_msg = recv_byo_raw(&mut controls_rx);
    let expand_cmds = byo::parser::parse(&expand_msg).unwrap();
    assert_eq!(
        expand_cmds.len(),
        1,
        "expected 1 expand request, got: {expand_msg}"
    );
    assert!(
        matches!(
            &expand_cmds[0],
            Command::Request {
                kind: RequestKind::Expand,
                seq: 0,
                targets,
                props,
            } if targets.len() == 1
                && targets[0] == "app:save"
                && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "kind" && value.as_ref() == "button"))
                && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "label" && value.as_ref() == "Save"))
        ),
        "unexpected expand request to controls: {expand_msg}"
    );

    // Icons should NOT have received anything yet (nested expansion hasn't triggered).
    assert_no_message(&mut icons_rx);

    // Step 5: Controls responds with expansion containing +icon (Icons-claimed type).
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view save-root class=\"btn\" { +icon save-icon name=\"check\" } }",
    )
    .await;

    // Step 6: Router detects +icon in Controls' expansion and triggers nested expansion.
    // Icons daemon should now receive ?expand for the icon.
    // Compositor still should NOT have received anything (nested expansion pending).
    assert_no_message(&mut compositor_rx);

    let nested_expand_msg = recv_byo_raw(&mut icons_rx);
    let nested_cmds = byo::parser::parse(&nested_expand_msg).unwrap();
    assert_eq!(
        nested_cmds.len(),
        1,
        "expected 1 nested expand request, got: {nested_expand_msg}"
    );
    assert!(
        matches!(
            &nested_cmds[0],
            Command::Request {
                kind: RequestKind::Expand,
                seq: 0,
                targets,
                props,
            } if targets.len() == 1
                && targets[0] == "controls:save-icon"
                && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "kind" && value.as_ref() == "icon"))
                && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "name" && value.as_ref() == "check"))
        ),
        "unexpected nested expand request to icons: {nested_expand_msg}"
    );

    // Step 7: Icons responds with its expansion.
    send_byo(
        &mut router,
        pid(3),
        ".expand 0 { +image check-img src=\"check.png\" }",
    )
    .await;

    // Step 8: All expansions complete. Compositor should receive the fully rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expected: +view controls:save-root class="btn" { +image icons:check-img src="check.png" }
    // = Upsert(save-root), Push, Upsert(check-img), Pop = 4 commands
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
    assert!(matches!(&cmds[1], Command::Push));
    assert!(
        matches!(
            &cmds[2],
            Command::Upsert { kind, id, props }
            if *kind == "image" && *id == "icons:check-img"
               && props.iter().any(|p| matches!(p, Prop::Value { key, value } if *key == "src" && value.as_ref() == "check.png"))
        ),
        "expected +image icons:check-img src=check.png, got: {s}"
    );
    assert!(matches!(&cmds[3], Command::Pop));

    assert_no_message(&mut compositor_rx);
}

/// Test 29: Three-daemon chain (A -> B -> C).
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. DaemonA claims `widget`, DaemonB claims `gadget`, DaemonC claims `thing`
/// 3. App sends `+widget w1`
/// 4. Router sends `?expand 0 app:w1 kind=widget` to DaemonA
/// 5. DaemonA responds: `.expand 0 { +view wa { +gadget ga } }`
/// 6. Router detects +gadget, sends `?expand 0 daemona:ga kind=gadget` to DaemonB
/// 7. DaemonB responds: `.expand 0 { +view wb { +thing ta } }`
/// 8. Router detects +thing, sends `?expand 0 daemonb:ta kind=thing` to DaemonC
/// 9. DaemonC responds: `.expand 0 { +view wc }`
/// 10. Final output: `+view daemona:wa { +view daemonb:wb { +view daemonc:wc } }`
#[tokio::test]
async fn three_daemon_a_to_b_to_c() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (daemon_a, mut daemon_a_rx) = mock_process(2, "daemona");
    let (daemon_b, mut daemon_b_rx) = mock_process(3, "daemonb");
    let (daemon_c, mut daemon_c_rx) = mock_process(4, "daemonc");
    let (app, mut _app_rx) = mock_process(5, "app");
    router.add_process(compositor);
    router.add_process(daemon_a);
    router.add_process(daemon_b);
    router.add_process(daemon_c);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Step 2: Each daemon claims its type.
    send_byo(&mut router, pid(2), "?claim 0 widget").await;
    send_byo(&mut router, pid(3), "?claim 0 gadget").await;
    send_byo(&mut router, pid(4), "?claim 0 thing").await;

    // Step 3: App sends a widget.
    send_byo(&mut router, pid(5), "+widget w1").await;

    // Step 4: DaemonA receives ?expand.
    assert_no_message(&mut compositor_rx);
    let expand_a = recv_byo_raw(&mut daemon_a_rx);
    let cmds_a = byo::parser::parse(&expand_a).unwrap();
    assert_eq!(cmds_a.len(), 1);
    assert!(
        matches!(
            &cmds_a[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "app:w1"
        ),
        "expected ?expand 0 app:w1, got: {expand_a}"
    );

    // Step 5: DaemonA responds with expansion containing +gadget.
    send_byo(&mut router, pid(2), ".expand 0 { +view wa { +gadget ga } }").await;

    // Step 6: DaemonB receives ?expand for gadget.
    assert_no_message(&mut compositor_rx);
    let expand_b = recv_byo_raw(&mut daemon_b_rx);
    let cmds_b = byo::parser::parse(&expand_b).unwrap();
    assert_eq!(cmds_b.len(), 1);
    assert!(
        matches!(
            &cmds_b[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "daemona:ga"
        ),
        "expected ?expand 0 daemona:ga, got: {expand_b}"
    );

    // Step 7: DaemonB responds with expansion containing +thing.
    send_byo(&mut router, pid(3), ".expand 0 { +view wb { +thing ta } }").await;

    // Step 8: DaemonC receives ?expand for thing.
    assert_no_message(&mut compositor_rx);
    let expand_c = recv_byo_raw(&mut daemon_c_rx);
    let cmds_c = byo::parser::parse(&expand_c).unwrap();
    assert_eq!(cmds_c.len(), 1);
    assert!(
        matches!(
            &cmds_c[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "daemonb:ta"
        ),
        "expected ?expand 0 daemonb:ta, got: {expand_c}"
    );

    // Step 9: DaemonC responds with leaf expansion.
    send_byo(&mut router, pid(4), ".expand 0 { +view wc }").await;

    // Step 10: All expansions complete. Compositor receives the fully rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expected: +view daemona:wa { +view daemonb:wb { +view daemonc:wc } }
    // = Upsert(wa), Push, Upsert(wb), Push, Upsert(wc), Pop, Pop = 7 commands
    assert_eq!(
        cmds.len(),
        7,
        "expected 7 commands, got {}: {s}",
        cmds.len()
    );
    assert!(
        matches!(&cmds[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "daemona:wa"),
        "expected +view daemona:wa, got: {s}"
    );
    assert!(matches!(&cmds[1], Command::Push));
    assert!(
        matches!(&cmds[2], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "daemonb:wb"),
        "expected +view daemonb:wb, got: {s}"
    );
    assert!(matches!(&cmds[3], Command::Push));
    assert!(
        matches!(&cmds[4], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "daemonc:wc"),
        "expected +view daemonc:wc, got: {s}"
    );
    assert!(matches!(&cmds[5], Command::Pop));
    assert!(matches!(&cmds[6], Command::Pop));

    assert_no_message(&mut compositor_rx);
}

/// Test 30: Circular expansion (A -> B -> A).
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. DaemonA claims `type-a`, DaemonB claims `type-b`
/// 3. App sends `+type-a root`
/// 4. DaemonA gets `?expand 0 app:root kind=type-a`, responds:
///    `.expand 0 { +view a-view { +type-b nested-b } }`
/// 5. DaemonB gets `?expand 0 daemona:nested-b kind=type-b`, responds:
///    `.expand 0 { +view b-view { +type-a nested-a } }`
/// 6. DaemonA gets `?expand 1 daemonb:nested-a kind=type-a`, responds with leaf:
///    `.expand 1 { +view a-leaf }`
/// 7. Final: all views qualified, no daemon types remaining.
#[tokio::test]
async fn circular_a_to_b_to_a() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (daemon_a, mut daemon_a_rx) = mock_process(2, "daemona");
    let (daemon_b, mut daemon_b_rx) = mock_process(3, "daemonb");
    let (app, mut _app_rx) = mock_process(4, "app");
    router.add_process(compositor);
    router.add_process(daemon_a);
    router.add_process(daemon_b);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Step 2: DaemonA claims type-a, DaemonB claims type-b.
    send_byo(&mut router, pid(2), "?claim 0 type-a").await;
    send_byo(&mut router, pid(3), "?claim 0 type-b").await;

    // Step 3: App sends +type-a root.
    send_byo(&mut router, pid(4), "+type-a root").await;

    // Step 4: DaemonA receives ?expand 0 for the type-a root.
    assert_no_message(&mut compositor_rx);
    let expand_a0 = recv_byo_raw(&mut daemon_a_rx);
    let cmds_a0 = byo::parser::parse(&expand_a0).unwrap();
    assert_eq!(cmds_a0.len(), 1);
    assert!(
        matches!(
            &cmds_a0[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "app:root"
        ),
        "expected ?expand 0 app:root, got: {expand_a0}"
    );

    // DaemonA responds with expansion containing +type-b.
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view a-view { +type-b nested-b } }",
    )
    .await;

    // Step 5: DaemonB receives ?expand for the nested type-b.
    assert_no_message(&mut compositor_rx);
    let expand_b0 = recv_byo_raw(&mut daemon_b_rx);
    let cmds_b0 = byo::parser::parse(&expand_b0).unwrap();
    assert_eq!(cmds_b0.len(), 1);
    assert!(
        matches!(
            &cmds_b0[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "daemona:nested-b"
        ),
        "expected ?expand 0 daemona:nested-b, got: {expand_b0}"
    );

    // DaemonB responds with expansion containing +type-a (back to DaemonA).
    send_byo(
        &mut router,
        pid(3),
        ".expand 0 { +view b-view { +type-a nested-a } }",
    )
    .await;

    // Step 6: DaemonA receives a SECOND ?expand (seq 1) for the nested-a.
    assert_no_message(&mut compositor_rx);
    let expand_a1 = recv_byo_raw(&mut daemon_a_rx);
    let cmds_a1 = byo::parser::parse(&expand_a1).unwrap();
    assert_eq!(cmds_a1.len(), 1);
    assert!(
        matches!(
            &cmds_a1[0],
            Command::Request { kind: RequestKind::Expand, seq: 1, targets, .. }
            if targets[0] == "daemonb:nested-a"
        ),
        "expected ?expand 1 daemonb:nested-a, got: {expand_a1}"
    );

    // DaemonA responds with a leaf (no more daemon types).
    send_byo(&mut router, pid(2), ".expand 1 { +view a-leaf }").await;

    // Step 7: All expansions complete. Compositor gets the final rewritten output.
    // Expected: +view daemona:a-view { +view daemonb:b-view { +view daemona:a-leaf } }
    let s = recv_byo_raw(&mut compositor_rx);
    byo_assert_eq!(s,
        +view daemona:a-view {
            +view daemonb:b-view {
                +view daemona:a-leaf
            }
        }
    );

    assert_no_message(&mut compositor_rx);
}

/// Test 31: Self-referential expansion (A -> A).
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. DaemonA claims `widget`
/// 3. App sends `+widget root`
/// 4. DaemonA receives `?expand 0 app:root kind=widget`, responds:
///    `.expand 0 { +view outer { +widget sub } }`
/// 5. Router sees +widget sub in DaemonA's own expansion. Since trigger_nested_expansions
///    has NO `!= from` check, A->A is possible. Router sends:
///    `?expand 1 daemona:sub kind=widget` back to DaemonA
/// 6. DaemonA responds with leaf: `.expand 1 { +view inner }`
/// 7. Final: `+view daemona:outer { +view daemona:inner }`
#[tokio::test]
async fn self_referential_a_to_a() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (daemon_a, mut daemon_a_rx) = mock_process(2, "daemona");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(daemon_a);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Step 2: DaemonA claims widget.
    send_byo(&mut router, pid(2), "?claim 0 widget").await;

    // Step 3: App sends +widget root.
    send_byo(&mut router, pid(3), "+widget root").await;

    // Step 4: DaemonA receives ?expand 0 for the widget.
    assert_no_message(&mut compositor_rx);
    let expand_0 = recv_byo_raw(&mut daemon_a_rx);
    let cmds_0 = byo::parser::parse(&expand_0).unwrap();
    assert_eq!(cmds_0.len(), 1);
    assert!(
        matches!(
            &cmds_0[0],
            Command::Request { kind: RequestKind::Expand, seq: 0, targets, .. }
            if targets[0] == "app:root"
        ),
        "expected ?expand 0 app:root, got: {expand_0}"
    );

    // DaemonA responds with expansion containing +widget (self-reference).
    send_byo(
        &mut router,
        pid(2),
        ".expand 0 { +view outer { +widget sub } }",
    )
    .await;

    // Step 5: DaemonA receives a SECOND ?expand (seq 1) for the self-referential widget.
    assert_no_message(&mut compositor_rx);
    let expand_1 = recv_byo_raw(&mut daemon_a_rx);
    let cmds_1 = byo::parser::parse(&expand_1).unwrap();
    assert_eq!(cmds_1.len(), 1);
    assert!(
        matches!(
            &cmds_1[0],
            Command::Request { kind: RequestKind::Expand, seq: 1, targets, .. }
            if targets[0] == "daemona:sub"
        ),
        "expected ?expand 1 daemona:sub, got: {expand_1}"
    );

    // Step 6: DaemonA responds with a leaf (no more widget references).
    send_byo(&mut router, pid(2), ".expand 1 { +view inner }").await;

    // Step 7: All expansions complete. Compositor receives the final rewritten batch.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Expected: +view daemona:outer { +view daemona:inner }
    // = Upsert(outer), Push, Upsert(inner), Pop = 4 commands
    assert_eq!(
        cmds.len(),
        4,
        "expected 4 commands, got {}: {s}",
        cmds.len()
    );
    assert!(
        matches!(&cmds[0], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "daemona:outer"),
        "expected +view daemona:outer, got: {s}"
    );
    assert!(matches!(&cmds[1], Command::Push));
    assert!(
        matches!(&cmds[2], Command::Upsert { kind, id, .. } if *kind == "view" && *id == "daemona:inner"),
        "expected +view daemona:inner, got: {s}"
    );
    assert!(matches!(&cmds[3], Command::Pop));

    assert_no_message(&mut compositor_rx);
}

/// Test 32: Depth limit exceeded — expansion stops at MAX_EXPANSION_DEPTH (64).
///
/// Expected message flow:
/// 1. Compositor observes `view`
/// 2. DaemonA claims `recurse`
/// 3. App sends `+recurse root`
/// 4. DaemonA always responds to ?expand with `+view levelN { +recurse next }`
/// 5. Router stops sending ?expand at depth 64 (MAX_EXPANSION_DEPTH)
/// 6. Batch still completes (doesn't hang)
/// 7. Output contains +view nodes from the expansion chain
///
/// The depth limit prevents infinite recursion. After reaching the limit,
/// remaining +recurse nodes stay as-is (they're claimed but not expanded).
#[tokio::test]
async fn depth_limit_exceeded() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (daemon_a, mut daemon_a_rx) = mock_process(2, "daemona");
    let (app, mut _app_rx) = mock_process(3, "app");
    router.add_process(compositor);
    router.add_process(daemon_a);
    router.add_process(app);

    // Step 1: Compositor observes view.
    send_byo(&mut router, pid(1), "?observe 0 view").await;

    // Step 2: DaemonA claims recurse.
    send_byo(&mut router, pid(2), "?claim 0 recurse").await;

    // Step 3: App sends +recurse root.
    send_byo(&mut router, pid(3), "+recurse root").await;

    // Step 4-5: Respond to ?expand requests in a loop.
    // Each response introduces another +recurse, triggering another ?expand
    // until the depth limit is reached.
    let mut expand_count = 0u64;
    while let Some(msg) = try_recv_byo(&mut daemon_a_rx) {
        let cmds = byo::parser::parse(&msg).unwrap();
        assert!(
            cmds.len() == 1
                && matches!(
                    &cmds[0],
                    Command::Request {
                        kind: RequestKind::Expand,
                        ..
                    }
                ),
            "expected ?expand, got: {msg}"
        );

        // Extract the sequence number from the expand request.
        let seq = match &cmds[0] {
            Command::Request { seq, .. } => *seq,
            _ => unreachable!(),
        };

        // Respond with a view wrapping another +recurse.
        let response = format!(
            ".expand {seq} {{ +view level{expand_count} {{ +recurse next{expand_count} }} }}"
        );
        send_byo(&mut router, pid(2), &response).await;
        expand_count += 1;
    }

    // Step 6: Verify the batch completed (compositor got output) and didn't hang.
    // The expand_count should be around 65 (initial + 64 nested levels).
    assert!(
        expand_count > 0,
        "expected at least some expansion rounds, got 0"
    );
    assert!(
        expand_count <= 66,
        "expected at most ~66 expansion rounds, got {expand_count}"
    );

    // Step 7: Compositor should have received the output.
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();

    // Verify output contains view upserts from the expansion chain.
    let view_upserts: Vec<_> = cmds
        .iter()
        .filter(|c| matches!(c, Command::Upsert { kind, .. } if *kind == "view"))
        .collect();
    assert!(
        !view_upserts.is_empty(),
        "expected +view nodes in output, got: {s}"
    );
    // The first expanded level should be present.
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "daemona:level0")),
        "expected daemona:level0 in output, got: {s}"
    );

    // Batch completed without hanging — the test itself reaching this point proves it.
    assert_no_message(&mut compositor_rx);
}
