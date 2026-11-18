//! Reattach the TUI-owned loopback transport without replaying general resume overrides.

use super::*;
use codex_app_server_protocol::TUI_MCP_OWNER_CONNECTED_REASON;
use codex_config::McpServerConfig;
use codex_config::McpServerTransportConfig;

fn is_tui_transport(transport: &McpServerTransportConfig) -> bool {
    let McpServerTransportConfig::StreamableHttp {
        url,
        bearer_token_env_var: None,
        http_headers: Some(headers),
        env_http_headers: None,
        http_headers_helper: None,
    } = transport
    else {
        return false;
    };
    let Ok(url) = url::Url::parse(url) else {
        return false;
    };
    url.scheme() == "http"
        && url.host_str() == Some("127.0.0.1")
        && url.port().is_some()
        && url.path() == "/mcp"
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && headers.len() == 1
        && headers.get("Authorization").is_some_and(|header| {
            header
                .strip_prefix("Bearer ")
                .is_some_and(|token| !token.is_empty())
        })
}

impl ThreadRequestProcessor {
    pub(super) async fn rebind_tui_mcp_for_resume(
        &self,
        request_id: &ConnectionRequestId,
        thread_id: ThreadId,
        thread: &Arc<CodexThread>,
        params: &ThreadResumeParams,
    ) -> Result<ThreadResumeParams, JSONRPCErrorError> {
        let mut remaining = params.clone();
        if !can_accept_direct_input(thread.multi_agent_version(), thread.session_source()) {
            return Ok(remaining);
        }
        let Some(value) = params
            .config
            .as_ref()
            .and_then(|config| config.get("mcp_servers.codex_tui"))
        else {
            return Ok(remaining);
        };
        let current = thread.config().await;
        let previous = current.mcp_servers.get().get("codex_tui");
        if previous.is_some_and(|server| !is_tui_transport(&server.transport)) {
            return Ok(remaining);
        }
        let replacement: McpServerConfig = serde_json::from_value(value.clone())
            .map_err(|_| invalid_request("invalid client task MCP registration".to_string()))?;
        if !is_tui_transport(&replacement.transport) {
            return Err(invalid_request(
                "client task MCP registration must use its authenticated loopback listener"
                    .to_string(),
            ));
        }
        if previous.is_none_or(|server| server.transport != replacement.transport) {
            if !self
                .thread_state_manager
                .claim_tui_mcp_owner(thread_id, request_id.connection_id)
                .await
            {
                let mut error = invalid_request(
                    "cannot rebind task MCP while its owning client is connected; disconnect that client and resume again",
                );
                error.data = Some(serde_json::json!({ "reason": TUI_MCP_OWNER_CONNECTED_REASON }));
                return Err(error);
            }
            thread
                .rebind_tui_mcp_transport(replacement.transport)
                .await
                .map_err(|_| {
                    invalid_request("could not apply client task MCP registration".to_string())
                })?;
        }
        // Only transport identity is applied. General settings keep normal loaded-thread semantics.
        if let Some(config) = remaining.config.as_mut() {
            config.remove("mcp_servers.codex_tui");
            if config.is_empty() {
                remaining.config = None;
            }
        }
        Ok(remaining)
    }
}
