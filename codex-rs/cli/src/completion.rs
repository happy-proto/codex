use std::ffi::OsStr;
use std::ffi::OsString;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use clap::CommandFactory;
use clap::Parser;
use clap_complete::ArgValueCompleter;
use clap_complete::CompleteEnv;
use clap_complete::CompletionCandidate;
use clap_complete::env::EnvCompleter;
use clap_complete::env::Fish;
use clap_complete::env::Shells;
use codex_state::SqliteConfig;
use codex_state::ThreadCompletion;
use codex_utils_absolute_path::AbsolutePathBuf;
use codex_utils_cli::CliConfigOverrides;
use codex_utils_cli::ProfileV2Name;

use crate::MultitoolCli;
use crate::Subcommand;
use crate::find_codex_home;
use crate::resolve_profile_v2_config_path;

const QUERY_TIMEOUT: Duration = Duration::from_millis(250);
const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// Run before arg0 dispatch, which creates helper files and starts a Tokio runtime.
pub(super) fn complete() {
    let Some(shell) = std::env::var_os("COMPLETE") else {
        return;
    };
    if shell.is_empty() || shell == "0" {
        return;
    }
    let args = std::env::args_os().collect::<Vec<_>>();
    let index = std::env::var("_CLAP_COMPLETE_INDEX")
        .ok()
        .and_then(|value| value.parse().ok());
    let completion = CompleteEnv::with_factory(|| command(&args, index, &shell));
    if Path::new(&shell).file_stem() == Some(OsStr::new("fish")) {
        completion
            .shells(Shells(&[&FishWithUnescapedToken]))
            .complete();
    } else {
        completion.complete();
    }
}

/// Fish's current-token output retains quotes and escapes, unlike its preceding tokens.
/// Let Fish decode it, including unfinished quotes, before invoking clap's engine.
struct FishWithUnescapedToken;

impl EnvCompleter for FishWithUnescapedToken {
    fn name(&self) -> &'static str {
        Fish.name()
    }

    fn is(&self, name: &str) -> bool {
        Fish.is(name)
    }

    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn std::io::Write,
    ) -> std::io::Result<()> {
        let mut script = Vec::new();
        Fish.write_registration(var, name, bin, completer, &mut script)?;
        let script = String::from_utf8(script)
            .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?;
        let token = "(commandline --current-token)";
        if !script.contains(token) {
            return Err(std::io::Error::other("clap Fish registration changed"));
        }
        buf.write_all(
            script
                .replace(
                    token,
                    "(commandline --current-token --cut-at-cursor | string unescape)",
                )
                .as_bytes(),
        )
    }

    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<OsString>,
        current_dir: Option<&Path>,
        buf: &mut dyn std::io::Write,
    ) -> std::io::Result<()> {
        Fish.write_complete(cmd, args, current_dir, buf)
    }
}

fn command(args: &[OsString], index: Option<usize>, shell: &OsStr) -> clap::Command {
    let command = MultitoolCli::command();
    let Some(separator) = args.iter().position(|arg| arg == "--") else {
        return command;
    };
    let request = &args[separator + 1..];
    // Bash and Zsh provide the cursor index; Fish/PowerShell pass only up to the cursor.
    let index = index.unwrap_or_else(|| request.len().saturating_sub(1));
    if index > request.len() {
        return command;
    }
    let mut context_args = request.to_vec();
    if index < context_args.len() {
        context_args.remove(index);
    }
    // Include options after the cursor too, especially --remote and --all.
    let Some(context) = SessionContext::parse(&context_args) else {
        return command;
    };
    let zsh = Path::new(shell).file_stem() == Some(OsStr::new("zsh"));
    let subcommand = context.subcommand;
    let completer = ArgValueCompleter::new(move |current: &OsStr| context.candidates(current, zsh));
    command.mut_subcommand(subcommand, |command| {
        // mut_arg removes and re-appends its argument, changing inferred positional indexes.
        command.mut_args(|arg| {
            if arg.get_id() == "session_id" {
                arg.add(completer.clone())
            } else {
                arg
            }
        })
    })
}

struct SessionContext {
    subcommand: &'static str,
    cwd: Option<PathBuf>,
    profile: Option<ProfileV2Name>,
    overrides: CliConfigOverrides,
    include_non_interactive: bool,
}

impl SessionContext {
    fn parse(prefix: &[OsString]) -> Option<Self> {
        let cli = MultitoolCli::try_parse_from(prefix).ok()?;
        if cli.remote.remote.is_some() {
            return None;
        }
        let (subcommand, session_id, last, all, remote, tui, include_non_interactive) =
            match cli.subcommand? {
                Subcommand::Resume(command) => (
                    "resume",
                    command.session_id,
                    command.last,
                    command.all,
                    command.remote,
                    command.config_overrides.0,
                    command.include_non_interactive,
                ),
                Subcommand::Fork(command) => (
                    "fork",
                    command.session_id,
                    command.last,
                    command.all,
                    command.remote,
                    command.config_overrides.0,
                    false,
                ),
                _ => return None,
            };
        if last || session_id.is_some() || remote.remote.is_some() {
            return None;
        }
        let cwd = if all {
            None
        } else {
            let base = std::env::current_dir().ok()?;
            let path = tui
                .shared
                .cwd
                .as_ref()
                .or(cli.interactive.shared.cwd.as_ref());
            let path = path
                .map(|path| AbsolutePathBuf::resolve_path_against_base(path, &base).to_path_buf())
                .unwrap_or(base);
            Some(path.canonicalize().ok()?)
        };
        let profile = tui.shared.config_profile_v2.clone().or(cli
            .interactive
            .shared
            .config_profile_v2
            .clone());
        let mut overrides = tui.config_overrides;
        overrides.prepend_root_overrides(cli.config_overrides);
        Some(Self {
            subcommand,
            cwd,
            profile,
            overrides,
            include_non_interactive,
        })
    }

    fn sqlite(&self) -> anyhow::Result<SqliteConfig> {
        let home = find_codex_home()?;
        let mut sqlite_home = configured_sqlite_home(&home.join("config.toml"))?;
        if let Some(profile) = self.profile.as_ref() {
            let path = resolve_profile_v2_config_path(home.as_path(), profile);
            if !path.as_path().is_file() {
                anyhow::bail!("completion profile does not exist");
            }
            sqlite_home = configured_sqlite_home(path.as_path())?.or(sqlite_home);
        }
        let base = std::env::current_dir()?;
        for (key, value) in self
            .overrides
            .parse_overrides()
            .map_err(anyhow::Error::msg)?
        {
            if key == "sqlite_home" {
                let value = value
                    .as_str()
                    .ok_or_else(|| anyhow::anyhow!("sqlite_home must be a string"))?;
                sqlite_home = Some(AbsolutePathBuf::resolve_path_against_base(value, &base));
            }
        }
        let sqlite_home = sqlite_home
            .or_else(|| {
                std::env::var_os("CODEX_SQLITE_HOME")
                    .filter(|value| !value.is_empty())
                    .map(|value| AbsolutePathBuf::resolve_path_against_base(value, &base))
            })
            .unwrap_or(home);
        Ok(SqliteConfig::from_sqlite_home(sqlite_home))
    }

    fn candidates(&self, current: &OsStr, zsh: bool) -> Vec<CompletionCandidate> {
        let Some(query) = current.to_str() else {
            return Vec::new();
        };
        let Ok(sqlite) = self.sqlite() else {
            return Vec::new();
        };
        let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        else {
            return Vec::new();
        };
        let result = runtime.block_on(async {
            tokio::time::timeout(QUERY_TIMEOUT, async {
                codex_state::read_thread_completions(
                    &sqlite,
                    query,
                    self.cwd.as_deref(),
                    self.include_non_interactive,
                )
                .await
            })
            .await
        });
        match result {
            Ok(Ok(threads)) => candidates(threads, query, zsh),
            _ => Vec::new(),
        }
    }
}

fn configured_sqlite_home(path: &Path) -> anyhow::Result<Option<AbsolutePathBuf>> {
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    let mut content = String::new();
    file.take(MAX_CONFIG_BYTES + 1)
        .read_to_string(&mut content)?;
    if content.len() as u64 > MAX_CONFIG_BYTES {
        anyhow::bail!("completion config is too large");
    }
    let config: toml::Value = toml::from_str(&content)?;
    Ok(config
        .get("sqlite_home")
        .and_then(toml::Value::as_str)
        .map(|value| {
            AbsolutePathBuf::resolve_path_against_base(
                value,
                path.parent().unwrap_or(Path::new(".")),
            )
        }))
}

fn candidates(threads: Vec<ThreadCompletion>, query: &str, zsh: bool) -> Vec<CompletionCandidate> {
    threads
        .into_iter()
        .filter_map(|thread| {
            let id_matches = thread.id.starts_with(query);
            // Zsh filters the inserted value again. For a name prefix, insert the exact
            // unique label so it remains a valid resume argument. Other shells can insert UUIDs.
            let value = if !id_matches && zsh {
                if !thread.unique_label || !thread.label.starts_with(query) {
                    return None;
                }
                thread.label.clone()
            } else {
                thread.id.clone()
            };
            if value.chars().any(char::is_control) {
                return None;
            }
            let help = format!(
                "{} · {} · {}",
                thread.label,
                thread.cwd.display(),
                thread.id
            );
            let help = help
                .chars()
                .map(|ch| if ch.is_control() { ' ' } else { ch })
                .take(240)
                .collect::<String>();
            Some(CompletionCandidate::new(value).help(Some(help.into())))
        })
        .enumerate()
        .map(|(index, candidate)| candidate.display_order(Some(index)))
        .collect()
}

#[cfg(test)]
#[path = "completion_tests.rs"]
mod tests;
