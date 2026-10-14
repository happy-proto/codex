//! The metric is available when completion is observed, and incomplete usage hides a turn.

use codex_features::Feature;
use codex_protocol::protocol::EventMsg;
use core_test_support::responses;
use core_test_support::skip_if_no_network;
use core_test_support::test_codex::test_codex;
use core_test_support::wait_for_event;
use serde_json::json;
use wiremock::ResponseTemplate;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_speed_is_saved_before_completion_and_missing_usage_hides_next_turn()
-> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("first"),
                responses::ev_assistant_message("message-1", "done"),
                json!({"type": "response.completed", "response": {"id": "first", "usage": {
                    "input_tokens": 100, "output_tokens": 200,
                    "output_tokens_details": {"reasoning_tokens": 150}, "total_tokens": 300
                }}}),
            ]),
            responses::sse(vec![
                responses::ev_response_created("missing"),
                responses::ev_assistant_message("message-2", "done"),
                json!({"type": "response.completed", "response": {"id": "missing"}}),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config
                .features
                .enable(Feature::Sqlite)
                .expect("enable sqlite");
        })
        .build_with_auto_env(&server)
        .await?;
    let db = test.codex.state_db().expect("state db enabled");
    let thread_id = test.session_configured.thread_id;
    test.submit_turn("first").await?;
    let speeds = db.generation_speeds(thread_id).await?;
    assert_eq!(speeds.len(), 1);
    assert!(
        speeds.values().all(
            |record| record.tokens_per_second().is_finite() && record.tokens_per_second() > 0.0
        )
    );
    test.submit_turn("missing usage").await?;
    assert_eq!(db.generation_speeds(thread_id).await?, speeds);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_speed_is_hidden_after_an_internal_http_retry_succeeds() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    let request_mock = responses::mount_response_sequence(
        &server,
        vec![
            ResponseTemplate::new(500)
                .set_body_json(json!({"error": {"code": "internal_server_error"}})),
            responses::sse_response(responses::sse(vec![
                responses::ev_response_created("retried"),
                responses::ev_assistant_message("message", "done"),
                json!({"type": "response.completed", "response": {"id": "retried", "usage": {
                    "input_tokens": 100, "output_tokens": 200, "total_tokens": 300
                }}}),
            ])),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config
                .features
                .enable(Feature::Sqlite)
                .expect("enable sqlite");
            config.model_provider.supports_websockets = false;
            config.model_provider.request_max_retries = Some(1);
            config.model_provider.stream_max_retries = Some(0);
        })
        .build_with_auto_env(&server)
        .await?;
    let db = test.codex.state_db().expect("state db enabled");
    test.submit_turn("retry this request").await?;
    assert_eq!(request_mock.requests().len(), 2);
    assert!(
        db.generation_speeds(test.session_configured.thread_id)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_speed_is_hidden_after_an_interrupted_response_with_usage() -> anyhow::Result<()>
{
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    responses::mount_sse_sequence(
        &server,
        vec![
            responses::sse(vec![
                responses::ev_response_created("interrupted"),
                responses::ev_assistant_message("partial", "partial"),
                json!({"type": "response.incomplete", "response": {"id": "interrupted",
                    "incomplete_details": {"reason": "interrupted"},
                    "usage": {"input_tokens": 10, "output_tokens": 100, "total_tokens": 110}
                }}),
            ]),
            responses::sse(vec![
                responses::ev_response_created("follow-up"),
                responses::ev_assistant_message("final", "done"),
                json!({"type": "response.completed", "response": {"id": "follow-up",
                    "usage": {"input_tokens": 10, "output_tokens": 100, "total_tokens": 110}
                }}),
            ]),
        ],
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config
                .features
                .enable(Feature::Sqlite)
                .expect("enable sqlite");
        })
        .build_with_auto_env(&server)
        .await?;
    test.submit_turn("continue interrupted output").await?;
    let db = test.codex.state_db().expect("state db enabled");
    assert!(
        db.generation_speeds(test.session_configured.thread_id)
            .await?
            .is_empty()
    );
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn generation_speed_recovery_clears_pre_crash_statistics() -> anyhow::Result<()> {
    skip_if_no_network!(Ok(()));
    let server = responses::start_mock_server().await;
    responses::mount_sse_once(
        &server,
        responses::sse(vec![
            responses::ev_response_created("recovered"),
            responses::ev_assistant_message("message", "done"),
            json!({"type": "response.completed", "response": {"id": "recovered",
                "usage": {"input_tokens": 10, "output_tokens": 100, "total_tokens": 110}
            }}),
        ]),
    )
    .await;
    let test = test_codex()
        .with_config(|config| {
            config
                .features
                .enable(Feature::Sqlite)
                .expect("enable sqlite");
        })
        .build_with_auto_env(&server)
        .await?;
    let db = test.codex.state_db().expect("state db enabled");
    let thread_id = test.session_configured.thread_id;
    db.save_generation_speed(
        thread_id,
        "recovered-turn",
        20,
        1_000_000,
        codex_state::GenerationSpeedCompletion {
            started_at: 10,
            completed_at: 11,
            duration_ms: 1000,
        },
    )
    .await?;
    test.codex
        .recover_turn_if_idle(codex_core::RecoverTurnRequest {
            turn_id: "recovered-turn".into(),
            thread_settings: Default::default(),
            trace: None,
            cyber_access_program: None,
        })
        .await?;
    wait_for_event(&test.codex, |event| {
        matches!(event, EventMsg::TurnComplete(completed)
        if completed.turn_id == "recovered-turn" && completed.error.is_none())
    })
    .await;
    assert!(db.generation_speeds(thread_id).await?.is_empty());
    Ok(())
}
