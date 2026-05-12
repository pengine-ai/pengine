mod app;
mod build_info;
mod infrastructure;
pub mod modules;
#[cfg(target_os = "macos")]
mod prelaunch;
mod shared;

pub fn run() {
    #[cfg(target_os = "macos")]
    if prelaunch::is_cli_invocation() {
        prelaunch::rename_to_cli();
        prelaunch::hide_dock_icon();
    }
    app::run();
}
