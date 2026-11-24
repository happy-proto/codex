//! Derives TUI capabilities from terminal identity and session environment.
//! Keep environment-specific clipboard decisions here so delivery code only sees a strategy.

use codex_terminal_detection::TerminalName;

/// How clipboard writes should reach the attached terminal.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ClipboardCopyMode {
    FallbackOsc52,
    Osc52,
    PreferTerminal { native_for_large_copies: bool },
}

impl ClipboardCopyMode {
    pub(crate) fn for_terminal(terminal: TerminalName) -> Self {
        if terminal == TerminalName::Rex {
            Self::Osc52
        } else {
            Self::FallbackOsc52
        }
    }
}

pub(crate) struct ClipboardCapabilities {
    pub(crate) copy_mode: ClipboardCopyMode,
}

impl ClipboardCapabilities {
    pub(crate) fn detect() -> Self {
        let terminal = codex_terminal_detection::terminal_info();
        Self {
            copy_mode: if is_tmux_session()
                || is_ssh_session()
                || terminal.name == TerminalName::Rex
                || std::env::var("HERDR_ENV").is_ok_and(|value| value == "1")
            {
                ClipboardCopyMode::Osc52
            } else if terminal.is_vscode_dev_container() {
                ClipboardCopyMode::PreferTerminal {
                    native_for_large_copies: cfg!(target_os = "linux")
                        && std::env::var_os("WAYLAND_DISPLAY")
                            .is_some_and(|display| !display.is_empty()),
                }
            } else {
                ClipboardCopyMode::for_terminal(terminal.name)
            },
        }
    }
}

pub(crate) fn is_ssh_session() -> bool {
    std::env::var_os("SSH_TTY").is_some() || std::env::var_os("SSH_CONNECTION").is_some()
}

pub(crate) fn is_tmux_session() -> bool {
    std::env::var_os("TMUX").is_some() || std::env::var_os("TMUX_PANE").is_some()
}
