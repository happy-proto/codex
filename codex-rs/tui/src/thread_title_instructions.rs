use crate::legacy_core::config::Config;

pub(crate) const FIRST_TURN_TITLE_INSTRUCTIONS: &str = "\
During the first turn of this new conversation, use the codex_tui task-management tool set_thread_title to name the current task as soon as you understand what the user needs. Discover the tool if necessary. Omit threadId and set onlyIfUnnamed to true so an existing title, including a manual title, takes precedence.
If the request depends on a file, message, URL, or other external reference, read just enough to identify the actual task, then set its title before continuing lengthy investigation or implementation. Do not wait until the turn or the entire task is complete.
Use a concise, specific, single-line title in the user's language, at most 36 characters and under five words where possible. Preserve ticket references. Do not use quotes, Markdown, or trailing punctuation.
Do this once during the first turn. Do not automatically rename the conversation on later turns. If the tool is unavailable, continue the user's task without it.";

pub(crate) fn with_first_turn_title_instructions(
    config: &Config,
    control_instructions: Option<String>,
) -> Option<String> {
    if config.ephemeral {
        return control_instructions;
    }
    let existing = control_instructions.or_else(|| config.developer_instructions.clone());
    Some(match existing.as_deref() {
        Some(existing) if !existing.trim().is_empty() => {
            format!("{existing}\n\n{FIRST_TURN_TITLE_INSTRUCTIONS}")
        }
        _ => FIRST_TURN_TITLE_INSTRUCTIONS.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app_server_session::ThreadParamsMode;
    use crate::app_server_session::thread_start_params_from_config;
    use crate::legacy_core::config::ConfigBuilder;

    #[tokio::test]
    async fn first_turn_thread_title_instructions_preserve_existing_instructions() {
        let home = tempfile::tempdir().unwrap();
        let mut config = ConfigBuilder::default()
            .codex_home(home.path().to_path_buf())
            .build()
            .await
            .unwrap();
        config.developer_instructions = Some("Keep project-specific guidance.".to_string());
        for mode in [ThreadParamsMode::Embedded, ThreadParamsMode::Remote] {
            let params = thread_start_params_from_config(&config, mode, None, None);
            assert_eq!(
                params.developer_instructions.as_deref(),
                Some(
                    format!("Keep project-specific guidance.\n\n{FIRST_TURN_TITLE_INSTRUCTIONS}")
                        .as_str()
                )
            );
        }
        let params = thread_start_params_from_config(&config, ThreadParamsMode::Cloud, None, None);
        assert!(params.developer_instructions.is_none());
        config.ephemeral = true;
        let params =
            thread_start_params_from_config(&config, ThreadParamsMode::Embedded, None, None);
        assert!(params.developer_instructions.is_none());
    }
}
