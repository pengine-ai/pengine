use crate::state::AppState;
use teloxide::prelude::*;
use teloxide::types::Me;
use teloxide::utils::command::BotCommands;
use tokio::sync::Notify;
use std::sync::Arc;

pub async fn verify_token(token: &str) -> Result<Me, String> {
    let bot = Bot::new(token);
    bot.get_me()
        .await
        .map_err(|e| format!("Telegram getMe failed: {e}"))
}

pub async fn start_bot(state: AppState, token: String, shutdown: Arc<Notify>) {
    let bot = Bot::new(&token);
    let state_clone = state.clone();

    state.emit_log("ok", "Telegram bot starting long poll…").await;

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
            shutdown_token.shutdown().expect("dispatcher shutdown").await;
        }
    });

    {
        let mut running = state.bot_running.lock().await;
        *running = true;
    }
    state.emit_log("ok", "Telegram bot connected and polling").await;

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
            state.emit_log("msg", &format!("/start from {}", user_label(&msg))).await;
            "Howdy, how are you doing? Pengine is ready.".to_string()
        }
        BotCommand::Help => {
            state.emit_log("msg", &format!("/help from {}", user_label(&msg))).await;
            "Send me any message and I'll echo it back. Pengine POC.".to_string()
        }
    };
    bot.send_message(msg.chat.id, text).await?;
    Ok(())
}

async fn text_handler(_bot: Bot, msg: Message, state: AppState) -> ResponseResult<()> {
    let incoming = msg.text().unwrap_or("<non-text>");
    state
        .emit_log("msg", &format!("from {}: {}", user_label(&msg), incoming))
        .await;
    Ok(())
}

#[allow(dead_code)]
async fn try_ollama(prompt: &str) -> Result<String, String> {
    let payload = serde_json::json!({
        "model": "llama3.2",
        "messages": [{"role": "user", "content": format!("Think fast and answer extremely short. If you don't know the answer, say you don't know. Question: {prompt}")}],
        "stream": false,
    });

    let resp = reqwest::Client::new()
        .post("http://localhost:11434/api/chat")
        .json(&payload)
        .timeout(std::time::Duration::from_secs(120))
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    body["message"]["content"]
        .as_str()
        .map(|s| s.to_string())
        .ok_or_else(|| "unexpected Ollama response shape".to_string())
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
