//! Pengine CLI — transport-agnostic command surface.
//!
//! Implements GitHub issue #90. The CLI is the same binary as the Tauri
//! desktop app, branched via `tauri-plugin-cli`'s `app.cli().matches()`.
//!
//! Invariants:
//! - Native commands (registered in [`commands`]) are never visible to the
//!   agent. The router dispatches them synchronously and returns a
//!   [`output::CliReply`] without calling [`crate::modules::agent::run_turn`].
//! - Unknown slash commands surface as [`router::RouterOutcome::Unknown`] —
//!   an error to the user's sink, never forwarded to the model.
//! - Handlers call existing module services (bot, mcp, skills, ollama,
//!   user_settings) — no duplicated business logic.
//! - Telegram `$` bridge (`telegram_bridge`) reuses [`dispatch`] + [`router`].

pub mod banner;
pub mod bootstrap;
pub mod commands;
pub mod dispatch;
pub mod flavor;
pub mod folder_trust;
pub mod handlers;
pub mod mentions;
pub mod output;
pub mod repl;
pub mod router;
pub mod session;
pub mod shim;
pub mod telegram_bridge;
