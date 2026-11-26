use std::io;
use std::io::Write;
use std::path::Path;
use std::sync::OnceLock;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use sha2::Digest;
use sha2::Sha256;

static TOKEN: OnceLock<String> = OnceLock::new();

/// Accept the token taken from the CLI's startup environment.
pub fn set_program_resume_token(token: String) {
    if (22..=64).contains(&token.len())
        && token
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
    {
        let _ = TOKEN.set(token);
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct Target {
    pub(crate) thread_id: String,
    pub(crate) cwd: String,
}

pub(crate) struct Reporter {
    token: &'static str,
    reported: Option<Target>,
    refresh: bool,
}

impl Reporter {
    pub(crate) fn new() -> Option<Self> {
        Some(Self {
            token: TOKEN.get()?.as_str(),
            reported: None,
            refresh: false,
        })
    }

    #[cfg(unix)]
    pub(crate) fn refresh(&mut self, writer: &mut impl Write) -> io::Result<()> {
        self.refresh = self.reported.is_some();
        self.update(writer, self.reported.clone())
    }

    pub(crate) fn update(
        &mut self,
        writer: &mut impl Write,
        target: Option<Target>,
    ) -> io::Result<()> {
        if target == self.reported && !self.refresh {
            return Ok(());
        }
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(io::Error::other)?
            .as_secs();
        let pid = std::process::id();
        if let Some((target, sequence)) = target.and_then(|target| {
            sequence(self.token, pid, time, Some(&target)).map(|sequence| (target, sequence))
        }) {
            writer.write_all(sequence.as_bytes())?;
            writer.flush()?;
            self.reported = Some(target);
        } else if self.reported.is_some() {
            let sequence = sequence(self.token, pid, time, None).expect("clear report fits");
            writer.write_all(sequence.as_bytes())?;
            writer.flush()?;
            self.reported = None;
        }
        self.refresh = false;
        Ok(())
    }
}

fn sequence(token: &str, pid: u32, time: u64, target: Option<&Target>) -> Option<String> {
    let fields = match target {
        Some(Target { thread_id, cwd }) => {
            if !Path::new(cwd).is_absolute() || cwd.chars().any(char::is_control) {
                return None;
            }
            let argv = ["codex", "resume", thread_id]
                .map(|arg| STANDARD.encode(arg))
                .join(",");
            format!("argv={argv}:cwd={}", STANDARD.encode(cwd))
        }
        None => "clear=1".to_string(),
    };
    let body = format!("pid={pid}:time={time}:{fields}");
    let mac = Sha256::new()
        .chain_update(body.as_bytes())
        .chain_update(b":")
        .chain_update(token.as_bytes())
        .finalize();
    let sequence = format!("\x1b]7577;{body}:mac={mac:x}\x1b\\");
    (sequence.len() <= 2048).then_some(sequence)
}

#[cfg(test)]
#[path = "program_resume_tests.rs"]
mod tests;
