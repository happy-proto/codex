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
            &mut stdout.lock(),
        )
    }

    fn write_for(
        &mut self,
        cwd: &Path,
        local_workspace: bool,
        interactive: bool,
        writer: &mut impl Write,
    ) -> io::Result<()> {
        if !local_workspace || !interactive || self.last_written.as_deref() == Some(cwd) {
            return Ok(());
        }
        let Some(sequence) = osc7_sequence(cwd) else {
            return Ok(());
        };
        writer.write_all(sequence.as_bytes())?;
        writer.flush()?;
        self.last_written = Some(cwd.to_path_buf());
        Ok(())
    }
}

fn osc7_sequence(cwd: &Path) -> Option<String> {
    // URL serialization percent-encodes path bytes, including OSC terminators,
    // spaces, non-ASCII characters, and non-UTF-8 Unix filenames.
    let mut url = url::Url::from_file_path(cwd).ok()?;
    // 本层只报告本地会话。Ghostty 对内核主机名按字节比较，而 URL 会转小写；
    // 使用明确的本机别名，避免混合大小写主机名或运行期间改名导致序列被拒绝。
    url.set_host(Some("localhost")).ok()?;
    Some(format!("\x1b]7;{url}\x1b\\"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn uses_localhost_authority_for_local_directory() {
        // Ghostty 按字节校验本机名；URL 标准化会将域名转小写，无法保留混合大小写。
        assert_eq!(
            osc7_sequence(Path::new("/session")),
            Some("\x1b]7;file://localhost/session\x1b\\".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn encodes_unicode_spaces_and_terminal_control_bytes() {
        assert_eq!(
            osc7_sequence(Path::new("/tmp/中文 a\x07\x1b")),
            Some("\x1b]7;file://localhost/tmp/%E4%B8%AD%E6%96%87%20a%07%1B\x1b\\".to_string())
        );
    }

    #[cfg(unix)]
    #[test]
    fn preserves_non_utf8_path_bytes() {
        use std::ffi::OsStr;
        use std::os::unix::ffi::OsStrExt;
        let path = Path::new(OsStr::from_bytes(b"/tmp/\xff"));
        assert_eq!(
            osc7_sequence(path),
            Some("\x1b]7;file://localhost/tmp/%FF\x1b\\".to_string())
        );
    }

    #[test]
    fn skips_relative_paths() {
        assert_eq!(osc7_sequence(Path::new("relative/path")), None);
    }

    #[cfg(unix)]
    #[test]
    fn only_announces_interactive_local_changes_and_deduplicates() {
        let mut state = TerminalWorkingDirectory::default();
        let mut output = Vec::new();
        state
            .write_for(Path::new("/first"), true, false, &mut output)
            .unwrap();
        state
            .write_for(Path::new("/remote"), false, true, &mut output)
            .unwrap();
        assert!(output.is_empty());
        state
            .write_for(Path::new("/first"), true, true, &mut output)
            .unwrap();
        state
            .write_for(Path::new("/first"), true, true, &mut output)
            .unwrap();
        state
            .write_for(Path::new("/worktree"), true, true, &mut output)
            .unwrap();
        assert_eq!(
            output,
            b"\x1b]7;file://localhost/first\x1b\\\x1b]7;file://localhost/worktree\x1b\\"
        );
    }
}
