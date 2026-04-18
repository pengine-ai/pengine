use crate::modules::bot::agent;
use crate::modules::cron::{repository as cron_repository, types::CronFile};
use crate::shared::state::AppState;
use crate::shared::text::split_by_chars;
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{ChatAction, ChatId, Me};
use teloxide::utils::command::BotCommands;
use tokio::sync::Notify;

/// Telegram's per-message hard limit is 4096 **UTF-16 code units**, not Unicode
/// scalars: one emoji outside the BMP counts as 2 code units. `split_by_chars`
/// counts Rust `char`s, so we halve the budget to stay safe even for messages
/// that are entirely supplementary characters. 2000 * 2 = 4000 UTF-16 units
/// leaves headroom under the 4096 limit.
const TELEGRAM_CHUNK_BUDGET: usize = 2000;

pub async fn verify_token(token: &str) -> Result<Me, String> {
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|e| format!("Telegram getMe failed: {e}"))
}

pub async fn start_bot(state: AppState, token: String, shutdown: Arc<Notify>) {
    let bot = Bot::new(&token);
    let state_clone = state.clone();

    state
        .emit_log("ok", "Telegram bot starting long poll…")
        .await;

    let handler = Update::filter_message()
        .branch(
            dptree::entry()
                .filter_command::<BotCommand>()
                .endpoint(command_handler),
        )
        .branch(dptree::endpoint(text_handler));

    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![state_clone])
        .enable_ctrlc_handler()
        .build();

    let shutdown_token = dispatcher.shutdown_token();

    tokio::spawn({
        let shutdown = shutdown.clone();
        async move {
            shutdown.notified().await;
            shutdown_token
                .shutdown()
                .expect("dispatcher shutdown")
                .await;
        }
    });

    {
        let mut running = state.bot_running.lock().await;
        *running = true;
    }
    state
        .emit_log("ok", "Telegram bot connected and polling")
        .await;

    dispatcher.dispatch().await;

    {
        let mut running = state.bot_running.lock().await;
        *running = false;
    }
    state.emit_log("ok", "Telegram bot stopped").await;
}

#[derive(BotCommands, Clone)]
#[command(rename_rule = "lowercase")]
enum BotCommand {
    #[command(description = "Start the bot")]
    Start,
    #[command(description = "Show help")]
    Help,
}

async fn command_handler(
    bot: Bot,
    msg: Message,
    cmd: BotCommand,
    state: AppState,
) -> ResponseResult<()> {
    let text = match cmd {
        BotCommand::Start => {
            state
                .emit_log("msg", &format!("/start from {}", user_label(&msg)))
                .await;
            "Howdy, how are you doing? Pengine is ready.".to_string()
        }
        BotCommand::Help => {
            state
                .emit_log("msg", &format!("/help from {}", user_label(&msg)))
                .await;
            "Send me any text message and I'll reply using your local Ollama model.".to_string()
        }
    };
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

async fn text_handler(bot: Bot, msg: Message, state: AppState) -> ResponseResult<()> {
    let incoming = msg.text().unwrap_or("<non-text>").to_string();
    state
        .emit_log("msg", &format!("from {}: {}", user_label(&msg), incoming))
        .await;

    remember_chat_id(&state, msg.chat.id).await;

    // Telegram's `typing` action lasts ~5 seconds. Refresh it every 4s in a
    // background task while the agent runs so the user sees a continuous
    // "writing…" indicator no matter how long the tool calls take.
    let typing_task = {
        let bot = bot.clone();
        let chat_id = msg.chat.id;
        tokio::spawn(async move {
            loop {
                let _ = bot.send_chat_action(chat_id, ChatAction::Typing).await;
                tokio::time::sleep(std::time::Duration::from_secs(4)).await;
            }
        })
    };

    let result = agent::run_turn(&state, &incoming).await;
    typing_task.abort();

    match result {
        Ok(turn) => {
            if turn.suppress_telegram_reply {
                state
                    .emit_log("reply", "[diary line saved; no Telegram reply]")
                    .await;
                return Ok(());
            }
            let reply = if turn.text.trim().is_empty() {
                "(no reply)".to_string()
            } else {
                turn.text
            };
            let tag = match turn.source {
                agent::ReplySource::Tool => "tool",
                agent::ReplySource::Model => "model",
            };
            state.emit_log("reply", &format!("[{tag}] {reply}")).await;
            send_reply(&bot, msg.chat.id, &reply, &state).await;
        }
        Err(e) => {
            state.emit_log("run", &format!("agent error: {e}")).await;
            send_inference_unavailable(&bot, &msg, &state).await;
        }
    }

    Ok(())
}

/// Persist the most recent chat id so the cron scheduler has somewhere to deliver
/// proactive messages. Writes `cron.json` only when the id changed.
async fn remember_chat_id(state: &AppState, chat_id: ChatId) {
    let new_id = chat_id.0;
    {
        let mut guard = state.last_chat_id.write().await;
        if *guard == Some(new_id) {
            return;
        }
        *guard = Some(new_id);
    }
    let snapshot = state.cron_jobs.read().await.clone();
    let file = CronFile {
        jobs: snapshot,
        last_chat_id: Some(new_id),
    };
    if let Err(e) = cron_repository::save(&state.cron_path, &file) {
        state
            .emit_log("cron", &format!("save last_chat_id failed: {e}"))
            .await;
    }
}

/// Send `text` to `chat_id`, splitting into Telegram-sized chunks if needed.
/// Send failures are logged (not propagated) so one bad chunk doesn't abort
/// the handler and leave the user with no reply at all.
async fn send_reply(bot: &Bot, chat_id: ChatId, text: &str, state: &AppState) {
    let chunks = split_by_chars(text, TELEGRAM_CHUNK_BUDGET);
    let total = chunks.len();
    for (i, chunk) in chunks.iter().enumerate() {
        if let Err(e) = bot.send_message(chat_id, chunk).await {
            state
                .emit_log(
                    "run",
                    &format!("telegram send failed (chunk {}/{}): {e}", i + 1, total),
                )
                .await;
            return;
        }
    }
}

async fn send_inference_unavailable(bot: &Bot, msg: &Message, state: &AppState) {
    const TEXT: &str = "Sorry, local inference is unavailable right now. Please try again later.";
    if let Err(e) = bot.send_message(msg.chat.id, TEXT).await {
        state
            .emit_log(
                "run",
                &format!("could not send inference-unavailable reply: {e}"),
            )
            .await;
    }
}

fn user_label(msg: &Message) -> String {
    msg.from
        .as_ref()
        .map(|u| {
            u.username
                .as_deref()
                .map(|n| format!("@{n}"))
                .unwrap_or_else(|| u.first_name.clone())
        })
        .unwrap_or_else(|| "unknown".to_string())
}
