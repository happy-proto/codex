use super::*;
use crate::api_bridge::map_api_error;
use pretty_assertions::assert_eq;
use tokio_tungstenite::WebSocketStream;
use tungstenite::protocol::Role;
use tungstenite::protocol::frame::coding::CloseCode;

async fn socket_pair() -> (WsStream, WebSocketStream<tokio::io::DuplexStream>) {
    let (client, server) = tokio::io::duplex(256);
    let client = WebSocketStream::from_raw_socket(client, Role::Client, None).await;
    let server = WebSocketStream::from_raw_socket(server, Role::Server, None).await;
    (WsStream::new(client, None), server)
}

async fn run_response(ws: &mut WsStream) -> (Result<(), ApiError>, Vec<ResponseEvent>) {
    let (tx, mut rx) = mpsc::channel(16);
    let context = ResponsesWebsocketTimingLogContext {
        model: "test".into(),
        session_id: None,
        thread_id: None,
        turn_id: None,
        traceparent: None,
        previous_response_id: None,
        request_start_ms: None,
        warmup: false,
        connection_reused: false,
    };
    let result = run_websocket_response_stream(
        ws,
        tx,
        "{}".into(),
        Duration::from_secs(1),
        None,
        None,
        &context,
        std::future::pending::<ResponseInterrupt>().shared(),
        &mut None,
    )
    .await;
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        events.push(event.expect("response event"));
    }
    (result, events)
}

#[tokio::test]
async fn websocket_close_preserves_code_reason_and_retry_policy() {
    for (code, reason) in [
        (1009, ""),
        (1009, "too large"),
        (1009, "bad\n\u{1b}[31m"),
        (1013, "try later"),
    ] {
        let (mut client, mut server) = socket_pair().await;
        let peer = tokio::spawn(async move {
            server.next().await.expect("request").expect("text");
            server
                .send(Message::Close(Some(CloseFrame {
                    code: CloseCode::from(code),
                    reason: reason.into(),
                })))
                .await
                .expect("close");
        });
        let error = run_response(&mut client).await.0.expect_err("server close");
        let error = map_api_error(error);
        assert!(error.to_string().contains(&code.to_string()), "{error}");
        assert!(
            error
                .to_string()
                .contains(&reason.escape_debug().to_string()),
            "{error}"
        );
        assert!(!error.to_string().contains(['\n', '\u{1b}']), "{error}");
        assert_eq!(error.retry_delay(1).is_some(), code != 1009);
        if code == 1009 {
            assert!(error.to_string().contains("Reduce"), "{error}");
        }
        peer.await.expect("peer");
    }
}

#[tokio::test]
async fn websocket_close_during_blocked_upload_is_terminal() {
    let (mut client, mut server) = socket_pair().await;
    let send = send_websocket_request(
        &mut client,
        "x".repeat(64 * 1024),
        Duration::from_secs(1),
        None,
        false,
    );
    tokio::pin!(send);
    // The request exceeds the duplex buffer; the peer closes without consuming it.
    assert!(futures::poll!(&mut send).is_pending());
    tokio::time::timeout(
        Duration::from_secs(1),
        tokio::io::AsyncReadExt::read_exact(server.get_mut(), &mut [0u8; 1]),
    )
    .await
    .expect("upload started")
    .expect("request frame prefix");
    server
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::Size,
            reason: "too large".into(),
        })))
        .await
        .expect("close during upload");
    let error = map_api_error(send.await.expect_err("upload rejected"));
    assert!(error.to_string().contains("1009"), "{error}");
    assert_eq!(error.retry_delay(1), None);
}

#[tokio::test]
async fn websocket_close_queued_before_send_is_not_discarded() {
    let (mut client, mut server) = socket_pair().await;
    server
        .send(Message::Close(Some(CloseFrame {
            code: CloseCode::Size,
            reason: "too large".into(),
        })))
        .await
        .expect("close");
    // Wait until the pump has observed the close before submitting a request.
    tokio::time::timeout(Duration::from_secs(1), async {
        while !client.pump_task.is_finished() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .expect("pump close");
    let error =
        send_websocket_request(&mut client, "{}".into(), Duration::from_secs(1), None, true)
            .await
            .expect_err("closed socket");
    let error = map_api_error(error);
    assert!(error.to_string().contains("1009"), "{error}");
    assert_eq!(error.retry_delay(1), None);
}

#[tokio::test]
async fn websocket_close_without_frame_remains_retryable() {
    let (mut client, mut server) = socket_pair().await;
    let peer = tokio::spawn(async move {
        server.next().await.expect("request").expect("text");
        server.send(Message::Close(None)).await.expect("close");
    });
    let error = run_response(&mut client).await.0.expect_err("close");
    assert!(map_api_error(error).retry_delay(1).is_some());
    peer.await.expect("peer");
}

#[tokio::test]
async fn websocket_close_after_completed_keeps_success() {
    let (mut client, mut server) = socket_pair().await;
    let peer = tokio::spawn(async move {
        server.next().await.expect("request").expect("text");
        server
            .send(Message::Text(
                serde_json::json!({"type": "response.completed", "response": {
                    "id": "resp_1", "output": []
                }})
                .to_string()
                .into(),
            ))
            .await
            .expect("completed");
        server.send(Message::Close(None)).await.expect("close");
    });
    let (result, events) = run_response(&mut client).await;
    result.expect("completed response");
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ResponseEvent::Completed { .. }))
    );
    peer.await.expect("peer");
}

#[tokio::test]
async fn websocket_close_pump_still_responds_to_ping() {
    let (_client, mut server) = socket_pair().await;
    server
        .send(Message::Ping(b"heartbeat".to_vec().into()))
        .await
        .expect("ping");
    let pong = tokio::time::timeout(Duration::from_secs(1), server.next())
        .await
        .expect("pong deadline")
        .expect("pong message")
        .expect("pong");
    assert_eq!(pong, Message::Pong(b"heartbeat".to_vec().into()));
}

#[test]
fn websocket_close_reason_has_bounded_safe_output() {
    let error = map_ws_close(Some(CloseFrame {
        code: CloseCode::Size,
        reason: "\u{1b}".repeat(120).into(),
    }));
    let ApiError::WebSocketClosed { code, reason } = error else {
        panic!("expected structured close error");
    };
    assert_eq!(code, 1009);
    assert_eq!(reason.len(), 256);
    assert!(!reason.contains('\u{1b}'));
}
