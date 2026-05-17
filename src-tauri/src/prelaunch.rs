//! Pre-Tauri activation-policy hook (macOS).
//!
//! The bundled `.app` declares `LSUIElement=true` in `Info.plist`, which
//! starts every launch as `NSApplicationActivationPolicyAccessory` — no
//! Dock icon, no menu bar. That covers production users.
//!
//! Dev builds (`cargo run`, `cargo tauri dev`, `target/debug/pengine`) have
//! no `Info.plist` applied, so they would otherwise show a brief Dock-icon
//! flash before [`crate::modules::cli::bootstrap::handle_cli_or_continue`]
//! runs from inside Tauri's `setup` callback. We close that gap by reading
//! `argv` / `env` ourselves at the top of `lib::run()` and calling
//! `[NSApp setActivationPolicy:…]` before `tauri::Builder::default()`
//! initializes anything.
//!
//! ## Policy choice
//!
//! CLI invocations (all paths that exit without opening a window) use
//! `NSApplicationActivationPolicyProhibited`, which is the strongest
//! possible hide: the process does not appear in the Dock, App Switcher,
//! Stage Manager, Mission Control, or any other macOS UI surface.
//! `Prohibited` cannot be overridden once set, so even if Tauri's internal
//! NSApplication initialization briefly promotes to `Regular`, the policy
//! holds.
//!
//! GUI invocations (Finder double-click, `open -a`, stdin not a TTY) do
//! **not** call this function; they start with whatever policy `Info.plist`
//! declares (`Accessory` via `LSUIElement=true`) and are promoted to
//! `Regular` inside `bootstrap::handle_cli_or_continue`.
//!
//! This mirrors the CLI/GUI detection in `bootstrap::handle_cli_or_continue`
//! — but it can't import that code because it must run before `tauri::App`
//! exists. Keep the two in sync when CLI subcommands or env markers change.

use std::env;
use std::ffi::CString;
use std::io::IsTerminal;

use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

/// `true` when this process should not register a Dock icon — i.e. any CLI
/// invocation that exits without opening a window.
pub fn is_cli_invocation() -> bool {
    if env::var("PENGINE_OPEN_GUI")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        return false;
    }
    if env::var("PENGINE_LAUNCH_MODE")
        .map(|v| v == "cli")
        .unwrap_or(false)
    {
        return true;
    }

    let mut args = env::args().skip(1).filter(|a| {
        let t = a.trim();
        !t.is_empty() && !t.starts_with("-psn_")
    });
    let Some(first) = args.next() else {
        // No arguments: REPL when stdin is a TTY; otherwise treat as a GUI
        // launch (Finder / Dock / `open -a pengine`).
        return std::io::stdin().is_terminal();
    };

    matches!(
        first.as_str(),
        "--help"
            | "-h"
            | "help"
            | "--version"
            | "-V"
            | "version"
            | "--json"
            | "--continue"
            | "-p"
            | "--print"
            | "--output-format"
            | "--shell"
            | "status"
            | "clear"
            | "config"
            | "model"
            | "bot"
            | "tools"
            | "skills"
            | "fs"
            | "logs"
            | "ask"
            | "app"
            | "compact"
            | "new"
    )
}

/// Rename the process to `pengine-cli` so it is distinguishable from the
/// GUI process in Activity Monitor and `ps`. Uses the POSIX `setprogname(3)`
/// call available on macOS/BSD — changes the argv[0] name used by the OS.
pub fn rename_to_cli() {
    if let Ok(name) = CString::new("pengine-cli") {
        // SAFETY: `setprogname` only reads the pointer; the CString lives
        // for the rest of `main`, so the pointer stays valid.
        unsafe {
            extern "C" {
                fn setprogname(name: *const std::ffi::c_char);
            }
            setprogname(name.as_ptr());
        }
    }
}

/// Set NSApp to `Prohibited` before Tauri's run loop starts so the CLI
/// process is invisible to every macOS UI surface (Dock, App Switcher,
/// Stage Manager, Mission Control).
///
/// `Prohibited` cannot be upgraded to `Regular` later, which is exactly
/// right for CLI paths — they always end in `process::exit`. No-op when
/// not on the main thread (defensive — `lib::run()` is always main).
pub fn hide_dock_icon() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Prohibited);
}
