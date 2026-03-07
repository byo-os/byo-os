use byo::protocol::Command;

use crate::{
    assert_no_message, mock_process, pid, recv_byo_raw, recv_passthrough_raw, send_byo,
    send_passthrough,
};
use byo_orchestrator::router::Router;

/// Default passthrough target is "/" (root tty). Passthrough bytes
/// should be forwarded to view observers without any redirect frame.
#[tokio::test]
async fn default_target_is_root() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty").await;

    // Passthrough from app with default target "/"
    send_passthrough(&mut router, pid(2), b"hello world").await;

    // Should receive passthrough directly — no redirect frame needed since
    // last_forwarded_target starts as "/".
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"hello world");
    assert_no_message(&mut compositor_rx);
}

/// When a client sets #redirect, subsequent passthrough from that client
/// should be preceded by a redirect BYO frame.
#[tokio::test]
async fn redirect_injects_frame() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty").await;

    // App redirects to "term-a"
    send_byo(&mut router, pid(2), "#redirect term-a").await;

    // Now send passthrough — should get a redirect frame first
    send_passthrough(&mut router, pid(2), b"data for term-a").await;

    // First message: BYO redirect pragma
    let redirect = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&redirect).unwrap();
    assert!(
        matches!(&cmds[0], Command::Pragma(byo::PragmaKind::Redirect(target))
            if target.as_ref() == "app:term-a"
        ),
        "expected #redirect app:term-a, got: {redirect}"
    );

    // Second message: passthrough data
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"data for term-a");
}

/// Consecutive passthrough from the same client with the same target
/// should NOT inject redundant redirect frames.
#[tokio::test]
async fn no_redundant_redirect() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty").await;
    send_byo(&mut router, pid(2), "#redirect term-a").await;

    // First passthrough — redirect frame + data
    send_passthrough(&mut router, pid(2), b"chunk1").await;
    let _redirect = recv_byo_raw(&mut compositor_rx);
    let _data = recv_passthrough_raw(&mut compositor_rx);

    // Second passthrough from same client — no redirect (already set)
    send_passthrough(&mut router, pid(2), b"chunk2").await;
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"chunk2");
    assert_no_message(&mut compositor_rx);
}

/// Interleaved passthrough from different clients should inject redirect
/// frames when switching.
#[tokio::test]
async fn interleaved_clients_switch_redirect() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app_a, mut _a_rx) = mock_process(2, "app-a");
    let (app_b, mut _b_rx) = mock_process(3, "app-b");
    router.add_process(compositor);
    router.add_process(app_a);
    router.add_process(app_b);

    send_byo(&mut router, pid(1), "#observe tty").await;
    send_byo(&mut router, pid(2), "#redirect term-a").await;
    send_byo(&mut router, pid(3), "#redirect term-b").await;

    // App A passthrough
    send_passthrough(&mut router, pid(2), b"from-a").await;
    let redirect_a = recv_byo_raw(&mut compositor_rx);
    assert!(
        redirect_a.contains("app-a:term-a"),
        "expected redirect to app-a:term-a, got: {redirect_a}"
    );
    let data_a = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data_a, b"from-a");

    // App B passthrough — should switch redirect
    send_passthrough(&mut router, pid(3), b"from-b").await;
    let redirect_b = recv_byo_raw(&mut compositor_rx);
    assert!(
        redirect_b.contains("app-b:term-b"),
        "expected redirect to app-b:term-b, got: {redirect_b}"
    );
    let data_b = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data_b, b"from-b");

    // App A again — should switch back
    send_passthrough(&mut router, pid(2), b"from-a-again").await;
    let redirect_a2 = recv_byo_raw(&mut compositor_rx);
    assert!(
        redirect_a2.contains("app-a:term-a"),
        "expected redirect back to app-a:term-a, got: {redirect_a2}"
    );
    let data_a2 = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data_a2, b"from-a-again");
}

/// #redirect _ should discard passthrough bytes.
#[tokio::test]
async fn redirect_discard() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty").await;
    send_byo(&mut router, pid(2), "#redirect _").await;

    // Passthrough should be discarded
    send_passthrough(&mut router, pid(2), b"should be discarded").await;
    assert_no_message(&mut compositor_rx);
}

/// #unredirect should restore target to "/".
#[tokio::test]
async fn unredirect_restores_root() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty").await;

    // Redirect then unredirect
    send_byo(&mut router, pid(2), "#redirect term-x").await;
    send_passthrough(&mut router, pid(2), b"to-term-x").await;
    let _ = recv_byo_raw(&mut compositor_rx); // redirect frame
    let _ = recv_passthrough_raw(&mut compositor_rx); // data

    send_byo(&mut router, pid(2), "#unredirect").await;
    send_passthrough(&mut router, pid(2), b"to-root").await;

    // Should get an #unredirect frame then data
    let unredirect = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&unredirect).unwrap();
    assert!(
        matches!(&cmds[0], Command::Pragma(byo::PragmaKind::Unredirect)),
        "expected #unredirect, got: {unredirect}"
    );
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"to-root");
}

/// Redirect pragmas should NOT be forwarded to observers in BYO frames.
#[tokio::test]
async fn redirect_pragma_not_forwarded_to_observers() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app, mut _app_rx) = mock_process(2, "app");
    router.add_process(compositor);
    router.add_process(app);

    send_byo(&mut router, pid(1), "#observe tty,view").await;

    // Send a view + redirect in same batch
    send_byo(&mut router, pid(2), "+view foo #redirect term-a").await;

    // Compositor should only see the view upsert, not the redirect pragma
    let s = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&s).unwrap();
    assert!(
        cmds.iter()
            .all(|c| !matches!(c, Command::Pragma(byo::PragmaKind::Redirect(_)))),
        "redirect pragma should not be forwarded: {s}"
    );
    assert!(
        cmds.iter()
            .any(|c| matches!(c, Command::Upsert { id, .. } if *id == "app:foo")),
        "view upsert should still be forwarded: {s}"
    );
}

/// Client with no explicit redirect uses default "/" target.
/// When interleaved with a client that has a redirect, proper switching occurs.
#[tokio::test]
async fn default_and_redirected_interleave() {
    let mut router = Router::new();
    let (compositor, mut compositor_rx) = mock_process(1, "compositor");
    let (app_default, mut _d_rx) = mock_process(2, "app-default");
    let (app_custom, mut _c_rx) = mock_process(3, "app-custom");
    router.add_process(compositor);
    router.add_process(app_default);
    router.add_process(app_custom);

    send_byo(&mut router, pid(1), "#observe tty").await;
    send_byo(&mut router, pid(3), "#redirect my-tty").await;

    // Default app passthrough (target "/") — matches initial last_forwarded_target
    send_passthrough(&mut router, pid(2), b"default-data").await;
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"default-data");
    assert_no_message(&mut compositor_rx); // no redirect needed

    // Custom app passthrough — needs redirect
    send_passthrough(&mut router, pid(3), b"custom-data").await;
    let redirect = recv_byo_raw(&mut compositor_rx);
    assert!(
        redirect.contains("app-custom:my-tty"),
        "expected app-custom:my-tty, got: {redirect}"
    );
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"custom-data");

    // Back to default app — needs unredirect
    send_passthrough(&mut router, pid(2), b"default-again").await;
    let unredirect = recv_byo_raw(&mut compositor_rx);
    let cmds = byo::parser::parse(&unredirect).unwrap();
    assert!(
        matches!(&cmds[0], Command::Pragma(byo::PragmaKind::Unredirect)),
        "expected #unredirect when switching back to root: {unredirect}"
    );
    let data = recv_passthrough_raw(&mut compositor_rx);
    assert_eq!(data, b"default-again");
}
