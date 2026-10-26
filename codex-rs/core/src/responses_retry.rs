//! Shared retry and transport fallback decisions for Responses requests.
//! Content-filter guidance is recorded for sampling requests before retry decisions.
//! Server advice controls timing without extending configured retry limits.
//! Headerless capacity errors use a one-minute deadline in this shared retry loop.

use std::time::Duration;

use crate::client::ModelClientSession;
use crate::context::ContentFilterGuidance;
use crate::context::ContextualUserFragment;
use crate::session::session::Session;
use crate::session::step_context::StepContext;
use crate::session::turn_context::TurnContext;
use codex_client::RetryOperation;
use codex_features::Feature;
use codex_http_client::RetryAfter;
use codex_protocol::error::CodexErr;
use codex_protocol::error::CodexErrorDetails;
use codex_protocol::protocol::EventMsg;
use codex_protocol::protocol::WarningEvent;
use tokio::time::Instant;
use tracing::warn;

const INITIAL_CONNECTION_RETRY_DELAY: Duration = Duration::from_secs(5);
const MAX_LOCAL_RETRY_DELAY: Duration = Duration::from_secs(60);
const DEFAULT_OVERLOAD_RETRY_DELAY: Duration = Duration::from_secs(60);

#[derive(Debug, Clone, Copy)]
pub(crate) enum ResponsesStreamRequest {
    Sampling,
    RemoteCompactionV2,
}

pub(crate) struct ResponsesStreamRetryState {
    retries: u64,
    connection_retries: u64,
    connection_retry_delay: Duration,
}

impl Default for ResponsesStreamRetryState {
    fn default() -> Self {
        Self {
            retries: 0,
            connection_retries: 0,
            connection_retry_delay: INITIAL_CONNECTION_RETRY_DELAY,
        }
    }
}

/// Server retry advice retained after stream retries are exhausted. The turn ID
/// prevents a reused Guardian session from applying advice from an earlier review.
pub(crate) struct ExhaustedResponseRetry {
    pub(crate) turn_id: String,
    pub(crate) retry_at: Option<tokio::time::Instant>,
}

/// Returns `Ok(())` when the caller should retry the request loop, or the original error when
/// it is terminal or the retry budget is exhausted.
#[tracing::instrument(level = "trace", skip_all, fields(request = ?request))]
pub(crate) async fn handle_response_stream_error(
    retry_state: &mut ResponsesStreamRetryState,
    max_retries: u64,
    err: CodexErr,
    client_session: &mut ModelClientSession,
    sess: &Session,
    step_context: &StepContext,
    request: ResponsesStreamRequest,
) -> Result<(), CodexErr> {
    // 将本地默认等待接入现有期限、提示和预算逻辑，不覆盖服务端的有效建议。
    let err = if matches!(err.details(), CodexErrorDetails::ServerOverloaded)
        && err.retry_after().is_none()
    {
        err.with_retry_after(
            RetryAfter::from_delay(DEFAULT_OVERLOAD_RETRY_DELAY)
                .expect("one-minute retry deadline"),
        )
    } else {
        err
    };
    let turn_context = &step_context.turn;
    if matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ContentFilter)
    {
        let model_info = &step_context.settings.model_info;
        let guidance = ContentFilterGuidance {
            text: codex_prompts::ResolvedModelMessages::from_model(model_info)
                .content_filter_guidance()
                .to_string(),
        };
        sess.record_conversation_items(
            turn_context,
            model_info,
            &[ContextualUserFragment::into(guidance)],
        )
        .await;
    }
    let operation = match request {
        ResponsesStreamRequest::Sampling => RetryOperation::Sampling,
        ResponsesStreamRequest::RemoteCompactionV2 => RetryOperation::RemoteCompactionV2,
    };
    let retry_count = retry_state.retries.saturating_add(1);
    let Some(delay) = err.retry_delay(retry_count) else {
        return Err(err);
    };
    let retry_after = err.retry_after();
    // 只限制本地退避；服务端的有效等待期限仍完整保留。
    let delay = if retry_after.is_none() {
        delay.min(MAX_LOCAL_RETRY_DELAY)
    } else {
        delay
    };

    if turn_context
        .config
        .features
        .enabled(Feature::UnboundedConnectionRetries)
        && matches!(request, ResponsesStreamRequest::Sampling)
        && matches!(err.details(), CodexErrorDetails::ConnectionFailed(_))
        && !turn_context.session_source.is_internal()
        && !turn_context.provider.info().is_amazon_bedrock()
    {
        let retry_delay = retry_state.connection_retry_delay;
        warn!(
            turn_id = %turn_context.sub_id,
            error = %err,
            ?retry_delay,
            "stream connection failed; waiting to retry"
        );
        sess.notify_stream_error(
            turn_context,
            format!("Reconnecting... (retrying in {}s)", retry_delay.as_secs(),),
            err,
        )
        .await;
        retry_state.connection_retries = retry_state.connection_retries.saturating_add(1);
        codex_client::record_retry!(retry_state.connection_retries, retry_delay, operation);
        tokio::time::sleep(retry_delay).await;
        retry_state.connection_retry_delay =
            retry_delay.saturating_mul(2).min(MAX_LOCAL_RETRY_DELAY);
        return Ok(());
    }

    if retry_state.retries >= max_retries
        && client_session.try_switch_fallback_transport(
            &turn_context.session_telemetry,
            turn_context.model_info(),
        )
    {
        let wait = retry_after.map(|advice| {
            format!(
                " (retrying in {}s)",
                advice
                    .deadline()
                    .saturating_duration_since(Instant::now())
                    .as_secs_f64()
                    .ceil() as u64
            )
        });
        sess.send_event(
            turn_context,
            EventMsg::Warning(WarningEvent {
                message: format!(
                    "Falling back from WebSockets to HTTPS transport{}. {err:#}",
                    wait.unwrap_or_default()
                ),
            }),
        )
        .await;
        // Changing transport must not bypass the server's retry deadline.
        if let Some(retry_after) = retry_after {
            sess.notify_stream_error(
                turn_context,
                format!(
                    "Reconnecting... (retrying in {}s)",
                    retry_after.remaining_delay().as_secs_f64().ceil() as u64,
                ),
                err,
            )
            .await;
            tokio::time::sleep_until(retry_after.deadline()).await;
        }
        retry_state.retries = 0;
        return Ok(());
    }

    if retry_state.retries < max_retries {
        retry_state.retries = retry_count;
        log_retry(request, turn_context, &err, retry_count, max_retries, delay);

        let wait =
            retry_after.map(|advice| advice.deadline().saturating_duration_since(Instant::now()));
        // 每次重试都先报告原因、次数和等待时间，包括发布版的首次 WebSocket 重连。
        sess.notify_stream_error(
            turn_context,
            format!(
                "Reconnecting... {retry_count}/{max_retries} (retrying in {}s)",
                wait.unwrap_or(delay).as_secs_f64().ceil() as u64,
            ),
            err,
        )
        .await;
        // 提示耗时计入服务端期限；本地退避仍从提示送达后开始。
        let now = Instant::now();
        let retry_at = retry_after.map(RetryAfter::deadline).unwrap_or(now + delay);
        let delay = retry_at.saturating_duration_since(now);
        codex_client::record_retry!(retry_count, delay, operation);
        tokio::time::sleep_until(retry_at).await;
        return Ok(());
    }

    sess.services
        .thread_extension_data
        .insert(ExhaustedResponseRetry {
            turn_id: turn_context.sub_id.clone(),
            retry_at: retry_after.map(RetryAfter::deadline),
        });
    Err(err)
}

fn log_retry(
    request: ResponsesStreamRequest,
    turn_context: &TurnContext,
    err: &CodexErr,
    retries: u64,
    max_retries: u64,
    delay: Duration,
) {
    match request {
        ResponsesStreamRequest::Sampling => {
            warn!(
                turn_id = %turn_context.sub_id,
                retries,
                max_retries,
                sampling_error = %err,
                "stream disconnected - retrying sampling request ({retries}/{max_retries} in {delay:?})...",
            );
        }
        ResponsesStreamRequest::RemoteCompactionV2 => {
            warn!(
                turn_id = %turn_context.sub_id,
                retries,
                max_retries,
                compact_error = %err,
                "remote compaction v2 stream failed; retrying request after delay"
            );
        }
    }
}

#[cfg(test)]
#[path = "responses_retry_tests.rs"]
mod tests;
