//! Compile-time app version (root `package.json`, set in `build.rs`) and git commit.

pub const APP_VERSION: &str = env!("PENGINE_APP_VERSION");
pub const GIT_COMMIT: &str = env!("PENGINE_GIT_COMMIT");
