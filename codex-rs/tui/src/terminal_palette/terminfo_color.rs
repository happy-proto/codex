//! Supplement environment-based color detection with the local terminfo database.
//!
//! SSH forwards TERM with the PTY request, but usually omits COLORTERM. Read the declared
//! capabilities once instead of guessing from the terminal name or querying across the network.

use super::StdoutColorLevel;
use std::io::IsTerminal;
use std::sync::OnceLock;
use terminfo::Database;
use terminfo::capability::MaxColors;
use terminfo::capability::Value;

pub(super) fn supplement(level: StdoutColorLevel) -> StdoutColorLevel {
    // Match supports-color's process-wide cache; rendering must not repeatedly read the
    // database, inspect environment variables, or issue isatty syscalls for each styled span.
    static COLOR_LEVEL: OnceLock<StdoutColorLevel> = OnceLock::new();
    *COLOR_LEVEL.get_or_init(|| detect(level))
}

fn detect(level: StdoutColorLevel) -> StdoutColorLevel {
    // Keep explicit color controls and redirected output under supports-color's policy.
    if level == StdoutColorLevel::TrueColor
        || !std::io::stdout().is_terminal()
        || ["FORCE_COLOR", "CLICOLOR_FORCE", "NO_COLOR"]
            .iter()
            .any(|name| std::env::var_os(name).is_some())
        || std::env::var("TERM").is_ok_and(|term| term == "dumb")
    {
        return level;
    }

    let declared = Database::from_env()
        .ok()
        .and_then(|database| color_level(&database));
    match (level, declared) {
        (_, Some(StdoutColorLevel::TrueColor)) => StdoutColorLevel::TrueColor,
        (StdoutColorLevel::Unknown | StdoutColorLevel::Ansi16, Some(StdoutColorLevel::Ansi256)) => {
            StdoutColorLevel::Ansi256
        }
        (StdoutColorLevel::Unknown, Some(StdoutColorLevel::Ansi16)) => StdoutColorLevel::Ansi16,
        _ => level,
    }
}

fn color_level(database: &Database) -> Option<StdoutColorLevel> {
    let direct_color = matches!(database.raw("Tc"), Some(Value::True))
        || matches!(database.raw("RGB"), Some(Value::True))
        || matches!(database.raw("RGB"), Some(Value::Number(bits)) if *bits > 0)
        || ["setrgbf", "setrgbb"].iter().all(|name| {
            matches!(database.raw(name), Some(Value::String(sequence)) if !sequence.is_empty())
        });
    let colors = database.get::<MaxColors>().map(|colors| colors.0);
    if direct_color || colors.is_some_and(|colors| colors >= 1 << 24) {
        Some(StdoutColorLevel::TrueColor)
    } else if colors.is_some_and(|colors| colors >= 256) {
        Some(StdoutColorLevel::Ansi256)
    } else if colors.is_some_and(|colors| colors >= 8) {
        Some(StdoutColorLevel::Ansi16)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    #[test]
    fn reads_truecolor_extensions() {
        for (name, value) in [
            ("Tc", Value::True),
            ("RGB", Value::True),
            ("RGB", Value::Number(8)),
        ] {
            let mut database = Database::new();
            database.name("test").raw(name, value);
            assert_eq!(
                color_level(&database.build().unwrap()),
                Some(StdoutColorLevel::TrueColor),
                "{name}",
            );
        }
    }

    #[test]
    fn requires_both_rgb_setters() {
        for (foreground, background, expected) in [
            (
                b"fg".as_slice(),
                b"bg".as_slice(),
                Some(StdoutColorLevel::TrueColor),
            ),
            (b"fg", b"", None),
            (b"", b"bg", None),
        ] {
            let mut database = Database::new();
            database
                .name("test")
                .raw("setrgbf", foreground)
                .raw("setrgbb", background);
            assert_eq!(color_level(&database.build().unwrap()), expected);
        }
    }

    #[test]
    fn reads_color_count_without_assuming_256_colors_means_truecolor() {
        for (colors, expected) in [
            (0, None),
            (8, Some(StdoutColorLevel::Ansi16)),
            (16, Some(StdoutColorLevel::Ansi16)),
            (256, Some(StdoutColorLevel::Ansi256)),
            (1 << 24, Some(StdoutColorLevel::TrueColor)),
        ] {
            let mut database = Database::new();
            database.name("test").set(MaxColors(colors));
            assert_eq!(color_level(&database.build().unwrap()), expected);
        }
    }

    #[test]
    fn missing_or_disabled_capabilities_provide_no_evidence() {
        let mut database = Database::new();
        database.name("test").raw("RGB", 0);
        assert_eq!(color_level(&database.build().unwrap()), None);
    }
}
