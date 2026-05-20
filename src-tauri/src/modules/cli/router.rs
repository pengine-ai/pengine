//! Transport-agnostic classifier.
//!
//! Two entry points feed into the handler layer:
//!
//! - [`classify_line`] — REPL / Telegram `$`-prefix path. Splits a free-text
//!   line into a native slash command, an agent message, or an unknown-slash
//!   error. (Used from PR 2 onwards.)
//! - One-shot `tauri-plugin-cli` matches bypass this file and go straight to
//!   [`super::bootstrap::dispatch`].
//!
//! Invariant: [`RouterOutcome::Unknown`] never converts into
//! [`RouterOutcome::Agent`] — this prevents the model from learning native
//! command names by observing echoed error text.

use super::commands;

#[derive(Debug, PartialEq, Eq)]
pub enum RouterOutcome<'a> {
    /// A recognized native command; `name` is the keyword (without the `/`),
    /// `rest` is the remaining argument text (may be empty).
    Native { name: &'static str, rest: &'a str },
    /// Free text — forward to the agent.
    Agent(&'a str),
    /// Slash-prefixed input with an unregistered name. Report to the user
    /// only; do not forward to the agent.
    Unknown(&'a str),
}

/// Classify a single line. Leading whitespace is tolerated.
pub fn classify_line(line: &str) -> RouterOutcome<'_> {
    let trimmed = line.trim_start();
    let Some(rest) = trimmed.strip_prefix('/') else {
        return RouterOutcome::Agent(trimmed);
    };
    let (name, tail) = match rest.find(char::is_whitespace) {
        Some(idx) => (&rest[..idx], rest[idx..].trim_start()),
        None => (rest, ""),
    };
    match commands::lookup(name) {
        Some(cmd) => RouterOutcome::Native {
            name: cmd.name,
            rest: tail,
        },
        // /nothink and /think are model directives stripped by run_turn — pass
        // the full original line through as an agent message, not an error.
        None if name == "nothink" || name == "think" => RouterOutcome::Agent(trimmed),
        None => RouterOutcome::Unknown(name),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_routes_to_agent() {
        assert_eq!(
            classify_line("hello world"),
            RouterOutcome::Agent("hello world")
        );
    }

    #[test]
    fn slash_known_routes_native_with_args() {
        assert_eq!(
            classify_line("/config skills_hint_max_bytes=12000"),
            RouterOutcome::Native {
                name: "config",
                rest: "skills_hint_max_bytes=12000",
            }
        );
    }

    #[test]
    fn slash_known_bare() {
        assert_eq!(
            classify_line("/help"),
            RouterOutcome::Native {
                name: "help",
                rest: "",
            }
        );
    }

    #[test]
    fn slash_unknown_stays_in_user_channel() {
        // Critical invariant: must not fall through to Agent.
        assert_eq!(
            classify_line("/deploy --dry-run"),
            RouterOutcome::Unknown("deploy")
        );
    }

    #[test]
    fn nothink_and_think_pass_through_as_agent() {
        // Model directives handled by run_turn — must not surface as errors.
        assert_eq!(
            classify_line("/nothink say hi"),
            RouterOutcome::Agent("/nothink say hi")
        );
        assert_eq!(
            classify_line("/think solve this"),
            RouterOutcome::Agent("/think solve this")
        );
        assert_eq!(classify_line("/nothink"), RouterOutcome::Agent("/nothink"));
    }
}
