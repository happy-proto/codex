//! Fork-only defaults; do not persist these to the shared Desktop configuration.

use codex_config::McpServerConfig;
use codex_config::McpServerTransportConfig;
use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

const SERVER_NAME: &str = "open_computer_use";
const COMMANDS: &[&str] = &["open-computer-use", "ocu"];

pub(super) fn default_computer_server(
    existing: &HashMap<String, McpServerConfig>,
    find: impl Fn(&str) -> Option<PathBuf>,
) -> Option<(String, McpServerConfig)> {
    // Existing disabled entries also suppress the fallback. Recognize aliases and
    // npm's npx launcher without replacing the user's transport or tool policies.
    if existing.iter().any(|(name, server)| {
        matches!(
            name.as_str(),
            "open_computer_use" | "open-computer-use" | "ocu"
        ) || match &server.transport {
            McpServerTransportConfig::Stdio { command, args, .. } => {
                let basename = Path::new(command)
                    .file_stem()
                    .and_then(|name| name.to_str());
                basename.is_some_and(|name| COMMANDS.contains(&name))
                    || (matches!(basename, Some("npx" | "bunx"))
                        && args.iter().any(|arg| {
                            arg == "open-computer-use" || arg.starts_with("open-computer-use@")
                        }))
            }
            McpServerTransportConfig::StreamableHttp { .. } => false,
        }
    }) {
        return None;
    }
    let command = COMMANDS.iter().find_map(|command| find(command))?;
    #[expect(
        clippy::expect_used,
        reason = "static built-in TOML is validated in tests"
    )]
    let mut server: McpServerConfig =
        toml::from_str("command = 'open-computer-use'\nargs = ['mcp']\n")
            .expect("valid built-in computer-use MCP configuration");
    if let McpServerTransportConfig::Stdio {
        command: target, ..
    } = &mut server.transport
    {
        *target = command.to_string_lossy().into_owned();
    }
    Some((SERVER_NAME.to_string(), server))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fork_automation_requires_an_installed_runtime() {
        assert!(default_computer_server(&HashMap::new(), |_| None).is_none());
        let (name, server) = default_computer_server(&HashMap::new(), |name| {
            (name == "ocu").then(|| PathBuf::from("/opt/bin/ocu"))
        })
        .unwrap();
        assert_eq!(name, SERVER_NAME);
        assert!(server.enabled);
        assert!(!server.required);
        assert!(
            matches!(server.transport, McpServerTransportConfig::Stdio { command, args, .. }
            if command == "/opt/bin/ocu" && args == ["mcp"])
        );
    }

    #[test]
    fn fork_automation_preserves_explicit_servers_and_disabled_aliases() {
        for config in [
            "command = '/custom/bin/ocu'\nargs = ['mcp']\nenabled = false",
            "command = 'npx'\nargs = ['-y', 'open-computer-use@0.1.0', 'mcp']",
        ] {
            let server = toml::from_str(config).unwrap();
            let existing = HashMap::from([("custom-name".to_string(), server)]);
            assert!(
                default_computer_server(&existing, |_| panic!("must preserve existing config"))
                    .is_none()
            );
        }
        let server = toml::from_str("url = 'http://localhost:1234/mcp'\nenabled = false").unwrap();
        let existing = HashMap::from([(SERVER_NAME.to_string(), server)]);
        assert!(
            default_computer_server(&existing, |_| panic!("must preserve explicit disable"))
                .is_none()
        );
    }
}
