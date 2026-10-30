use super::*;
use std::fs::File;
use std::os::fd::FromRawFd;
use std::process::Command;
use std::process::Stdio;

#[test]
fn terminfo_restores_ssh_truecolor_without_colorterm() {
    const CHILD: &str = "CODEX_TEST_TERMINFO_COLOR_CHILD";
    if std::env::var_os(CHILD).is_some() {
        assert_eq!(stdout_color_level(), StdoutColorLevel::TrueColor);
        assert_eq!(effective_stdout_color_level(), StdoutColorLevel::TrueColor);
        assert_eq!(best_color((12, 34, 56)), rgb_color((12, 34, 56)));
        return;
    }

    let directory = tempfile::tempdir().unwrap();
    let entry = directory.path().join("x/xterm-codex-terminfo-test");
    std::fs::create_dir_all(entry.parent().unwrap()).unwrap();
    std::fs::write(entry, include_bytes!("fixtures/truecolor.terminfo")).unwrap();

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
        ])
        .env(CHILD, "1")
        .env("TERM", "xterm-codex-terminfo-test")
        .env("TERMINFO", directory.path())
        .env("TERMINFO_DIRS", directory.path())
        .stdout(Stdio::from(slave));
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
    let output = command.output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr),
    );
}
