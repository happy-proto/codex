use std::io::Write;

use codex_config::types::McpServerConfig;
use codex_config::types::McpServerTransportConfig;
use codex_protocol::protocol::McpAuthStatus;
use codex_utils_cli::format_env_display;
use owo_colors::OwoColorize;

pub(super) struct McpListEntry<'a> {
    pub(super) name: &'a str,
    pub(super) config: &'a McpServerConfig,
    pub(super) auth_status: McpAuthStatus,
}

pub(super) fn write_human_list(
    mut output: impl Write,
    entries: &[McpListEntry<'_>],
    width: usize,
    color: bool,
) -> std::io::Result<()> {
    let title = format!("MCP servers · {} configured", entries.len());
    write_wrapped(&mut output, &bold(&title, color), "", "", width)?;
    for entry in entries {
        writeln!(output)?;
        let transport = match &entry.config.transport {
            McpServerTransportConfig::Stdio { .. } => "stdio",
            McpServerTransportConfig::StreamableHttp { .. } => "HTTP",
        };
        let status = if entry.config.enabled {
            if color {
                "enabled".green().to_string()
            } else {
                "enabled".to_string()
            }
        } else if color {
            "disabled".dimmed().to_string()
        } else {
            "disabled".to_string()
        };
        let name = bold(&safe_text(entry.name), color);
        let heading = format!("{name}  {status} · {transport}");
        write_wrapped(&mut output, &heading, "", "  ", width)?;

        match &entry.config.transport {
            McpServerTransportConfig::Stdio {
                command,
                args,
                env,
                env_vars,
                cwd,
            } => {
                field(&mut output, "Command", command, width, color)?;
                if !args.is_empty() {
                    // Keep arguments separate from the executable: this is configuration
                    // display, not a reconstructed shell command.
                    let args = args
                        .iter()
                        .map(|arg| {
                            if arg.is_empty() || arg.chars().any(char::is_whitespace) {
                                format!("{arg:?}")
                            } else {
                                safe_text(arg)
                            }
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    field(&mut output, "Args", &args, width, color)?;
                }
                let env = format_env_display(env.as_ref(), env_vars);
                if env != "-" {
                    field(&mut output, "Env", &env, width, color)?;
                }
                if let Some(cwd) = cwd.as_ref().filter(|cwd| !cwd.as_str().is_empty()) {
                    field(&mut output, "Cwd", cwd.as_str(), width, color)?;
                }
            }
            McpServerTransportConfig::StreamableHttp {
                url,
                bearer_token_env_var,
                ..
            } => {
                field(&mut output, "URL", url, width, color)?;
                if let Some(name) = bearer_token_env_var {
                    field(&mut output, "Token env", name, width, color)?;
                }
            }
        }

        let auth = entry.auth_status.to_string();
        let auth = if color
            && matches!(
                entry.auth_status,
                McpAuthStatus::NotLoggedIn | McpAuthStatus::Unknown
            ) {
            auth.yellow().to_string()
        } else {
            auth
        };
        // Styling is applied only to trusted status text, never to config values.
        styled_field(&mut output, "Auth", &auth, width, color)?;
        if !entry.config.enabled
            && let Some(reason) = &entry.config.disabled_reason
        {
            field(&mut output, "Reason", &reason.to_string(), width, color)?;
        }
    }
    Ok(())
}

fn field(
    output: &mut impl Write,
    label: &str,
    value: &str,
    width: usize,
    color: bool,
) -> std::io::Result<()> {
    styled_field(output, label, &safe_text(value), width, color)
}

fn styled_field(
    output: &mut impl Write,
    label: &str,
    value: &str,
    width: usize,
    color: bool,
) -> std::io::Result<()> {
    let label = format!("{label:<9}");
    let label = if color {
        label.dimmed().to_string()
    } else {
        label
    };
    write_wrapped(
        output,
        value,
        &format!("  {label}  "),
        "             ",
        width,
    )
}

fn write_wrapped(
    output: &mut impl Write,
    value: &str,
    initial_indent: &str,
    subsequent_indent: &str,
    width: usize,
) -> std::io::Result<()> {
    let options = textwrap::Options::new(width.max(20))
        .initial_indent(initial_indent)
        .subsequent_indent(subsequent_indent)
        .word_splitter(textwrap::WordSplitter::NoHyphenation);
    writeln!(output, "{}", textwrap::fill(value, options))
}

fn bold(text: &str, color: bool) -> String {
    if color {
        text.bold().to_string()
    } else {
        text.to_string()
    }
}

fn safe_text(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars() {
        if ch.is_control() {
            output.extend(ch.escape_default());
        } else {
            output.push(ch);
        }
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;

    fn render(
        config: &McpServerConfig,
        auth_status: McpAuthStatus,
        width: usize,
        color: bool,
    ) -> String {
        let mut output = Vec::new();
        write_human_list(
            &mut output,
            &[McpListEntry {
                name: "docs",
                config,
                auth_status,
            }],
            width,
            color,
        )
        .unwrap();
        String::from_utf8(output).unwrap()
    }

    #[test]
    fn stdio_details_are_readable_and_environment_values_stay_redacted() {
        let config = toml::from_str(
            r#"command = "docs-server"
args = ["--port", "4000", "two words", ""]
env = { TOKEN = "secret" }
env_vars = ["APP_TOKEN"]
"#,
        )
        .unwrap();
        let output = render(&config, McpAuthStatus::Unsupported, 80, false);
        assert!(output.contains("docs  enabled · stdio"));
        assert!(output.contains("Command    docs-server"));
        assert!(output.contains("--port 4000 \"two words\" \"\""));
        assert!(output.contains("TOKEN=*****"));
        assert!(output.contains("APP_TOKEN=*****"));
        assert!(!output.contains("secret"));
        assert!(!output.contains("Cwd"));
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn http_and_disabled_servers_use_the_same_layout() {
        let mut config: McpServerConfig = toml::from_str(
            r#"url = "https://example.com/mcp"
bearer_token_env_var = "MCP_TOKEN"
enabled = false
"#,
        )
        .unwrap();
        config.disabled_reason = Some(codex_config::types::McpServerDisabledReason::Unknown);
        let output = render(&config, McpAuthStatus::BearerToken, 80, false);
        assert!(output.contains("docs  disabled · HTTP"));
        assert!(output.contains("URL        https://example.com/mcp"));
        assert!(output.contains("Token env  MCP_TOKEN"));
        assert!(output.contains("Bearer token"));
        assert!(output.contains("Reason     unknown"));
        assert!(!output.contains("Command"));
    }

    #[test]
    fn mixed_transports_preserve_order_and_wrap_unicode_details() {
        let stdio =
            toml::from_str("command = \"文档服务器\"\ncwd = \"/项目/文档/工作目录\"").unwrap();
        let http = toml::from_str("url = \"https://example.com/mcp\"").unwrap();
        let mut output = Vec::new();
        write_human_list(
            &mut output,
            &[
                McpListEntry {
                    name: "docs",
                    config: &stdio,
                    auth_status: McpAuthStatus::Unsupported,
                },
                McpListEntry {
                    name: "github",
                    config: &http,
                    auth_status: McpAuthStatus::OAuth,
                },
            ],
            32,
            false,
        )
        .unwrap();
        let output = String::from_utf8(output).unwrap();
        assert!(output.contains("2 configured"));
        assert!(output.find("docs").unwrap() < output.find("github").unwrap());
        assert!(output.contains("文档服务器"));
        assert!(
            output
                .lines()
                .all(|line| textwrap::core::display_width(line) <= 32)
        );
        assert!(
            output
                .split_whitespace()
                .collect::<String>()
                .contains("/项目/文档/工作目录")
        );
    }

    #[test]
    fn narrow_output_wraps_long_urls_without_truncating_them() {
        let url = "https://example.com/a/very/long/path/to/mcp";
        let config = toml::from_str(&format!("url = {url:?}")).unwrap();
        let output = render(&config, McpAuthStatus::Unknown, 32, false);
        assert!(output.lines().all(|line| line.chars().count() <= 32));
        let flattened = output.split_whitespace().collect::<String>();
        assert!(flattened.contains(url));
    }

    #[test]
    fn colors_highlight_statuses_but_do_not_mark_unsupported_auth_as_failure() {
        let config = toml::from_str("command = \"docs-server\"").unwrap();
        let unsupported = render(&config, McpAuthStatus::Unsupported, 80, true);
        assert!(unsupported.contains('\u{1b}'));
        assert!(unsupported.contains("Unsupported"));
        assert!(!unsupported.contains("\u{1b}[33mUnsupported"));
        let unknown = render(&config, McpAuthStatus::Unknown, 80, true);
        assert!(unknown.contains("\u{1b}[33mUnknown"));
    }

    #[test]
    fn config_control_characters_cannot_inject_terminal_styles() {
        let config = toml::from_str("command = \"docs\\u001b[31m\\nserver\"").unwrap();
        let output = render(&config, McpAuthStatus::Unsupported, 80, false);
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("\\u{1b}[31m\\nserver"));
    }

    #[test]
    fn write_errors_are_propagated() {
        assert!(write_human_list(&mut [][..], &[], 80, false).is_err());
    }
}
