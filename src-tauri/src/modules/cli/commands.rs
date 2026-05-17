//! in [`super::bootstrap`].

use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
pub struct NativeCommand {
    pub name: &'static str,
    pub summary: &'static str,
    pub details: &'static str,
}

pub const COMMANDS: &[NativeCommand] = &[
    NativeCommand {
        name: "help",
        summary: "Show this help (or detailed help for a specific command).",
        details:
            "Usage: /help [command]\n\nWith no argument, lists every command.\nWith an argument, prints detailed usage for that command.",
    },
    NativeCommand {
        name: "app",
        summary:
            "Open the desktop window (new process; run alongside a terminal `pengine` session).",
        details:
            "Usage: pengine app\n\nLaunches the Pengine desktop window in a separate process so the\nterminal session can keep running. Not available over the Telegram bridge.",
    },
    NativeCommand {
        name: "version",
        summary: "Print the Pengine version and git commit.",
        details: "Usage: pengine version  (alias: -V, --version)",
    },
    NativeCommand {
        name: "status",
        summary: "Show bot, Ollama, and MCP status.",
        details:
            "Usage: pengine status\n\nReports: Telegram bot connection, active + preferred Ollama model,\nnumber of MCP tools, key user settings, and store path.",
    },
    NativeCommand {
        name: "config",
        summary: "Show or set user settings (e.g. skills_hint_max_bytes=12000).",
        details:
            "Usage: pengine config              # list settings\n       pengine config key=value   # set (clamped to allowed range)\n\nKnown keys: skills_hint_max_bytes",
    },
    NativeCommand {
        name: "model",
        summary: "List Ollama models; set preferred by name, or by # (loads model as daemon active); --clear.",
        details:
            "Usage: pengine model                # list models\n       pengine model <name>         # set preferred by name\n       pengine model <#>            # set preferred + load in Ollama daemon\n       pengine model --clear        # clear preference (use daemon active)",
    },
    NativeCommand {
        name: "bot",
        summary: "Connect or disconnect the Telegram bot.",
        details:
            "Usage: pengine bot connect <token>\n       pengine bot disconnect\n\nVerifies the token, persists metadata to connection.json, and stores the\ntoken in the OS keychain. Tokens never reach disk.",
    },
    NativeCommand {
        name: "tools",
        summary: "List MCP tools (optional search substring).",
        details:
            "Usage: pengine tools                # list every connected MCP tool\n       pengine tools <substring>    # filter by name/server/description",
    },
    NativeCommand {
        name: "skills",
        summary: "List, enable, or disable skills.",
        details:
            "Usage: pengine skills                       # list\n       pengine skills enable <slug>         # enable\n       pengine skills disable <slug>        # disable",
    },
    NativeCommand {
        name: "fs",
        summary: "List, add, or remove MCP filesystem roots.",
        details:
            "Usage: pengine fs                    # list current roots\n       pengine fs add <path>         # add an absolute path\n       pengine fs remove <path>      # remove a root",
    },
    NativeCommand {
        name: "logs",
        summary: "Stream log events (--follow / --tail).",
        details:
            "Usage: pengine logs                  # tail last 50 audit lines\n       pengine logs --tail 200       # tail last N\n       pengine logs --follow         # stream live (REPL/CLI only; not Telegram)",
    },
    NativeCommand {
        name: "ask",
        summary: "Send a message to the agent (AI path).",
        details:
            "Usage: pengine ask \"<prompt>\"\n\nRuns one agent turn. In REPL, free text without a leading `/` is the same\npath. Prefix with /think or /nothink to override reasoning mode.\n\nFile mentions: tokens like @path/to/file are inlined (capped at 64 KB)\nbefore the prompt is sent.",
    },
    NativeCommand {
        name: "new",
        summary: "Start a new session (optional name). Saves the current session first.",
        details: "Usage: /new [name]\n\nShortcut for `/session new [name]`. Creates a fresh conversation\ncontext and optionally names it for later `/session switch`.",
    },
    NativeCommand {
        name: "retry",
        summary: "Re-run the last user message with a fresh agent turn.",
        details: "Usage: /retry\n\nRepeats the most recent user message. Useful when the model\ngave an unsatisfying answer and you want a second attempt.",
    },
    NativeCommand {
        name: "search",
        summary: "Case-insensitive search through the current session's turn history.",
        details: "Usage: /search <query>\n\nSearches user messages and assistant replies in the active\nsession. Returns matching snippets with turn numbers.",
    },
    NativeCommand {
        name: "compact",
        summary: "Summarize old session turns into a compact memory (REPL-only).",
        details: "Usage: /compact\n\n\
                  Calls the AI to summarize all turns beyond the recent-turn keep budget\n\
                  (last 6 turns are preserved verbatim). The resulting summary is prepended\n\
                  to future context so the AI retains key decisions without consuming the\n\
                  full prompt window. Compaction also runs automatically in the background\n\
                  when the session exceeds 12 turns.",
    },
    NativeCommand {
        name: "session",
        summary: "Manage named sessions: list, new, switch, rename (REPL-only).",
        details: "Usage: /session list                   # list all saved sessions\n       \
                  /session new [name]            # start a fresh session (optional name)\n       \
                  /session switch <name-or-id>   # resume a saved session\n       \
                  /session rename <name>          # name or rename the active session\n       \
                  /session delete <name-or-id>   # delete a saved session from disk\n       \
                  /session help                  # show all session subcommands\n\n\
                  Sessions persist across restarts. Each session keeps a turn history and\n\
                  a compacted summary for context. /session switch saves the current session\n\
                  before switching. Run /session help for the full subcommand list.",
    },
    NativeCommand {
        name: "clear",
        summary: "Clear the REPL screen (REPL-only).",
        details: "Usage: /clear  (REPL-only; same as Ctrl+L on most terminals)",
    },
    NativeCommand {
        name: "exit",
        summary: "Exit the REPL.",
        details: "Usage: /exit  (alias: /quit, exit, quit, Ctrl+D)",
    },
    NativeCommand {
        name: "quit",
        summary: "Exit the REPL.",
        details: "Usage: /quit  (alias: /exit, exit, quit, Ctrl+D)",
    },
];

pub fn lookup(name: &str) -> Option<&'static NativeCommand> {
    COMMANDS.iter().find(|c| c.name == name)
}
