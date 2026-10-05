//! Report the displayed local session's cwd to terminal integrations via OSC 7.

use std::io;
use std::io::IsTerminal;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

#[derive(Default)]
pub(crate) struct TerminalWorkingDirectory {
    last_written: Option<PathBuf>,
}

impl TerminalWorkingDirectory {
    pub(crate) fn update(&mut self, cwd: &Path, local_workspace: bool) -> io::Result<()> {
        let stdout = io::stdout();
        self.write_for(
            cwd,
            local_workspace,
            stdout.is_terminal(),
            &codex_config::os_host_name().unwrap_or_else(|| "localhost".to_string()),
            &mut stdout.lock(),
        )
    }

    fn write_for(
        &mut self,
        cwd: &Path,
        local_workspace: bool,
        interactive: bool,
        hostname: &str,
        writer: &mut impl Write,
    ) -> io::Result<()> {
        if !local_workspace || !interactive || self.last_written.as_deref() == Some(cwd) {
            return Ok(());
        }
        let Some(sequence) = osc7_sequence(cwd, hostname) else {
            return Ok(());
        };
        writer.write_all(sequence.as_bytes())?;
        writer.flush()?;
        self.last_written = Some(cwd.to_path_buf());
        Ok(())
    }
}

fn osc7_sequence(cwd: &Path, hostname: &str) -> Option<String> {
    // URL serialization percent-encodes path bytes, including OSC terminators,
    // spaces, non-ASCII characters, and non-UTF-8 Unix filenames.
    let mut url = url::Url::from_file_path(cwd).ok()?;
    url.set_host(Some(hostname)).ok()?;
    Some(format!("\x1b]7;{url}\x1b\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn encodes_unicode_spaces_and_terminal_control_bytes() {
        assert_eq!(
            osc7_sequence(Path::new("/tmp/中文 a\x07\x1b"), "workstation.local"),
            Some(
                "\x1b]7;file://workstation.local/tmp/%E4%B8%AD%E6%96%87%20a%07%1B\x1b\\"
                    .to_string()
            )
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_path_bytes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"/tmp/\xff"));
        assert_eq!(
            osc7_sequence(path, "host"),
            Some("\x1b]7;file://host/tmp/%FF\x1b\\".to_string())
        );
    }

    #[test]
    fn skips_relative_paths() {
        assert_eq!(osc7_sequence(Path::new("relative/path"), "host"), None);
    }

    #[cfg(unix)]
    #[test]
    fn only_announces_interactive_local_changes_and_deduplicates() {
        let mut state = TerminalWorkingDirectory::default();
        let mut output = Vec::new();
        state
            .write_for(Path::new("/first"), true, false, "host", &mut output)
            .unwrap();
        state
            .write_for(Path::new("/remote"), false, true, "host", &mut output)
            .unwrap();
        assert!(output.is_empty());
        state
            .write_for(Path::new("/first"), true, true, "host", &mut output)
            .unwrap();
        state
            .write_for(Path::new("/first"), true, true, "host", &mut output)
            .unwrap();
        state
            .write_for(Path::new("/worktree"), true, true, "host", &mut output)
            .unwrap();
        assert_eq!(
            output,
            b"\x1b]7;file://host/first\x1b\\\x1b]7;file://host/worktree\x1b\\"
        );
    }
}
