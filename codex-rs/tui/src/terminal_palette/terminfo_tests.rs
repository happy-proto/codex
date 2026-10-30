use super::*;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::process::Command;
use std::process::Stdio;

#[test]
fn terminfo_restores_ssh_truecolor_without_colorterm() {
    const CHILD: &str = "CODEX_TEST_TERMINFO_COLOR_CHILD";
    if let Ok(expected) = std::env::var(CHILD) {
        let expected = match expected.as_str() {
            "truecolor" => StdoutColorLevel::TrueColor,
            "256" => StdoutColorLevel::Ansi256,
            "16" => StdoutColorLevel::Ansi16,
            "none" => StdoutColorLevel::Unknown,
            _ => panic!("unknown expected color level: {expected}"),
        };
        assert_eq!(stdout_color_level(), expected);
        assert_eq!(effective_stdout_color_level(), expected);
        if expected == StdoutColorLevel::TrueColor {
            assert_eq!(best_color((12, 34, 56)), rgb_color((12, 34, 56)));
        }
        return;
    }
    run_child("truecolor", true, Some(TRUECOLOR_ENTRY), &[]);
}

const TRUECOLOR_ENTRY: &[u8] = include_bytes!("fixtures/truecolor.terminfo");

#[test]
fn terminfo_preserves_explicit_color_controls() {
    for (name, value, expected) in [
        ("FORCE_COLOR", "1", "16"),
        ("FORCE_COLOR", "2", "256"),
        ("FORCE_COLOR", "3", "truecolor"),
        ("NO_COLOR", "1", "none"),
        ("CLICOLOR_FORCE", "1", "16"),
        ("TERM", "dumb", "none"),
    ] {
        run_child(expected, true, Some(TRUECOLOR_ENTRY), &[(name, value)]);
    }
}

#[test]
fn terminfo_does_not_enable_colors_for_redirected_stdout() {
    run_child("none", false, Some(TRUECOLOR_ENTRY), &[]);
}

#[test]
fn terminfo_missing_and_corrupt_entries_keep_environment_detection() {
    run_child("16", true, None, &[]);
    run_child("16", true, Some(b"not a terminfo database"), &[]);
}

fn run_child(expected: &str, tty: bool, entry: Option<&[u8]>, env: &[(&str, &str)]) {
    let directory = tempfile::tempdir().unwrap();
    if let Some(content) = entry {
        let entry = directory.path().join("x/xterm-codex-terminfo-test");
        std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
        std::fs::write(entry, content).unwrap();
    }

    let mut master = -1;
    let mut slave = -1;
    // SAFETY: openpty initializes both descriptors; null pointers select default settings.
    assert_eq!(
        unsafe {
            libc::openpty(
                &mut master,
                &mut slave,
                std::ptr::null_mut(),
                std::ptr::null(),
                std::ptr::null(),
            )
        },
        0,
    );
    // SAFETY: openpty succeeded and returned two owned descriptors.
    let _master = unsafe { File::from_raw_fd(master) };
    // SAFETY: the slave descriptor is owned and has not been wrapped before.
    let slave = unsafe { File::from_raw_fd(slave) };
    let mut command = Command::new(std::env::current_exe().unwrap());
    command
        .args([
            "--exact",
            "terminal_palette::terminfo_tests::terminfo_restores_ssh_truecolor_without_colorterm",
            "--nocapture",
        ])
        .env("CODEX_TEST_TERMINFO_COLOR_CHILD", expected)
        .env("TERM", "xterm-codex-terminfo-test")
        .env("TERMINFO", directory.path())
        .env("TERMINFO_DIRS", directory.path());
    if tty {
        command.stdout(Stdio::from(slave));
    }
    for name in [
        "COLORTERM",
        "TERM_PROGRAM",
        "FORCE_COLOR",
        "CLICOLOR_FORCE",
        "CLICOLOR",
        "NO_COLOR",
        "WT_SESSION",
        "IGNORE_IS_TERMINAL",
        "CI",
    ] {
        command.env_remove(name);
    }
    command.envs(env.iter().copied());
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "expected {expected}, tty={tty}, env={env:?}: {}",
        String::from_utf8_lossy(&output.stderr),
    );
}
