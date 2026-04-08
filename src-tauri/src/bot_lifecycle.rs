use crate::state::AppState;
use std::time::{Duration, Instant};

/// Max time to wait for `telegram_service::start_bot` to clear `bot_running`
/// after a shutdown notification.
pub const BOT_STOP_TIMEOUT: Duration = Duration::from_secs(30);

/// Notify the running dispatcher to stop and wait until `bot_running` is false
/// (or the timeout elapses).
pub async fn stop_and_wait_for_bot(state: &AppState) {
    let was_running = *state.bot_running.lock().await;
    if !was_running {
        return;
    }

    state.shutdown_notify.notify_waiters();
    state.emit_log("run", "Stopping existing bot…").await;

    let start = Instant::now();
    while start.elapsed() < BOT_STOP_TIMEOUT {
        if !*state.bot_running.lock().await {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    state
        .emit_log(
            "run",
            "Warning: bot still reports running after shutdown wait — proceeding anyway",
        )
        .await;
}
