use std::collections::HashMap;
use std::time::Duration;

use anyhow::Result;
use app_test_support::MockResponsesConfig;
use app_test_support::TestAppServer;
use app_test_support::create_mock_responses_server_repeating_assistant;
use axum::Json;
use axum::Router;
use axum::http::HeaderMap;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::post;
use codex_app_server_protocol::ClientRequest;
use codex_app_server_protocol::McpServerRefreshResponse;
use codex_app_server_protocol::McpServerToolCallParams;
use codex_app_server_protocol::McpServerToolCallResponse;
use codex_app_server_protocol::ThreadResumeParams;
use codex_app_server_protocol::ThreadResumeResponse;
use codex_app_server_protocol::ThreadStartParams;
use codex_app_server_protocol::TurnInterruptParams;
use codex_app_server_protocol::TurnInterruptResponse;
use codex_app_server_protocol::TurnStartParams;
use codex_app_server_protocol::TurnStartResponse;
use codex_app_server_protocol::UserInput;
use pretty_assertions::assert_eq;
use serde_json::Value;
use serde_json::json;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use wiremock::Mock;
use wiremock::matchers::method;
use wiremock::matchers::path_regex;

use super::connection_handling_websocket::connect_websocket;
use super::connection_handling_websocket::read_error_for_id;
use super::connection_handling_websocket::read_notification_for_method;
use super::connection_handling_websocket::read_response_for_id;
use super::connection_handling_websocket::send_initialize_request;
use super::connection_handling_websocket::send_request;
use super::connection_handling_websocket::spawn_websocket_server;

#[tokio::test]
async fn task_mcp_owner_conflict_allows_followup_without_rebinding() -> Result<()> {
    let model = create_mock_responses_server_repeating_assistant("Done").await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&model.uri()).write(home.path())?;
    let (old_url, old_task) = task_server("Bearer old").await?;
    let (new_url, new_task) = task_server("Bearer new").await?;
    let (mut process, addr) = spawn_websocket_server(home.path()).await?;
    let mut owner = connect_websocket(addr).await?;
    let mut sender = connect_websocket(addr).await?;
    send_initialize_request(&mut owner, 1, "task_owner").await?;
    read_response_for_id(&mut owner, 1).await?;
    send_initialize_request(&mut sender, 1, "task_sender").await?;
    read_response_for_id(&mut sender, 1).await?;

    send_request(
        &mut owner,
        "thread/start",
        2,
        Some(json!({
            "config": registration(&old_url, "Bearer old")
        })),
    )
    .await?;
    let started = read_response_for_id(&mut owner, 2).await?;
    let thread_id = &started.result["thread"]["id"];
    assert!(thread_id.is_string());

    send_request(
        &mut owner,
        "turn/start",
        3,
        Some(json!({
            "threadId": thread_id, "input": [{"type": "text", "text": "Materialize the thread"}]
        })),
    )
    .await?;
    read_response_for_id(&mut owner, 3).await?;
    read_notification_for_method(&mut owner, "turn/completed").await?;

    send_request(&mut sender, "thread/resume", 2, Some(json!({
        "threadId": thread_id, "excludeTurns": true, "config": registration(&new_url, "Bearer new")
    }))).await?;
    let conflict = read_error_for_id(&mut sender, 2).await?;
    assert_eq!(conflict.error.code, -32600);
    assert_eq!(
        conflict.error.data,
        Some(json!({"reason": "tuiMcpOwnerConnected"}))
    );

    send_request(
        &mut sender,
        "thread/resume",
        3,
        Some(json!({
            "threadId": thread_id, "excludeTurns": true
        })),
    )
    .await?;
    read_response_for_id(&mut sender, 3).await?;
    send_request(
        &mut sender,
        "turn/start",
        4,
        Some(json!({
            "threadId": thread_id, "input": [], "toolOutput": {
                "name": "send_message_to_thread", "namespace": "codex_tui",
                "retain": true, "output": "Follow up"
            }
        })),
    )
    .await?;
    read_response_for_id(&mut sender, 4).await?;
    send_request(
        &mut sender,
        "mcpServer/tool/call",
        5,
        Some(json!({
            "threadId": thread_id, "server": "codex_tui", "tool": "probe", "arguments": {}
        })),
    )
    .await?;
    let probe = read_response_for_id(&mut sender, 5).await?;
    assert_eq!(probe.result["content"][0]["text"], "Bearer old");

    old_task.abort();
    new_task.abort();
    process.kill().await?;
    Ok(())
}

async fn task_server(marker: &'static str) -> Result<(String, JoinHandle<()>)> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let url = format!("http://{}/mcp", listener.local_addr()?);
    let router = Router::new().route(
        "/mcp",
        post(
            move |headers: HeaderMap, Json(message): Json<Value>| async move {
                if headers.get("Authorization").and_then(|v| v.to_str().ok()) != Some(marker) {
                    return StatusCode::UNAUTHORIZED.into_response();
                }
                if message.get("id").is_none() {
                    return StatusCode::ACCEPTED.into_response();
                }
                let result = match message["method"].as_str() {
                    Some("initialize") => json!({
                        "protocolVersion": message["params"]["protocolVersion"],
                        "capabilities": {"tools": {}},
                        "serverInfo": {"name": "task-fixture", "version": "1"}
                    }),
                    Some("tools/list") => json!({"tools": [{
                        "name": "probe", "description": "Return server identity",
                        "inputSchema": {"type": "object", "properties": {}}
                    }]}),
                    Some("tools/call") => {
                        json!({"content": [{"type": "text", "text": marker}], "isError": false})
                    }
                    _ => json!({}),
                };
                Json(json!({"jsonrpc": "2.0", "id": message["id"], "result": result}))
                    .into_response()
            },
        ),
    );
    let task = tokio::spawn(async move {
        axum::serve(listener, router).await.unwrap();
    });
    Ok((url, task))
}

fn registration(url: &str, marker: &str) -> HashMap<String, Value> {
    HashMap::from([(
        "mcp_servers.codex_tui".into(),
        json!({
            "url": url, "http_headers": {"Authorization": marker},
            "default_tools_approval_mode": "approve"
        }),
    )])
}

async fn probe(app: &mut TestAppServer, thread_id: &str) -> Result<McpServerToolCallResponse> {
    app.request(|request_id| ClientRequest::McpServerToolCall {
        request_id,
        params: McpServerToolCallParams {
            thread_id: thread_id.into(),
            server: "codex_tui".into(),
            tool: "probe".into(),
            environment_id: None,
            arguments: Some(json!({})),
            meta: None,
        },
    })
    .await
}

#[test_case::test_case(true, false; "stale idle registration")]
#[test_case::test_case(true, true; "stale active registration")]
#[test_case::test_case(false, true; "registration absent during active recovery")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn task_mcp_resume_replaces_endpoint_and_token_and_survives_reload(
    registered: bool,
    active: bool,
) -> Result<()> {
    let model = create_mock_responses_server_repeating_assistant("Done").await;
    let home = TempDir::new()?;
    MockResponsesConfig::new(&model.uri()).write(home.path())?;
    let (old_url, old_task) = task_server("Bearer old").await?;
    let (new_url, new_task) = task_server("Bearer new").await?;
    let mut app = TestAppServer::builder()
        .with_codex_home(home.path())
        .build_initialized()
        .await?;
    let started = app
        .start_thread(ThreadStartParams {
            config: registered.then(|| registration(&old_url, "Bearer old")),
            ..Default::default()
        })
        .await?;
    let thread_id = started.thread.id;
    // Materialize a resumable rollout while keeping the original thread loaded/subscribed.
    let _: TurnStartResponse = app
        .request(|request_id| ClientRequest::TurnStart {
            request_id,
            params: TurnStartParams {
                thread_id: thread_id.clone(),
                input: vec![UserInput::Text {
                    text: "materialize".into(),
                    text_elements: vec![],
                }],
                ..Default::default()
            },
        })
        .await?;
    app.read_notification::<codex_app_server_protocol::TurnCompletedNotification>("turn/completed")
        .await?;
    if registered {
        assert_eq!(
            probe(&mut app, &thread_id).await?.content[0]["text"],
            "Bearer old"
        );
    }
    let pending_turn = if active {
        model.reset().await;
        let body = core_test_support::responses::sse(vec![
            core_test_support::responses::ev_response_created("held"),
            core_test_support::responses::ev_completed("held"),
        ]);
        let pending_request = Mock::given(method("POST"))
            .and(path_regex(".*/responses$"))
            .respond_with(
                core_test_support::responses::sse_response(body).set_delay(Duration::from_secs(30)),
            )
            .expect(1)
            .mount_as_scoped(&model)
            .await;
        let started: TurnStartResponse = app
            .request(|request_id| ClientRequest::TurnStart {
                request_id,
                params: TurnStartParams {
                    thread_id: thread_id.clone(),
                    input: vec![UserInput::Text {
                        text: "hold active".into(),
                        text_elements: vec![],
                    }],
                    ..Default::default()
                },
            })
            .await?;
        // Establish an in-flight model request before resuming. The resume summary
        // can lag the live turn's notifications and is not the rebind contract.
        tokio::time::timeout(
            Duration::from_secs(10),
            pending_request.wait_until_satisfied(),
        )
        .await?;
        Some((started.turn.id, pending_request))
    } else {
        None
    };
    let resumed: ThreadResumeResponse = app
        .request(|request_id| ClientRequest::ThreadResume {
            request_id,
            params: ThreadResumeParams {
                thread_id: thread_id.clone(),
                exclude_turns: true,
                config: Some(registration(&new_url, "Bearer new")),
                ..Default::default()
            },
        })
        .await?;
    assert_eq!(resumed.approval_policy, started.approval_policy);
    assert_eq!(resumed.sandbox, started.sandbox);
    // Closing the old listener turns a retained endpoint into an observable failure.
    old_task.abort();
    assert_eq!(
        probe(&mut app, &thread_id).await?.content[0]["text"],
        "Bearer new"
    );
    let _: McpServerRefreshResponse = app
        .request(|request_id| ClientRequest::McpServerRefresh {
            request_id,
            params: None,
        })
        .await?;
    assert_eq!(
        probe(&mut app, &thread_id).await?.content[0]["text"],
        "Bearer new"
    );
    new_task.abort();
    if let Some((turn_id, _pending_request)) = pending_turn {
        assert_eq!(
            model.received_requests().await.unwrap().len(),
            1,
            "resuming must preserve the pending turn rather than start another model request"
        );
        let _: TurnInterruptResponse = app
            .request(|request_id| ClientRequest::TurnInterrupt {
                request_id,
                params: TurnInterruptParams { thread_id, turn_id },
            })
            .await?;
    }
    Ok(())
}
