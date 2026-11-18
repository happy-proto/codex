use codex_app_server_client::RemoteAppServerClient;
use codex_app_server_client::RemoteAppServerConnectArgs;
use codex_app_server_client::RemoteAppServerEndpoint;
use codex_app_server_protocol::JSONRPCRequest;
use codex_app_server_protocol::RequestId;
use codex_utils_home_dir::find_codex_home;
use color_eyre::eyre::Result;
use color_eyre::eyre::WrapErr;
use color_eyre::eyre::eyre;
use serde_json::Value;
use serde_json::json;
use std::time::Duration;
use tokio::time::Instant;

#[derive(Debug, clap::Subcommand)]
pub enum SessionTaskCommand {
    /// List recent sessions on the shared app-server.
    List {
        #[arg(long, default_value_t = 20)]
        limit: u32,
        #[arg(long)]
        archived: bool,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Read session status and one page of recent turns.
    Read {
        thread_id: String,
        #[arg(long, default_value_t = 5)]
        limit: u32,
        #[arg(long)]
        cursor: Option<String>,
    },
    /// Send a delegated prompt and return its accepted turn.
    Send {
        thread_id: String,
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        model: Option<String>,
    },
    /// Create a session using the calling session's settings and start a turn.
    Create {
        #[arg(long)]
        prompt: String,
        #[arg(long)]
        title: Option<String>,
        #[arg(long)]
        model: Option<String>,
    },
    /// Fork a session without starting a turn.
    Fork { thread_id: Option<String> },
    /// Set a session's title.
    Rename {
        title: String,
        #[arg(long)]
        thread_id: Option<String>,
        #[arg(long)]
        only_if_unnamed: bool,
    },
    /// Archive a session.
    Archive { thread_id: String },
    /// Restore an archived session.
    Unarchive { thread_id: String },
    /// Wait until one of the sessions becomes idle or needs attention.
    Wait {
        #[arg(required = true, num_args = 1..=8)]
        thread_ids: Vec<String>,
        #[arg(long, default_value_t = 30, value_parser = clap::value_parser!(u64).range(0..=300))]
        timeout_seconds: u64,
    },
}

pub async fn run_session_task_command(
    command: SessionTaskCommand,
    endpoint: Option<RemoteAppServerEndpoint>,
) -> Result<Value> {
    let implicit = endpoint.is_none();
    let endpoint = match endpoint {
        Some(endpoint) => endpoint,
        None => {
            let home = find_codex_home()?;
            let socket_path =
                codex_app_server_client::app_server_control_socket_path(home.as_path())?;
            RemoteAppServerEndpoint::UnixSocket { socket_path }
        }
    };
    let args = RemoteAppServerConnectArgs {
        endpoint,
        client_name: "codex-task".to_string(),
        client_version: env!("CARGO_PKG_VERSION").to_string(),
        required_human_approval: false,
        experimental_api: true,
        mcp_server_openai_form_elicitation: false,
        opt_out_notification_methods: Vec::new(),
        channel_capacity: crate::DEFAULT_IN_PROCESS_CHANNEL_CAPACITY,
    };
    #[cfg(windows)]
    let connection = if implicit {
        RemoteAppServerClient::connect_local_daemon(args).await
    } else {
        RemoteAppServerClient::connect(args).await
    };
    #[cfg(not(windows))]
    let connection = RemoteAppServerClient::connect(args).await;
    let app = connection.wrap_err_with(|| {
        if implicit {
            "could not reach the shared app-server; start the daemon or specify --remote"
        } else {
            "could not reach the selected app-server"
        }
    })?;
    let mut client = TaskClient {
        app: &app,
        request_id: 0,
    };
    let result = client.execute(command).await;
    // Closing this connection does not shut down the shared app-server.
    let _ = app.shutdown().await;
    result
}

struct TaskClient<'a> {
    app: &'a RemoteAppServerClient,
    request_id: i64,
}

impl TaskClient<'_> {
    async fn rpc(&mut self, method: &str, params: Value) -> Result<Value> {
        self.request_id += 1;
        self.app
            .request_handle()
            .request_json_rpc(JSONRPCRequest {
                id: RequestId::Integer(self.request_id),
                method: method.to_string(),
                params: Some(params),
                trace: None,
            })
            .await
            .wrap_err_with(|| format!("{method}: connection lost before receiving a reply"))?
            .map_err(|err| eyre!("{method}: {} ({})", err.message, err.code))
    }

    async fn read(&mut self, thread_id: &str) -> Result<Value> {
        self.rpc(
            "thread/read",
            json!({"threadId": thread_id, "includeTurns": false}),
        )
        .await
    }

    async fn resume(&mut self, thread_id: &str) -> Result<Value> {
        self.rpc(
            "thread/resume",
            json!({"threadId": thread_id, "excludeTurns": true}),
        )
        .await
    }

    async fn start_turn(
        &mut self,
        thread_id: &str,
        tool: &str,
        prompt: String,
        model: Option<String>,
        sandbox_policy: Option<&Value>,
    ) -> Result<Value> {
        let result = self
            .rpc(
                "turn/start",
                json!({
                    "threadId": thread_id,
                    "input": [],
                    "model": model,
                    "sandboxPolicy": sandbox_policy,
                    "toolOutput": {
                        "name": tool,
                        "namespace": crate::dynamic_tools::NAMESPACE,
                        "output": prompt,
                        "retain": true
                    }
                }),
            )
            .await?;
        let turn = &result["turn"];
        if turn["id"].as_str().is_none() {
            return Err(eyre!(
                "turn/start did not return a turn ID for thread {thread_id}"
            ));
        }
        Ok(json!({"threadId": thread_id, "turn": turn}))
    }

    async fn execute(&mut self, command: SessionTaskCommand) -> Result<Value> {
        use SessionTaskCommand::*;
        match command {
            List {
                limit,
                archived,
                cursor,
            } => {
                self.rpc(
                    "thread/list",
                    json!({
                        "limit": limit,
                        "archived": archived,
                        "cursor": cursor,
                        "sortKey": "updated_at",
                        "currentAccountOnly": true,
                        "useStateDbOnly": true,
                        "modelProviders": []
                    }),
                )
                .await
            }
            Read {
                thread_id,
                limit,
                cursor,
            } => {
                let mut result = self.read(&thread_id).await?;
                result["turnsPage"] = self
                    .rpc(
                        "thread/turns/list",
                        json!({
                            "threadId": thread_id,
                            "limit": limit,
                            "cursor": cursor,
                            "itemsView": "full"
                        }),
                    )
                    .await?;
                Ok(result)
            }
            Send {
                thread_id,
                prompt,
                model,
            } => {
                let source = current_thread()?;
                if source == thread_id {
                    return Err(eyre!("send requires a different target session"));
                }
                let prompt = prepare_prompt(&source, &prompt)?;
                self.read(&source).await?;
                self.resume(&thread_id).await?;
                self.start_turn(
                    &thread_id,
                    "send_message_to_thread",
                    prompt,
                    model,
                    /*sandbox_policy*/ None,
                )
                .await
            }
            Create {
                prompt,
                title,
                model,
            } => {
                let source_id = current_thread()?;
                let prompt = prepare_prompt(&source_id, &prompt)?;
                let source = self.resume(&source_id).await?;
                if source["thread"]["ephemeral"] == true {
                    return Err(eyre!(
                        "ephemeral sessions cannot create inspectable background sessions"
                    ));
                }
                let mut params = json!({
                    "cwd": source["cwd"],
                    "model": model.as_deref().or_else(|| source["model"].as_str()),
                    "modelProvider": source["modelProvider"],
                    "serviceTier": source["serviceTier"],
                    "projectId": source["thread"]["projectId"],
                    "runtimeWorkspaceRoots": source["runtimeWorkspaceRoots"],
                    "approvalPolicy": source["approvalPolicy"],
                    "approvalsReviewer": source["approvalsReviewer"]
                });
                let sandbox_policy = if let Some(profile) =
                    source["activePermissionProfile"]["id"].as_str()
                {
                    params["permissions"] = json!(profile);
                    None
                } else {
                    let sandbox = &source["sandbox"];
                    let mode = match sandbox["type"].as_str() {
                        Some("dangerFullAccess") => "danger-full-access",
                        Some("workspaceWrite") => "workspace-write",
                        Some("readOnly") => "read-only",
                        _ => {
                            return Err(eyre!(
                                "cannot inherit the source session's sandbox without a named permission profile"
                            ));
                        }
                    };
                    params["sandbox"] = json!(mode);
                    Some(sandbox)
                };
                let started = self.rpc("thread/start", params).await?;
                let id = started["thread"]["id"]
                    .as_str()
                    .ok_or_else(|| eyre!("thread/start did not return a thread ID"))?;
                let mut warnings = Vec::new();
                if let Some(title) = title
                    && let Err(err) = self
                        .rpc("thread/name/set", json!({"threadId": id, "name": title}))
                        .await
                {
                    warnings.push(err.to_string());
                }
                let mut result = self
                    .start_turn(
                        id,
                        "create_thread",
                        prompt,
                        /*model*/ None,
                        sandbox_policy,
                    )
                    .await
                    .wrap_err_with(|| {
                        format!("created thread {id}; could not confirm its first turn")
                    })?;
                if !warnings.is_empty() {
                    result["warnings"] = json!(warnings);
                }
                Ok(result)
            }
            Fork { thread_id } => {
                let id = thread_id.map_or_else(current_thread, Ok)?;
                let result = self
                    .rpc("thread/fork", json!({"threadId": id, "excludeTurns": true}))
                    .await?;
                Ok(json!({"threadId": result["thread"]["id"]}))
            }
            Rename {
                thread_id,
                title,
                only_if_unnamed,
            } => {
                let id = thread_id.map_or_else(current_thread, Ok)?;
                if title.trim().is_empty() {
                    return Err(eyre!("title must not be empty"));
                }
                if only_if_unnamed
                    && let Some(name) = self.read(&id).await?["thread"]["name"].as_str()
                {
                    return Ok(json!({"threadId": id, "title": name, "updated": false}));
                }
                self.rpc(
                    "thread/name/set",
                    json!({"threadId": id, "name": title.trim()}),
                )
                .await?;
                Ok(json!({"threadId": id, "title": title.trim(), "updated": true}))
            }
            Archive { thread_id } => {
                if std::env::var("CODEX_THREAD_ID").as_deref() == Ok(thread_id.as_str()) {
                    return Err(eyre!("cannot archive the calling session"));
                }
                self.rpc("thread/archive", json!({"threadId": thread_id}))
                    .await?;
                Ok(json!({"threadId": thread_id, "archived": true}))
            }
            Unarchive { thread_id } => {
                self.rpc("thread/unarchive", json!({"threadId": thread_id}))
                    .await?;
                Ok(json!({"threadId": thread_id, "archived": false}))
            }
            Wait {
                thread_ids,
                timeout_seconds,
            } => self.wait(thread_ids, timeout_seconds).await,
        }
    }

    async fn wait(&mut self, thread_ids: Vec<String>, timeout_seconds: u64) -> Result<Value> {
        let deadline = Instant::now() + Duration::from_secs(timeout_seconds);
        loop {
            let mut statuses = Vec::with_capacity(thread_ids.len());
            for id in &thread_ids {
                let read = self.read(id).await?;
                statuses.push(json!({"threadId": id, "status": read["thread"]["status"]}));
            }
            let ready = statuses.iter().any(|entry| {
                let status = &entry["status"];
                status["type"] != "active"
                    || status["activeFlags"]
                        .as_array()
                        .is_some_and(|flags| !flags.is_empty())
            });
            if ready || Instant::now() >= deadline {
                return Ok(json!({"timedOut": !ready, "threads": statuses}));
            }
            tokio::time::sleep_until((Instant::now() + Duration::from_secs(1)).min(deadline)).await;
        }
    }
}

fn current_thread() -> Result<String> {
    std::env::var("CODEX_THREAD_ID")
        .ok()
        .filter(|id| !id.is_empty())
        .ok_or_else(|| eyre!("this command requires a calling session (CODEX_THREAD_ID)"))
}

fn prepare_prompt(source: &str, prompt: &str) -> Result<String> {
    if prompt.trim().is_empty() || prompt.len() > 1_000 {
        return Err(eyre!("prompt must contain between 1 and 1,000 UTF-8 bytes"));
    }
    let escape = |text: &str| {
        text.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
    };
    Ok(format!(
        "<codex_delegation>\n  <source_thread_id>{}</source_thread_id>\n  <input>{}</input>\n</codex_delegation>",
        escape(source),
        escape(prompt)
    ))
}
