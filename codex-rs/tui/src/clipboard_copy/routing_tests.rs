//! Clipboard routing, native ownership, and payload-boundary regressions.

use super::ClipboardLease;
use super::CopyEnvironment;
use super::CopyFormat;
use super::CopyOutcome;
use super::CopyStatus;
use super::OSC52_MAX_RAW_BYTES;
use super::copy_to_clipboard_with;
use pretty_assertions::assert_eq;
use std::cell::RefCell;

#[test]
fn rex_requests_terminal_copy_on_every_copy_without_native_writes() {
    for (ssh_session, tmux_session, wsl_session) in [
        (false, false, false),
        (true, false, true),
        (false, true, false),
    ] {
        let native_calls = std::cell::Cell::new(0);
        let terminal_calls = std::cell::Cell::new(0);
        // No client inventory is consulted: the same process can gain remote clients later.
        for text in ["local client", "remote client joined", "mixed clients"] {
            let result = copy_to_clipboard_with(
                text,
                CopyFormat::Markdown,
                CopyEnvironment {
                    rex_session: true,
                    ssh_session,
                    tmux_session,
                    wsl_session,
                    ..local_tmux_environment()
                },
                |_| panic!("Rex copies must not invoke tmux"),
                |actual| {
                    assert_eq!(actual, text);
                    terminal_calls.set(terminal_calls.get() + 1);
                    Ok(())
                },
                |_, _| {
                    native_calls.set(native_calls.get() + 1);
                    Ok(Some(ClipboardLease::test()))
                },
                |_| panic!("Rex copies must not write to the Windows clipboard"),
            );
            assert!(matches!(result, Ok(CopyOutcome::Requested)));
        }
        assert_eq!(native_calls.get(), 0);
        assert_eq!(terminal_calls.get(), 3);
    }
}

#[test]
fn rex_copy_preserves_payload_limits_empty_errors_and_terminal_failures() {
    let environment = CopyEnvironment {
        rex_session: true,
        tmux_session: false,
        ..local_tmux_environment()
    };
    for text in [String::new(), "x".repeat(OSC52_MAX_RAW_BYTES + 1)] {
        let result = copy_to_clipboard_with(
            &text,
            CopyFormat::PlainText,
            environment,
            |_| panic!("Rex must not invoke tmux"),
            |text| super::osc52_sequence(text, super::Osc52Target::Direct).map(|_| ()),
            |_, _| panic!("Rex must not write to the native clipboard"),
            |_| panic!("Rex must not write to the Windows clipboard"),
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
        |_| panic!("Rex must not invoke tmux"),
        |_| Err("terminal write failed".into()),
        |_, _| panic!("Rex must not write to the native clipboard"),
        |_| panic!("Rex must not write to the Windows clipboard"),
    );
    assert_eq!(result.err().as_deref(), Some("terminal write failed"));
}

#[test]
fn local_tmux_preserves_native_html_when_terminal_silently_rejects_copy() {
    let calls = RefCell::new(Vec::new());
    let result = copy_to_clipboard_with(
        "**hello**",
        CopyFormat::Markdown,
        CopyEnvironment {
            rex_session: false,
            herdr_session: false,
            ssh_session: false,
            forwarded_x11: false,
            tmux_session: true,
            wsl_session: false,
        },
        |_| {
            // Like default xterm: the command succeeds without delivery.
            calls.borrow_mut().push("tmux");
            Ok(())
        },
        |_| panic!("tmux send succeeded"),
        |text, html| {
            assert_eq!(
                (text, html),
                ("**hello**", Some("<p><strong>hello</strong></p>\n"))
            );
            calls.borrow_mut().push("native");
            Ok(Some(ClipboardLease::test()))
        },
        |_| panic!("native copy succeeded"),
    );
    assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
    assert_eq!(calls.into_inner(), vec!["native", "tmux"]);
}

#[test]
fn copy_uses_osc52_when_tmux_fails_even_if_native_copy_succeeds() {
    let calls = RefCell::new(Vec::new());
    let result = copy_to_clipboard_with(
        "hello",
        CopyFormat::PlainText,
        CopyEnvironment {
            rex_session: false,
            herdr_session: false,
            ssh_session: false,
            forwarded_x11: false,
            tmux_session: true,
            wsl_session: false,
        },
        |_| {
            calls.borrow_mut().push("tmux");
            Err("tmux unavailable".into())
        },
        |_| {
            calls.borrow_mut().push("osc52");
            Ok(())
        },
        |text, html| {
            assert_eq!((text, html), ("hello", None));
            calls.borrow_mut().push("native");
            Ok(Some(ClipboardLease::test()))
        },
        |_| panic!("native copy succeeded"),
    );
    assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
    assert_eq!(calls.into_inner(), vec!["native", "tmux", "osc52"]);
}

fn local_tmux_environment() -> CopyEnvironment {
    CopyEnvironment {
        rex_session: false,
        herdr_session: false,
        tmux_session: true,
        ssh_session: false,
        forwarded_x11: false,
        wsl_session: false,
    }
}

#[test]
fn herdr_forwards_to_osc52_even_when_native_copy_succeeds() {
    let calls = RefCell::new(Vec::new());
    let result = copy_to_clipboard_with(
        "hello",
        CopyFormat::PlainText,
        CopyEnvironment {
            rex_session: false,
            herdr_session: true,
            ssh_session: false,
            forwarded_x11: false,
            tmux_session: false,
            wsl_session: false,
        },
        |_| panic!("Herdr should use OSC 52 directly"),
        |_| {
            calls.borrow_mut().push("osc52");
            Ok(())
        },
        |text, html| {
            assert_eq!((text, html), ("hello", None));
            calls.borrow_mut().push("native");
            Ok(Some(ClipboardLease::test()))
        },
        |_| panic!("native copy succeeded"),
    );

    assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
    assert_eq!(calls.into_inner(), vec!["native", "osc52"]);
}

#[test]
fn oversized_tmux_copy_preserves_native_copy() {
    for (size, expected_calls) in [
        (OSC52_MAX_RAW_BYTES, vec!["native", "tmux"]),
        (OSC52_MAX_RAW_BYTES + 1, vec!["native", "osc52"]),
    ] {
        let text = "x".repeat(size);
        let calls = RefCell::new(Vec::new());
        let result = copy_to_clipboard_with(
            &text,
            CopyFormat::PlainText,
            local_tmux_environment(),
            |actual| {
                assert_eq!(actual, text);
                calls.borrow_mut().push("tmux");
                Ok(())
            },
            |_| {
                calls.borrow_mut().push("osc52");
                Err("payload too large".into())
            },
            |actual, html| {
                assert_eq!((actual, html), (text.as_str(), None));
                calls.borrow_mut().push("native");
                Ok(Some(ClipboardLease::test()))
            },
            |_| panic!("PowerShell fallback should not be needed"),
        );
        assert!(matches!(result, Ok(CopyOutcome::Copied(Some(_)))));
        assert_eq!(calls.into_inner(), expected_calls);
    }
}

#[test]
fn empty_copy_reports_failure_without_touching_clipboards() {
    let result = copy_to_clipboard_with(
        "",
        CopyFormat::PlainText,
        local_tmux_environment(),
        |_| panic!("empty input must not reach tmux"),
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
        let sequence = super::osc52_sequence(text, super::Osc52Target::Screen).unwrap();
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
