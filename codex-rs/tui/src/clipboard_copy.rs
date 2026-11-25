//! Clipboard copy backend for the TUI's `/copy` command and `Ctrl+O` hotkey.
//!
//! In tmux, Herdr, Rex, and SSH, send OSC 52 directly to the terminal.
//! Elsewhere, try the native clipboard or the Windows clipboard on WSL first.
//! Terminal writes have no delivery acknowledgement.
//!
//! On Linux, X11 and some Wayland compositors require the process that wrote the
//! clipboard to keep its handle open. `ClipboardLease` wraps the `arboard::Clipboard`
//! so the copy worker can retain it for the lifetime of the TUI. On other platforms the lease
//! is always `None`.
//!
//! Empty selections fail without modifying a clipboard.
//! Markdown copies also offer HTML on the native clipboard. Terminal and WSL
//! fallbacks retain the original text. Terminal writes are unacknowledged requests,
//! so callers must distinguish them from confirmed native clipboard writes.
//! Image paste lives in `clipboard_paste`.

pub(crate) mod primary;
pub(crate) mod worker;
mod wsl;

use crate::terminal_capabilities::ClipboardCapabilities;
use crate::terminal_capabilities::ClipboardCopyMode;
pub(crate) use crate::terminal_capabilities::is_ssh_session;
use crate::terminal_capabilities::is_tmux_session;
use base64::Engine;
use std::io::Write;
use std::sync::Arc;

/// Maximum raw bytes encoded into OSC 52.
/// Large payloads are rejected before encoding to avoid overwhelming the terminal.
const OSC52_MAX_RAW_BYTES: usize = 100_000;
pub(crate) const UNCONFIRMED_COPY_MESSAGE: &str =
    "Try pasting. If the text isn’t there, use /export to save the chat.";

/// Whether copied text should also have a rendered HTML representation and its source.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum CopyFormat {
    PlainText,
    Markdown,
    /// Visible selection text is copied literally; only HTML uses the semantic Markdown.
    MarkdownSelection(Arc<str>),
}

/// Host delivery status and any native clipboard ownership to retain.
pub(crate) enum CopyOutcome {
    Copied(Option<ClipboardLease>),
    Unconfirmed(Option<ClipboardLease>),
}

impl CopyOutcome {
    /// Replace native ownership only when a backend supplies a new lease.
    pub(crate) fn store(self, lease: &mut Option<ClipboardLease>) -> CopyStatus {
        let (status, next_lease) = match self {
            Self::Copied(next) => (CopyStatus::Confirmed, next),
            Self::Unconfirmed(next) => (CopyStatus::Unconfirmed, next),
        };
        if let Some(next) = next_lease {
            *lease = Some(next);
        }
        status
    }
}

/// Submission progress or the delivery status reported by a clipboard backend.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CopyStatus {
    Confirmed,
    Unconfirmed,
    Pending(u64),
    Busy,
}

impl CopyStatus {
    pub(crate) fn message(self, label: &str) -> String {
        match self {
            Self::Confirmed => format!("Copied {label} to clipboard"),
            Self::Unconfirmed => UNCONFIRMED_COPY_MESSAGE.to_string(),
            Self::Pending(_) => format!("Copying {label}…"),
            Self::Busy => "Copy already in progress".to_string(),
        }
    }
}

/// Copy text to the system clipboard.
///
/// Use OSC 52 directly in terminal-forwarding sessions, and as a fallback
/// elsewhere. A terminal send does not confirm clipboard delivery.
fn copy_to_clipboard(
    text: &str,
    format: CopyFormat,
    begin_delivery: impl Fn() -> Result<(), String>,
    osc52: impl Fn(&str) -> Result<(), String>,
) -> Result<CopyOutcome, String> {
    let wsl_deadline = std::time::Instant::now() + std::time::Duration::from_secs(/*secs*/ 5);
    let capabilities = ClipboardCapabilities::detect();
    let environment = CopyEnvironment {
        terminal_copy: capabilities.copy_mode,
        wsl_session: is_wsl_session(),
    };
    #[cfg(not(target_os = "android"))]
    let native_copy = {
        // Bound WSL native setup: even initialization can block forever.
        let clipboard = (!environment.wsl_session && environment.uses_native(text))
            .then(arboard::Clipboard::new);
        move |text: &str, html: Option<&str>| {
            let clipboard = match clipboard {
                Some(clipboard) => clipboard.map_err(|e| format!("clipboard unavailable: {e}"))?,
                None => wsl::native_clipboard()?,
            };
            arboard_copy(clipboard, text, html)
        }
    };
    #[cfg(target_os = "android")]
    let native_copy = |_text: &str, _html: Option<&str>| {
        Err("native clipboard unavailable on Android".to_string())
    };
    // Decide before propagating setup success or failure: either result could otherwise
    // start a native write or fallback long after the user abandoned this request.
    begin_delivery()?;
    copy_to_clipboard_with(text, format, environment, osc52, native_copy, |text| {
        wsl::copy(text, wsl_deadline)
    })
}

/// Keeps a platform clipboard owner alive when the backend requires one.
///
/// On Linux/X11 and some Wayland compositors, clipboard contents are served by the
/// owning process. Dropping the `arboard::Clipboard` before the user pastes causes
/// the content to vanish. Store this lease on a session-lived owner so closing a
/// transient overlay does not release the clipboard contents. On non-Linux native paths and OSC 52
/// paths the lease is `None` — those backends do not require process-lifetime
/// ownership.
pub(crate) struct ClipboardLease {
    #[cfg(target_os = "linux")]
    _clipboard: Option<arboard::Clipboard>,
}

impl ClipboardLease {
    #[cfg(target_os = "linux")]
    fn native_linux(clipboard: arboard::Clipboard) -> Self {
        Self {
            _clipboard: Some(clipboard),
        }
    }

    #[cfg(test)]
    pub(crate) fn test() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            _clipboard: None,
        }
    }
}

/// Inputs to clipboard routing, independent of terminal identity.
#[derive(Clone, Copy)]
struct CopyEnvironment {
    terminal_copy: ClipboardCopyMode,
    wsl_session: bool,
}

impl CopyEnvironment {
    fn uses_native(self, text: &str) -> bool {
        match self.terminal_copy {
            ClipboardCopyMode::FallbackOsc52 => true,
            ClipboardCopyMode::Osc52 => false,
            ClipboardCopyMode::PreferTerminal {
                native_for_large_copies,
            } => native_for_large_copies && text.len() > OSC52_MAX_RAW_BYTES,
        }
    }
}

fn copy_to_clipboard_with(
    text: &str,
    format: CopyFormat,
    environment: CopyEnvironment,
    osc52_copy_fn: impl Fn(&str) -> Result<(), String>,
    arboard_copy_fn: impl FnOnce(&str, Option<&str>) -> Result<Option<ClipboardLease>, String>,
    wsl_copy_fn: impl Fn(&str) -> Result<(), String>,
) -> Result<CopyOutcome, String> {
    if text.is_empty() {
        return Err("nothing to copy: the selected content is empty".to_string());
    }

    if !environment.uses_native(text) {
        return osc52_copy_fn(text).map(|()| CopyOutcome::Unconfirmed(None));
    }

    let native_copy = || {
        let html = match format {
            CopyFormat::PlainText => None,
            CopyFormat::Markdown => Some(crate::clipboard_html::render_markdown(text)),
            CopyFormat::MarkdownSelection(source) => {
                Some(crate::clipboard_html::render_markdown(&source))
            }
        };
        arboard_copy_fn(text, html.as_deref()).map_err(|error| format!("native clipboard: {error}"))
    };
    if matches!(
        environment.terminal_copy,
        ClipboardCopyMode::PreferTerminal { .. }
    ) {
        return native_copy().map(CopyOutcome::Unconfirmed);
    }
    let native_result = if environment.wsl_session {
        wsl_copy_fn(text).map(|()| None).or_else(|windows_error| {
            native_copy().map_err(|native_error| {
                format!("Windows clipboard: {windows_error}; {native_error}")
            })
        })
    } else {
        native_copy()
    };
    match native_result {
        Ok(lease) => Ok(CopyOutcome::Copied(lease)),
        Err(native_error) => osc52_copy_fn(text)
            .map(|()| CopyOutcome::Unconfirmed(None))
            .map_err(|terminal_error| {
                format!("{native_error}; terminal clipboard: {terminal_error}")
            }),
    }
}

#[cfg(target_os = "linux")]
pub(crate) fn is_wsl_session() -> bool {
    crate::clipboard_paste::is_probably_wsl()
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn is_wsl_session() -> bool {
    false
}

/// Write to the native clipboard. The TUI owns any process-wide stderr redirection.
#[cfg(not(target_os = "android"))]
fn arboard_copy(
    mut clipboard: arboard::Clipboard,
    text: &str,
    html: Option<&str>,
) -> Result<Option<ClipboardLease>, String> {
    match html {
        Some(html) => clipboard
            .set_html(html, Some(text))
            .or_else(|_| clipboard.set_text(text)),
        None => clipboard.set_text(text),
    }
    .map_err(|e| format!("failed to set clipboard text: {e}"))?;
    // Linux clipboard owners must stay alive until the user pastes.
    #[cfg(target_os = "linux")]
    {
        Ok(Some(ClipboardLease::native_linux(clipboard)))
    }
    #[cfg(not(target_os = "linux"))]
    {
        Ok(None)
    }
}

/// Write text to the clipboard via the OSC 52 terminal escape sequence.
fn write_osc52(sequence: &str) -> Result<(), String> {
    #[cfg(unix)]
    {
        match std::fs::OpenOptions::new().write(true).open("/dev/tty") {
            Ok(tty) => match write_osc52_to_writer(tty, sequence) {
                Ok(()) => return Ok(()),
                Err(err) => tracing::debug!(
                    "failed to write OSC 52 to /dev/tty: {err}; falling back to stdout"
                ),
            },
            Err(err) => {
                tracing::debug!("failed to open /dev/tty for OSC 52: {err}; falling back to stdout")
            }
        }
    }

    write_osc52_to_writer(std::io::stdout().lock(), sequence)
}

fn write_osc52_to_writer(mut writer: impl Write, sequence: &str) -> Result<(), String> {
    writer
        .write_all(sequence.as_bytes())
        .map_err(|e| format!("failed to write OSC 52: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("failed to flush OSC 52: {e}"))
}

enum Osc52Target {
    Direct,
    Tmux,
    Screen,
}

fn osc52_sequence(text: &str, target: Osc52Target) -> Result<String, String> {
    let raw_bytes = text.len();
    if raw_bytes > OSC52_MAX_RAW_BYTES {
        return Err(format!(
            "OSC 52 payload too large ({raw_bytes} bytes; max {OSC52_MAX_RAW_BYTES})"
        ));
    }

    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    let osc = format!("\x1b]52;c;{encoded}\x07");
    Ok(match target {
        Osc52Target::Direct => osc,
        Osc52Target::Tmux => format!("\x1bPtmux;\x1b{osc}\x1b\\"),
        Osc52Target::Screen => {
            // Screen buffers each DCS separately. Stay below even its older 256-byte
            // string limit while forwarding one continuous OSC 52 to the outer terminal.
            const CHUNK_SIZE: usize = 252;
            let mut wrapped = String::new();
            for start in (0..osc.len()).step_by(CHUNK_SIZE) {
                let end = (start + CHUNK_SIZE).min(osc.len());
                // OSC 52 contains only ASCII, so every byte offset is a character boundary.
                wrapped.push_str("\x1bP");
                wrapped.push_str(&osc[start..end]);
                wrapped.push_str("\x1b\\");
            }
            wrapped
        }
    })
}

#[cfg(test)]
mod tests {
    use pretty_assertions::assert_eq;
    use std::cell::Cell;

    use super::ClipboardCopyMode;
    use super::CopyEnvironment;
    use super::CopyFormat;
    use super::CopyOutcome;
    use super::OSC52_MAX_RAW_BYTES;
    use super::Osc52Target;
    use super::copy_to_clipboard_with;
    use super::osc52_sequence;
    use super::write_osc52_to_writer;

    fn local_environment() -> CopyEnvironment {
        CopyEnvironment {
            terminal_copy: ClipboardCopyMode::FallbackOsc52,
            wsl_session: false,
        }
    }

    fn local_wsl_environment() -> CopyEnvironment {
        CopyEnvironment {
            wsl_session: true,
            ..local_environment()
        }
    }

    #[test]
    fn osc52_encoding_roundtrips() {
        use base64::Engine;
        let text = "# Hello\n\n```rust\nfn main() {}\n```\n";
        let sequence = osc52_sequence(text, Osc52Target::Direct).expect("OSC 52 sequence");
        let encoded = sequence
            .trim_start_matches("\u{1b}]52;c;")
            .trim_end_matches('\u{7}');
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(encoded)
            .unwrap();
        assert_eq!(decoded, text.as_bytes());
    }

    #[test]
    fn osc52_rejects_payload_larger_than_limit() {
        let text = "x".repeat(OSC52_MAX_RAW_BYTES + 1);
        assert_eq!(
            osc52_sequence(&text, Osc52Target::Direct),
            Err(format!(
                "OSC 52 payload too large ({} bytes; max {OSC52_MAX_RAW_BYTES})",
                OSC52_MAX_RAW_BYTES + 1
            ))
        );
    }

    #[test]
    fn osc52_wraps_tmux_passthrough() {
        assert_eq!(
            osc52_sequence("hello", Osc52Target::Tmux),
            Ok("\u{1b}Ptmux;\u{1b}\u{1b}]52;c;aGVsbG8=\u{7}\u{1b}\\".to_string())
        );
    }

    #[test]
    fn write_osc52_to_writer_emits_sequence_verbatim() {
        let sequence = "\u{1b}]52;c;aGVsbG8=\u{7}";
        let mut output = Vec::new();
        assert_eq!(write_osc52_to_writer(&mut output, sequence), Ok(()));
        assert_eq!(output, sequence.as_bytes());
    }

    #[test]
    fn local_uses_native_clipboard_first() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "hello",
            CopyFormat::PlainText,
            local_environment(),
            |_| {
                osc_calls.set(osc_calls.get() + 1);
                Ok(())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Ok(Some(super::ClipboardLease::test()))
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Ok(())
            },
        );

        assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
        assert_eq!(osc_calls.get(), 0);
        assert_eq!(native_calls.get(), 1);
        assert_eq!(wsl_calls.get(), 0);
    }

    #[test]
    fn local_copy_offers_html_only_for_markdown() {
        for (text, format, html) in [
            (
                "**hello**",
                CopyFormat::Markdown,
                Some("<p><strong>hello</strong></p>\n"),
            ),
            ("**hello**", CopyFormat::PlainText, None),
            (
                "hello /repo/my_notes!.txt",
                CopyFormat::MarkdownSelection(r"**hello** /repo/my\_notes\!.txt".into()),
                Some("<p><strong>hello</strong> /repo/my_notes!.txt</p>\n"),
            ),
        ] {
            for environment in [local_environment(), local_wsl_environment()] {
                let result = copy_to_clipboard_with(
                    text,
                    format.clone(),
                    environment,
                    |_| panic!("native copy should succeed"),
                    |actual_text, actual_html| {
                        assert_eq!((actual_text, actual_html), (text, html));
                        Ok(Some(super::ClipboardLease::test()))
                    },
                    |_| Err("Windows clipboard unavailable".into()),
                );
                assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
            }
        }
    }

    #[test]
    fn local_non_wsl_falls_back_to_osc52_when_native_fails() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "**hello**",
            CopyFormat::Markdown,
            local_environment(),
            |text| {
                assert_eq!(text, "**hello**");
                osc_calls.set(osc_calls.get() + 1);
                Ok(())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Err("native unavailable".into())
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Ok(())
            },
        );

        assert!(matches!(result, Ok(CopyOutcome::Unconfirmed(None))));
        assert_eq!(osc_calls.get(), 1);
        assert_eq!(native_calls.get(), 1);
        assert_eq!(wsl_calls.get(), 0);
    }

    #[test]
    fn local_wsl_uses_windows_and_skips_osc52_on_success() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "hello",
            CopyFormat::PlainText,
            local_wsl_environment(),
            |_| {
                osc_calls.set(osc_calls.get() + 1);
                Ok(())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Err("native unavailable".into())
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Ok(())
            },
        );

        assert!(matches!(result, Ok(CopyOutcome::Copied(None))));
        assert_eq!(osc_calls.get(), 0);
        assert_eq!(native_calls.get(), 0);
        assert_eq!(wsl_calls.get(), 1);
    }

    #[test]
    fn local_wsl_falls_back_to_osc52_when_powershell_fails() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "hello",
            CopyFormat::PlainText,
            local_wsl_environment(),
            |_| {
                osc_calls.set(osc_calls.get() + 1);
                Ok(())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Err("native unavailable".into())
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Err("powershell unavailable".into())
            },
        );

        assert!(matches!(result, Ok(CopyOutcome::Unconfirmed(None))));
        assert_eq!(osc_calls.get(), 1);
        assert_eq!(native_calls.get(), 1);
        assert_eq!(wsl_calls.get(), 1);
    }

    #[test]
    fn local_reports_both_errors_when_native_and_osc52_fail() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "hello",
            CopyFormat::PlainText,
            local_environment(),
            |_| {
                osc_calls.set(osc_calls.get() + 1);
                Err("osc blocked".into())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Err("native unavailable".into())
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Ok(())
            },
        );

        let Err(error) = result else {
            panic!("expected native and OSC 52 errors");
        };
        assert_eq!(
            error,
            "native clipboard: native unavailable; terminal clipboard: osc blocked"
        );
        assert_eq!(osc_calls.get(), 1);
        assert_eq!(native_calls.get(), 1);
        assert_eq!(wsl_calls.get(), 0);
    }

    #[test]
    fn local_wsl_reports_windows_and_osc52_errors_when_all_fail() {
        let osc_calls = Cell::new(/*value*/ 0_u8);
        let native_calls = Cell::new(/*value*/ 0_u8);
        let wsl_calls = Cell::new(/*value*/ 0_u8);
        let result = copy_to_clipboard_with(
            "hello",
            CopyFormat::PlainText,
            local_wsl_environment(),
            |_| {
                osc_calls.set(osc_calls.get() + 1);
                Err("osc blocked".into())
            },
            |_, _| {
                native_calls.set(native_calls.get() + 1);
                Err("native unavailable".into())
            },
            |_| {
                wsl_calls.set(wsl_calls.get() + 1);
                Err("powershell unavailable".into())
            },
        );

        let Err(error) = result else {
            panic!("expected Windows and OSC 52 errors");
        };
        assert_eq!(
            error,
            "Windows clipboard: powershell unavailable; native clipboard: native unavailable; terminal clipboard: osc blocked"
        );
        assert_eq!(osc_calls.get(), 1);
        assert_eq!(native_calls.get(), 1);
        assert_eq!(wsl_calls.get(), 1);
    }
}

#[cfg(test)]
#[path = "clipboard_copy/routing_tests.rs"]
mod routing_tests;
