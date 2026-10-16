use crate::error::ApiError;
use codex_client::Request;
use codex_client::RequestTelemetry;
use codex_client::Response;
use codex_client::RetryPolicy;
use codex_client::StreamResponse;
use codex_client::TransportError;
use codex_client::run_with_retry;
use http::StatusCode;
use std::future::Future;
use std::sync::Arc;
use std::time::Duration;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Error;
use tokio_tungstenite::tungstenite::Message;

/// Generic telemetry.
pub trait SseTelemetry: Send + Sync {
    fn on_sse_poll(
        &self,
        result: &Result<
            Option<
                Result<
                    eventsource_stream::Event,
                    eventsource_stream::EventStreamError<TransportError>,
                >,
            >,
            tokio::time::error::Elapsed,
        >,
        duration: Duration,
    );
}

/// Telemetry for Responses WebSocket transport.
pub trait WebsocketTelemetry: Send + Sync {
    fn on_ws_request(&self, duration: Duration, error: Option<&ApiError>, connection_reused: bool);

    fn on_ws_event(
        &self,
        result: &Result<Option<Result<Message, Error>>, ApiError>,
        duration: Duration,
    );
}

pub(crate) trait WithStatus {
    fn status(&self) -> StatusCode;
}

fn http_status(err: &TransportError) -> Option<StatusCode> {
    match err {
        TransportError::Http { status, .. } => Some(*status),
        _ => None,
    }
}

impl WithStatus for Response {
    fn status(&self) -> StatusCode {
        self.status
    }
}

impl WithStatus for StreamResponse {
    fn status(&self) -> StatusCode {
        self.status
    }
}

pub(crate) async fn run_with_request_telemetry<T, F, Fut>(
    policy: RetryPolicy,
    delegate_capacity_retries: bool,
    telemetry: Option<Arc<dyn RequestTelemetry>>,
    make_request: impl FnMut() -> Request,
    send: F,
) -> Result<T, TransportError>
where
    T: WithStatus,
    F: Clone + Fn(Request) -> Fut,
    Fut: Future<Output = Result<T, TransportError>>,
{
    // Record the actual HTTP result before wrapping capacity failures as a completed
    // retry operation. The core owns their unbounded backoff and five-minute cap;
    // retrying them here would add a second budget and could wait past that cap.
    run_with_retry(policy, make_request, move |req, attempt| {
        let telemetry = telemetry.clone();
        let send = send.clone();
        async move {
            let start = Instant::now();
            let result = send(req).await;
            if let Some(t) = telemetry.as_ref() {
                let (status, err) = match &result {
                    Ok(resp) => (Some(resp.status()), None),
                    Err(err) => (http_status(err), Some(err)),
                };
                t.on_request(attempt, status, err, start.elapsed());
            }
            match result {
                Err(err) if delegate_capacity_retries && is_capacity_error(&err) => Ok(Err(err)),
                result => result.map(Ok),
            }
        }
    })
    .await?
}

fn is_capacity_error(err: &TransportError) -> bool {
    matches!(
        err,
        TransportError::Http { status, body: Some(body), .. }
            if *status == StatusCode::SERVICE_UNAVAILABLE
                && serde_json::from_str::<serde_json::Value>(body)
                    .is_ok_and(|value| value["error"]["code"].as_str() == Some("server_is_overloaded"))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use codex_client::RetryOn;
    use codex_http_client::RetryAfter;
    use std::sync::atomic::AtomicU64;
    use std::sync::atomic::Ordering;

    struct TestResponse;

    impl WithStatus for TestResponse {
        fn status(&self) -> StatusCode {
            StatusCode::OK
        }
    }

    #[tokio::test(start_paused = true)]
    async fn capacity_errors_bypass_http_retries_and_preserve_advice() {
        let attempts = Arc::new(AtomicU64::new(0));
        let started = Instant::now();
        let advice = RetryAfter::from_delay(Duration::from_secs(600)).expect("retry deadline");
        let result = run_with_request_telemetry(
            RetryPolicy {
                max_attempts: 3,
                base_delay: Duration::from_millis(200),
                retry_on: RetryOn {
                    retry_429: true,
                    retry_5xx: true,
                    retry_transport: true,
                },
            },
            true,
            None,
            || Request::new(http::Method::POST, "http://localhost/responses".into()),
            {
                let attempts = Arc::clone(&attempts);
                move |_| {
                    attempts.fetch_add(1, Ordering::Relaxed);
                    std::future::ready(Err::<TestResponse, _>(TransportError::Http {
                        status: StatusCode::SERVICE_UNAVAILABLE,
                        url: None,
                        headers: None,
                        body: Some(r#"{"error":{"code":"server_is_overloaded"}}"#.into()),
                        retry_after: Some(advice),
                    }))
                }
            },
        )
        .await;
        let err = result
            .err()
            .expect("capacity failure should reach the core");
        assert!(is_capacity_error(&err));
        assert_eq!(err.retry_after(), Some(advice));
        assert_eq!(attempts.load(Ordering::Relaxed), 1);
        assert_eq!(started.elapsed(), Duration::ZERO);
    }

    #[test]
    fn only_explicit_http_503_capacity_errors_bypass_retries() {
        for (status, body, expected) in [
            (503, r#"{"error":{"code":"server_is_overloaded"}}"#, true),
            (500, r#"{"error":{"code":"server_is_overloaded"}}"#, false),
            (503, r#"{"error":{"code":"service_unavailable"}}"#, false),
            (503, "server_is_overloaded", false),
        ] {
            assert_eq!(
                is_capacity_error(&TransportError::Http {
                    status: StatusCode::from_u16(status).expect("valid status"),
                    url: None,
                    headers: None,
                    body: Some(body.into()),
                    retry_after: None,
                }),
                expected,
            );
        }
    }
}
