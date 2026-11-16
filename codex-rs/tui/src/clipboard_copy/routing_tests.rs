//! Clipboard routing, native ownership, and payload-boundary regressions.

use super::ClipboardCopyMode;
use super::ClipboardLease;
use super::CopyEnvironment;
use super::CopyFormat;
use super::CopyOutcome;
use super::CopyStatus;
use super::OSC52_MAX_RAW_BYTES;
use super::copy_to_clipboard_with;
use codex_terminal_detection::TerminalName;
use pretty_assertions::assert_eq;

#[test]
fn osc52_mode_requests_terminal_copy_on_every_copy_without_native_writes() {
    for wsl_session in [false, true] {
        let native_calls = std::cell::Cell::new(0);
        let terminal_calls = std::cell::Cell::new(0);
        // No client inventory is consulted: the same process can gain remote clients later.
        for text in ["local client", "remote client joined", "mixed clients"] {
            let result = copy_to_clipboard_with(
                text,
                CopyFormat::Markdown,
                CopyEnvironment {
                    terminal_copy: ClipboardCopyMode::Osc52,
                    wsl_session,
                },
                |actual| {
                    assert_eq!(actual, text);
                    terminal_calls.set(terminal_calls.get() + 1);
                    Ok(())
                },
                |_, _| {
                    native_calls.set(native_calls.get() + 1);
                    Ok(Some(ClipboardLease::test()))
                },
                |_| panic!("OSC 52 mode must not write to the Windows clipboard"),
            );
            assert!(matches!(result, Ok(CopyOutcome::Unconfirmed(None))));
        }
        assert_eq!(native_calls.get(), 0);
        assert_eq!(terminal_calls.get(), 3);
    }
}

#[test]
fn osc52_mode_preserves_payload_limits_empty_errors_and_terminal_failures() {
    let environment = CopyEnvironment {
        terminal_copy: ClipboardCopyMode::Osc52,
        wsl_session: false,
    };
    for text in [String::new(), "x".repeat(OSC52_MAX_RAW_BYTES + 1)] {
        let result = copy_to_clipboard_with(
            &text,
            CopyFormat::PlainText,
            environment,
            |text| super::osc52_sequence(text, super::Osc52Target::Direct).map(|_| ()),
            |_, _| panic!("OSC 52 mode must not write to the native clipboard"),
            |_| panic!("OSC 52 mode must not write to the Windows clipboard"),
        );
        let error = result.err().expect("invalid payload should fail");
        assert!(error.contains(if text.is_empty() {
            "empty"
        } else {
            "too large"
        }));
    }
    let result = copy_to_clipboard_with(
        "hello",
        CopyFormat::PlainText,
        environment,
        |_| Err("terminal write failed".into()),
        |_, _| panic!("OSC 52 mode must not write to the native clipboard"),
        |_| panic!("OSC 52 mode must not write to the Windows clipboard"),
    );
    assert_eq!(result.err().as_deref(), Some("terminal write failed"));
}

#[test]
fn dev_container_uses_native_only_for_oversized_copies_with_shared_wayland() {
    let oversized = "x".repeat(OSC52_MAX_RAW_BYTES + 1);
    for (text, shared_wayland, expect_native) in [
        ("small", true, false),
        (oversized.as_str(), false, false),
        (oversized.as_str(), true, true),
    ] {
        let result = copy_to_clipboard_with(
            text,
            CopyFormat::PlainText,
            CopyEnvironment {
                terminal_copy: ClipboardCopyMode::PreferTerminal {
                    native_for_large_copies: shared_wayland,
                },
                wsl_session: false,
            },
            |_| {
                assert!(!expect_native);
                Ok(())
            },
            |_, _| {
                assert!(expect_native);
                Ok(Some(ClipboardLease::test()))
            },
            |_| panic!("dev-container routing must not write to the Windows clipboard"),
        );
        let CopyOutcome::Unconfirmed(lease) = result.unwrap() else {
            panic!("host delivery cannot be confirmed");
        };
        assert_eq!(lease.is_some(), expect_native);
    }
}

#[test]
fn rex_selects_osc52_without_ssh_or_tmux() {
    assert_eq!(
        ClipboardCopyMode::for_terminal(TerminalName::Rex),
        ClipboardCopyMode::Osc52,
    );
}

#[test]
fn empty_copy_reports_failure_without_touching_clipboards() {
    let result = copy_to_clipboard_with(
        "",
        CopyFormat::PlainText,
        CopyEnvironment {
            terminal_copy: ClipboardCopyMode::Osc52,
            wsl_session: false,
        },
        |_| panic!("empty input must not reach OSC 52"),
        |_, _| panic!("empty input must not reach the native clipboard"),
        |_| panic!("empty input must not reach PowerShell"),
    );
    insta::assert_snapshot!(result.err().expect("empty selection should fail"));
}

#[test]
fn confirmed_copy_without_new_lease_preserves_native_ownership() {
    let mut lease = Some(ClipboardLease::test());

    assert_eq!(
        CopyOutcome::Copied(None).store(&mut lease),
        CopyStatus::Confirmed
    );
    assert!(lease.is_some());
}

#[test]
fn screen_forwards_one_complete_osc52_across_bounded_passthrough_strings() {
    for text in ["hello", &"世界".repeat(500)] {
        let sequence = super::osc52_sequence(text, super::Osc52Target::ChunkedDcs).unwrap();
        let chunks = sequence
            .strip_prefix("\x1bP")
            .unwrap()
            .strip_suffix("\x1b\\")
            .unwrap();
        let mut forwarded = String::new();
        for chunk in chunks.split("\x1b\\\x1bP") {
            assert!(chunk.len() < 256);
            forwarded.push_str(chunk);
        }
        assert_eq!(
            forwarded,
            super::osc52_sequence(text, super::Osc52Target::Direct).unwrap()
        );
    }
}
