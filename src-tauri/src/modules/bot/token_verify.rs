//! Minimal Telegram API calls used before the dispatcher is running.
//! Lives outside [`super::service`] so `modules::cli::handlers` can verify
//! tokens without creating a `bot -> cli -> handlers -> bot` dependency cycle.

use teloxide::prelude::*;
use teloxide::types::Me;

pub async fn verify_token(token: &str) -> Result<Me, String> {
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|e| format!("Telegram getMe failed: {e}"))
}
