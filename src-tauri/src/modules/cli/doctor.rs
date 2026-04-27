//! `pengine doctor` — probes each subsystem and prints a checklist.
//!
//! Adapter only: every check delegates to existing services. The handler in
//! [`super::handlers::doctor`] formats the report.

use crate::modules::mcp::service as mcp_service;
use crate::modules::ollama::service as ollama;
use crate::modules::secure_store;
use crate::shared::state::AppState;
use std::time::Duration;

#[derive(Debug, Clone)]
pub enum Status {
    Ok,
    Warn,
    Fail,
}

impl Status {
    fn tag(&self) -> &'static str {
        match self {
            Status::Ok => "[ok]",
            Status::Warn => "[warn]",
            Status::Fail => "[fail]",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Check {
    pub name: &'static str,
    pub status: Status,
    pub detail: String,
}

pub async fn run(state: &AppState) -> Vec<Check> {
    let mut out = Vec::new();
    out.push(check_store_writable(state).await);
    out.push(check_ollama_reachable().await);
    out.push(check_active_model().await);
    out.push(check_mcp(state).await);
    out.push(check_keychain(state).await);
    out.push(check_network().await);
    out
}

async fn check_store_writable(state: &AppState) -> Check {
    let parent = state.store_path.parent();
    let Some(p) = parent else {
        return Check {
            name: "store",
            status: Status::Fail,
            detail: "no parent directory".into(),
        };
    };
    let probe = p.join(".pengine_doctor_probe");
    match std::fs::write(&probe, b"ok") {
        Ok(()) => {
            let _ = std::fs::remove_file(&probe);
            Check {
                name: "store",
                status: Status::Ok,
                detail: p.display().to_string(),
            }
        }
        Err(e) => Check {
            name: "store",
            status: Status::Fail,
            detail: format!("{}: {e}", p.display()),
        },
    }
}

async fn check_ollama_reachable() -> Check {
    match tokio::time::timeout(Duration::from_millis(2000), ollama::active_model()).await {
        Ok(Ok(m)) => Check {
            name: "ollama",
            status: Status::Ok,
            detail: format!("daemon up; active={m}"),
        },
        Ok(Err(e)) => Check {
            name: "ollama",
            status: Status::Fail,
            detail: format!("{e} — is `ollama serve` running?"),
        },
        Err(_) => Check {
            name: "ollama",
            status: Status::Fail,
            detail: "timed out after 2s".into(),
        },
    }
}

async fn check_active_model() -> Check {
    match tokio::time::timeout(Duration::from_millis(3000), ollama::model_catalog(2500)).await {
        Ok(Ok(c)) => {
            let n = c.models.len();
            if n == 0 {
                Check {
                    name: "models",
                    status: Status::Warn,
                    detail: "no models installed (pull one via `ollama pull <name>`)".into(),
                }
            } else {
                Check {
                    name: "models",
                    status: Status::Ok,
                    detail: format!("{n} model(s) available"),
                }
            }
        }
        Ok(Err(e)) => Check {
            name: "models",
            status: Status::Warn,
            detail: format!("could not list catalog: {e}"),
        },
        Err(_) => Check {
            name: "models",
            status: Status::Warn,
            detail: "model catalog timed out".into(),
        },
    }
}

async fn check_mcp(state: &AppState) -> Check {
    match mcp_service::rebuild_registry_into_state(state).await {
        Ok(()) => {
            let n = state.mcp.read().await.tool_names().len();
            if n == 0 {
                Check {
                    name: "mcp",
                    status: Status::Warn,
                    detail: "no tools registered (Dashboard → MCP Tools)".into(),
                }
            } else {
                Check {
                    name: "mcp",
                    status: Status::Ok,
                    detail: format!("{n} tool(s) connected"),
                }
            }
        }
        Err(e) => Check {
            name: "mcp",
            status: Status::Fail,
            detail: format!("registry rebuild failed: {e}"),
        },
    }
}

async fn check_keychain(state: &AppState) -> Check {
    let bot_id = state
        .connection
        .lock()
        .await
        .as_ref()
        .map(|c| c.bot_id.clone());
    let Some(id) = bot_id else {
        return Check {
            name: "keychain",
            status: Status::Ok,
            detail: "no bot connected (skipped)".into(),
        };
    };
    match secure_store::load_token(&id) {
        Ok(t) if !t.is_empty() => Check {
            name: "keychain",
            status: Status::Ok,
            detail: format!("token present for bot {id}"),
        },
        Ok(_) => Check {
            name: "keychain",
            status: Status::Warn,
            detail: format!("entry empty for bot {id} — reconnect with `pengine bot connect`"),
        },
        Err(e) => Check {
            name: "keychain",
            status: Status::Fail,
            detail: format!("{e}"),
        },
    }
}

async fn check_network() -> Check {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(2500))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            return Check {
                name: "network",
                status: Status::Warn,
                detail: format!("reqwest: {e}"),
            }
        }
    };
    // Generic outbound HTTPS probe — Cloudflare is widely reachable and
    // returns quickly. We don't probe ollama.com here because that would
    // conflate "no internet" with "Ollama Cloud product unreachable".
    match client.head("https://1.1.1.1/").send().await {
        Ok(_) => Check {
            name: "network",
            status: Status::Ok,
            detail: "outbound https reachable".into(),
        },
        Err(e) if e.is_timeout() => Check {
            name: "network",
            status: Status::Warn,
            detail: "outbound https timeout (offline?)".into(),
        },
        Err(e) => Check {
            name: "network",
            status: Status::Warn,
            detail: format!("outbound https: {e}"),
        },
    }
}

pub fn format_report(checks: &[Check]) -> String {
    let name_w = checks.iter().map(|c| c.name.len()).max().unwrap_or(6);
    let mut out = String::new();
    for c in checks {
        out.push_str(&format!(
            "  {:<6}  {:<name_w$}  {}\n",
            c.status.tag(),
            c.name,
            c.detail,
            name_w = name_w
        ));
    }
    out.trim_end().to_string()
}
