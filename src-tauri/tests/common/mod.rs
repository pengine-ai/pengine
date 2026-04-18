//! Shared integration-test setup: avoid real OS credential stores (see `secure_store::mock_store`).

// `#[ctor]` runs before `main` in an unspecified order relative to other ctors; `set_var` is not
// documented as thread-safe, so this assumes single-threaded ctor execution.
#[ctor::ctor]
fn enable_mock_keychain() {
    if std::env::var_os("PENGINE_MOCK_KEYCHAIN").is_none() {
        std::env::set_var("PENGINE_MOCK_KEYCHAIN", "1");
    }
}
