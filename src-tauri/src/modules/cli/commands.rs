//! Native command registry — single source of truth for the CLI surface.
//!
//! The registry drives `pengine help` and `GET /v1/cli/commands`. Adding a command
//! is one entry here + one handler function + (for subcommand dispatch) one arm
//! in [`super::bootstrap`].

use serde::Serialize;

/// Metadata for a native (CLI-only) command.
#[derive(Debug, Clone, Copy, Serialize)]
pub struct NativeCommand {
    pub name: &'static str,
    pub summary: &'static str,
}

/// Canonical registry. Order is the order `help` prints them.
pub const COMMANDS: &[NativeCommand] = &[
    NativeCommand {
        name: "help",
        summary: "Show this help.",
    },
    NativeCommand {
        name: "app",
        summary:
            "Open the desktop window (new process; run alongside a terminal `pengine` session).",
    },
    NativeCommand {
        name: "version",
        summary: "Print the Pengine version and git commit.",
    },
    NativeCommand {
        name: "status",
        summary: "Show bot, Ollama, and MCP status.",
    },
    NativeCommand {
        name: "config",
        summary: "Show or set user settings (e.g. skills_hint_max_bytes=12000).",
    },
    NativeCommand {
        name: "model",
        summary: "Show or set the preferred Ollama model.",
    },
    NativeCommand {
        name: "bot",
        summary: "Connect or disconnect the Telegram bot.",
    },
    NativeCommand {
        name: "tools",
        summary: "List MCP tools (optional search substring).",
    },
    NativeCommand {
        name: "skills",
        summary: "List, enable, or disable skills.",
    },
    NativeCommand {
        name: "fs",
        summary: "List, add, or remove MCP filesystem roots.",
    },
    NativeCommand {
        name: "logs",
        summary: "Stream log events (--follow / --tail).",
    },
    NativeCommand {
        name: "ask",
        summary: "Send a message to the agent (AI path).",
    },
];

pub fn lookup(name: &str) -> Option<&'static NativeCommand> {
    COMMANDS.iter().find(|c| c.name == name)
}
