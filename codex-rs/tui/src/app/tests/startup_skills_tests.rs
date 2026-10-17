//! Session attachment must keep the prompt usable while skill metadata is loading.

use super::*;
use codex_app_server_protocol::JSONRPCError;
use codex_app_server_protocol::JSONRPCErrorError;
use codex_app_server_protocol::JSONRPCMessage;
use codex_app_server_protocol::JSONRPCResponse;
use futures::SinkExt;
use futures::StreamExt;
use pretty_assertions::assert_eq;
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::Message;

async fn delayed_skills_server(
    config: &Config,
    started: oneshot::Sender<serde_json::Value>,
    mut release: oneshot::Receiver<()>,
    delivered: oneshot::Sender<()>,
) -> Result<(AppServerSession, JoinHandle<Result<()>>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let websocket_url = format!("ws://{}", listener.local_addr()?);
    let codex_home = config.codex_home.display().to_string();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await?;
        let mut websocket = accept_async(stream).await?;
        let mut started = Some(started);
        let mut delivered = Some(delivered);
        let mut pending: Option<JSONRPCMessage> = None;
        loop {
            let frame = tokio::select! {
                _ = &mut release, if pending.is_some() => {
                    let response = pending.take().expect("held skills response");
                    websocket.send(Message::Text(serde_json::to_string(&response)?.into())).await?;
                    let _ = delivered.take().expect("skills response delivery").send(());
                    continue;
                }
                frame = websocket.next() => frame,
            };
            let Some(Ok(Message::Text(text))) = frame else {
                break;
            };
            let JSONRPCMessage::Request(request) = serde_json::from_str(&text)? else {
                continue;
            };
            let response = match request.method.as_str() {
                "initialize" => JSONRPCMessage::Response(JSONRPCResponse {
                    id: request.id,
                    result: serde_json::json!({
                        "userAgent": "codex-tui-test",
                        "codexHome": codex_home,
                    }),
                }),
                "skills/list" => {
                    let response = JSONRPCMessage::Response(JSONRPCResponse {
                        id: request.id,
                        result: serde_json::json!({"data": []}),
                    });
                    if let Some(started) = started.take() {
                        let _ = started.send(request.params.expect("skills/list params"));
                        pending = Some(response);
                        continue;
                    }
                    response
                }
                _ => JSONRPCMessage::Error(JSONRPCError {
                    id: request.id,
                    error: JSONRPCErrorError {
                        code: -32601,
                        message: "method not supported by skills fixture".into(),
                        data: None,
                    },
                }),
            };
            websocket
                .send(Message::Text(serde_json::to_string(&response)?.into()))
                .await?;
        }
        Ok(())
    });
    let client = crate::connect_remote_app_server(crate::RemoteAppServerEndpoint::WebSocket {
        websocket_url,
        auth_token: None,
    })
    .await?;
    Ok((
        AppServerSession::new(
            crate::backend_id::BackendId::new(),
            client,
            crate::app_server_session::ThreadParamsMode::Remote,
        )
        .with_local_codex_home(&config.codex_home),
        server,
    ))
}

#[tokio::test]
async fn session_attachment_keeps_prompt_usable_while_skills_load() -> Result<()> {
    let (mut app, mut events, mut ops) = make_test_app_with_channels().await;
    let mut tui = crate::tui::test_support::make_test_tui()?;
    let (started_tx, started_rx) = oneshot::channel();
    let (release_tx, release_rx) = oneshot::channel();
    let (delivered_tx, delivered_rx) = oneshot::channel();
    let (mut server, proxy) =
        delayed_skills_server(&app.config, started_tx, release_rx, delivered_tx).await?;
    let thread_id = ThreadId::new();
    let cwd = app.config.cwd.to_path_buf();
    app.active_thread_id = Some(thread_id);
    app.chat_widget
        .handle_thread_session_quiet(test_thread_session(thread_id, cwd.clone()));
    // The manual widget fixture sends commands to a separate channel. Route them through the
    // same app-event path used by the running TUI.
    while let Ok(op) = ops.try_recv() {
        app.app_event_tx.send(AppEvent::CodexOp(op));
    }
    let handled = tokio::time::timeout(Duration::from_secs(2), async {
        while let Ok(event) = events.try_recv() {
            app.handle_event(&mut tui, &mut server, event).await?;
        }
        Ok::<_, color_eyre::Report>(())
    })
    .await;
    let params = tokio::time::timeout(Duration::from_secs(5), started_rx).await??;
    for character in "a你".chars() {
        app.chat_widget
            .handle_key_event(KeyCode::Char(character).into());
    }
    let text = app.chat_widget.composer_text_with_pending();
    let _ = release_tx.send(());
    tokio::time::timeout(Duration::from_secs(5), delivered_rx).await??;
    server.shutdown().await?;
    proxy.await??;

    handled.expect("automatic skills refresh blocked the app event loop")?;
    assert_eq!(params["forceReload"], true);
    assert_eq!(params["cwds"], serde_json::json!([cwd]));
    assert_eq!(text, "a你");
    Ok(())
}
