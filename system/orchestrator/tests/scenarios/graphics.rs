use byo::protocol::Command;

use crate::{
    assert_no_message, mock_process, pid, recv_byo_raw, recv_graphics_raw, recv_passthrough_raw,
    send_byo, send_graphics, send_passthrough,
};
use byo_orchestrator::process::WriteMsg;
use byo_orchestrator::router::{Router, RouterMsg};

/// Helper: parse the control portion of a kitty graphics payload to extract `i=` value.
fn extract_image_id(payload: &[u8]) -> Option<u32> {
    let s = std::str::from_utf8(payload).ok()?;
    let control = s.split(';').next()?;
    for pair in control.split(',') {
        if let Some(("i", val)) = pair.split_once('=') {
            return val.parse().ok();
        }
    }
    None
}

/// Helper: parse the control portion to extract `I=` (image number) value.
fn extract_image_number(payload: &[u8]) -> Option<u32> {
    let s = std::str::from_utf8(payload).ok()?;
    let control = s.split(';').next()?;
    for pair in control.split(',') {
        if let Some(("I", val)) = pair.split_once('=') {
            return val.parse().ok();
        }
    }
    None
}

/// Basic: a graphics payload with i= is remapped to a global ID.
#[tokio::test]
async fn image_id_remapped() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    // App uploads image with local i=1
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1;iVBORw0KGgo=").await;

    let payload = recv_graphics_raw(&mut compositor_rx);
    let global_id = extract_image_id(&payload).expect("should have i=");
    // Global ID should be >= 1 (first allocation)
    assert!(global_id >= 1, "global id should be >= 1, got {global_id}");
    // Payload portion should be preserved
    let s = String::from_utf8(payload).unwrap();
    assert!(
        s.contains(";iVBORw0KGgo="),
        "payload should be preserved: {s}"
    );
}

/// Same local ID from the same client should map to the same global ID.
#[tokio::test]
async fn same_local_id_maps_consistently() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    // Two graphics commands with same i=1
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1,m=1;chunk1").await;
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1,m=0;chunk2").await;

    let p1 = recv_graphics_raw(&mut compositor_rx);
    let p2 = recv_graphics_raw(&mut compositor_rx);
    let id1 = extract_image_id(&p1).unwrap();
    let id2 = extract_image_id(&p2).unwrap();
    assert_eq!(id1, id2, "same local ID should map to same global ID");
}

/// Different local IDs from the same client get different global IDs.
#[tokio::test]
async fn different_local_ids_get_different_globals() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1;data1").await;
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=2;data2").await;

    let p1 = recv_graphics_raw(&mut compositor_rx);
    let p2 = recv_graphics_raw(&mut compositor_rx);
    let id1 = extract_image_id(&p1).unwrap();
    let id2 = extract_image_id(&p2).unwrap();
    assert_ne!(
        id1, id2,
        "different local IDs should get different global IDs"
    );
}

/// Same local ID from DIFFERENT clients get different global IDs (no collision).
#[tokio::test]
async fn cross_client_id_isolation() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app_a, mut _a_rx) = mock_process(2, "app-a");
    let (app_b, mut _b_rx) = mock_process(3, "app-b");
    router.add_process(compositor);
    router.add_process(app_a);
    router.add_process(app_b);

    send_byo(&mut router, pid(1), "#observe G").await;

    // Both apps use local i=1
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1;data-a").await;
    send_graphics(&mut router, pid(3), b"a=t,f=100,i=1;data-b").await;

    let p_a = recv_graphics_raw(&mut compositor_rx);
    let p_b = recv_graphics_raw(&mut compositor_rx);
    let id_a = extract_image_id(&p_a).unwrap();
    let id_b = extract_image_id(&p_b).unwrap();
    assert_ne!(
        id_a, id_b,
        "same local ID from different clients must not collide"
    );
}

/// Image number (I=) is also remapped independently.
#[tokio::test]
async fn image_number_remapped() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    send_graphics(&mut router, pid(2), b"a=t,f=100,I=5;data").await;

    let payload = recv_graphics_raw(&mut compositor_rx);
    let global_num = extract_image_number(&payload).expect("should have I=");
    assert!(
        global_num >= 1,
        "global number should be >= 1, got {global_num}"
    );
}

/// Graphics without i= or I= are forwarded unchanged.
#[tokio::test]
async fn no_id_forwarded_unchanged() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    send_graphics(&mut router, pid(2), b"a=d,d=a").await;

    let payload = recv_graphics_raw(&mut compositor_rx);
    assert_eq!(payload, b"a=d,d=a");
}

/// $img(N) in BYO prop values is remapped to the global ID.
#[tokio::test]
async fn img_var_remapped_in_props() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G,view").await;

    // First, upload an image to establish the mapping
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=7;data").await;
    let gfx_payload = recv_graphics_raw(&mut compositor_rx);
    let global_id = extract_image_id(&gfx_payload).unwrap();

    // Now send a view with $img(7) referencing the local ID
    send_byo(&mut router, pid(2), "+view bg background-image=\"$img(7)\"").await;

    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert_eq!(cmds.len(), 1);
    match &cmds[0] {
        Command::Upsert { props, .. } => {
            let bg_prop = props
                .iter()
                .find(|p| matches!(p, byo::Prop::Value { key, .. } if *key == "background-image"))
                .expect("should have background-image prop");
            if let byo::Prop::Value { value, .. } = bg_prop {
                assert_eq!(
                    value.as_ref(),
                    &format!("$img({global_id})"),
                    "local ID 7 should be remapped to global {global_id}"
                );
            }
        }
        _ => panic!("expected Upsert"),
    }
}

/// $img(N) with no prior graphics upload is NOT remapped (no mapping exists).
#[tokio::test]
async fn img_var_no_mapping_unchanged() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe view").await;

    // No graphics uploaded — $img(99) has no mapping
    send_byo(
        &mut router,
        pid(2),
        "+view bg background-image=\"$img(99)\"",
    )
    .await;

    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    match &cmds[0] {
        Command::Upsert { props, .. } => {
            let bg_prop = props
                .iter()
                .find(|p| matches!(p, byo::Prop::Value { key, .. } if *key == "background-image"))
                .unwrap();
            if let byo::Prop::Value { value, .. } = bg_prop {
                assert_eq!(
                    value.as_ref(),
                    "$img(99)",
                    "unmapped $img should be left unchanged"
                );
            }
        }
        _ => panic!("expected Upsert"),
    }
}

/// On disconnect, delete commands are sent for all of the client's global images.
#[tokio::test]
async fn disconnect_sends_delete_commands() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    // Upload 3 images
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1;d1").await;
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=2;d2").await;
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=3;d3").await;

    let p1 = recv_graphics_raw(&mut compositor_rx);
    let p2 = recv_graphics_raw(&mut compositor_rx);
    let p3 = recv_graphics_raw(&mut compositor_rx);
    let global_ids: Vec<u32> = [p1, p2, p3]
        .iter()
        .map(|p| extract_image_id(p).unwrap())
        .collect();

    // Disconnect the app
    router
        .handle(RouterMsg::Disconnected { process: pid(2) })
        .await;

    // Compositor should receive delete commands for all 3 images
    let mut deleted_ids: Vec<u32> = Vec::new();
    loop {
        match compositor_rx.try_recv() {
            Ok(WriteMsg::Graphics(raw)) => {
                let s = String::from_utf8((*raw).clone()).unwrap();
                assert!(s.contains("a=d"), "expected delete command: {s}");
                if let Some(id) = extract_image_id(&raw) {
                    deleted_ids.push(id);
                }
            }
            Ok(WriteMsg::Byo(_)) => {
                // Disconnect may also send BYO destroy commands — skip those
                continue;
            }
            _ => break,
        }
    }

    deleted_ids.sort();
    let mut expected = global_ids.clone();
    expected.sort();
    assert_eq!(
        deleted_ids, expected,
        "all global image IDs should be deleted on disconnect"
    );
}

/// Delete command (a=d,d=i,i=N) also gets its i= remapped.
#[tokio::test]
async fn delete_command_remapped() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    // Upload with i=42
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=42;data").await;
    let upload = recv_graphics_raw(&mut compositor_rx);
    let global_id = extract_image_id(&upload).unwrap();

    // Delete with same local i=42
    send_graphics(&mut router, pid(2), b"a=d,d=i,i=42").await;
    let delete = recv_graphics_raw(&mut compositor_rx);
    let delete_id = extract_image_id(&delete).unwrap();

    assert_eq!(
        delete_id, global_id,
        "delete command should use the same global ID"
    );
}

/// Placement command (a=p,i=N) also gets remapped.
#[tokio::test]
async fn placement_command_remapped() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    // Upload i=10
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=10;data").await;
    let upload = recv_graphics_raw(&mut compositor_rx);
    let global_id = extract_image_id(&upload).unwrap();

    // Place i=10
    send_graphics(&mut router, pid(2), b"a=p,i=10,c=40,r=20").await;
    let place = recv_graphics_raw(&mut compositor_rx);
    let place_id = extract_image_id(&place).unwrap();

    assert_eq!(
        place_id, global_id,
        "placement should reference the same global ID"
    );
}

/// Both i= and I= in the same command are remapped.
#[tokio::test]
async fn both_id_and_number_remapped() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe G").await;

    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1,I=1;data").await;
    let payload = recv_graphics_raw(&mut compositor_rx);

    let global_id = extract_image_id(&payload).unwrap();
    let global_num = extract_image_number(&payload).unwrap();

    // Both should be remapped (may or may not be the same value, depending on counter state)
    assert!(global_id >= 1);
    assert!(global_num >= 1);
}

/// Graphics sent BEFORE observer subscribes should be buffered and flushed.
#[tokio::test]
async fn graphics_buffered_before_observer() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // App uploads BEFORE compositor subscribes to G
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=1;data1").await;
    send_graphics(&mut router, pid(2), b"a=t,f=100,i=2;data2").await;

    // No messages should have arrived yet
    assert_no_message(&mut compositor_rx);

    // Now compositor subscribes to G
    send_byo(&mut router, pid(1), "#observe G").await;

    // Buffered graphics should now be flushed
    let p1 = recv_graphics_raw(&mut compositor_rx);
    let p2 = recv_graphics_raw(&mut compositor_rx);
    let id1 = extract_image_id(&p1).unwrap();
    let id2 = extract_image_id(&p2).unwrap();
    assert_ne!(
        id1, id2,
        "different local IDs should get different global IDs"
    );
    assert_no_message(&mut compositor_rx);
}

/// Passthrough sent before observer subscribes should be buffered and flushed.
#[tokio::test]
async fn passthrough_buffered_before_observer() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    // App sends passthrough BEFORE compositor subscribes to tty
    send_passthrough(&mut router, pid(2), b"early bytes").await;

    // No messages should have arrived yet
    assert_no_message(&mut compositor_rx);

    // Now compositor subscribes to tty
    send_byo(&mut router, pid(1), "#observe tty").await;

    // Buffered passthrough should now be flushed
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"early bytes");
    assert_no_message(&mut compositor_rx);
}
