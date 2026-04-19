//! Cloud model discovery from ollama.com.
//!
//! The local Ollama daemon's `/api/tags` only lists models that have been
//! pulled to disk. Cloud models (accessible after `ollama signin`) live in
//! the upstream catalog at `https://ollama.com/library/<slug>` and use
//! `:cloud` or `<size>-cloud` tags. This module enumerates them by scraping
//! the cloud category page and each model's detail page, then caches the
//! result so the dashboard picker can show them without re-fetching every
//! few seconds.

use regex::Regex;
use std::sync::OnceLock;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;

const CLOUD_SEARCH_URL: &str = "https://ollama.com/search?c=cloud";
const CLOUD_LIBRARY_PREFIX: &str = "https://ollama.com/library/";
/// Stale cloud catalog is fine — Ollama publishes new cloud tags rarely. One
/// hour keeps the dashboard responsive and avoids hammering ollama.com.
const CACHE_TTL: Duration = Duration::from_secs(60 * 60);
const SEARCH_TIMEOUT: Duration = Duration::from_secs(5);
const DETAIL_TIMEOUT: Duration = Duration::from_secs(4);

struct CacheEntry {
    fetched_at: Instant,
    models: Vec<String>,
}

static CACHE: OnceLock<Mutex<Option<CacheEntry>>> = OnceLock::new();
static SLUG_RE: OnceLock<Regex> = OnceLock::new();
static RUN_RE: OnceLock<Regex> = OnceLock::new();

fn cache() -> &'static Mutex<Option<CacheEntry>> {
    CACHE.get_or_init(|| Mutex::new(None))
}

fn slug_re() -> &'static Regex {
    SLUG_RE.get_or_init(|| Regex::new(r#"href="/library/([a-z0-9._-]+)""#).unwrap())
}

fn run_re() -> &'static Regex {
    RUN_RE.get_or_init(|| {
        Regex::new(r#"ollama\s+(?:run|pull)\s+([a-z0-9._-]+(?::[a-z0-9._-]+)?)"#).unwrap()
    })
}

/// Returns cloud-tagged model names (e.g. `glm-4.6:cloud`,
/// `qwen3-coder:480b-cloud`). Falls back to a stale cache, then to an empty
/// list, when the upstream catalog is unreachable.
pub async fn list_cloud_models() -> Vec<String> {
    {
        let guard = cache().lock().await;
        if let Some(ref entry) = *guard {
            if entry.fetched_at.elapsed() < CACHE_TTL {
                return entry.models.clone();
            }
        }
    }
    match fetch_cloud_models().await {
        Ok(models) => {
            let mut guard = cache().lock().await;
            *guard = Some(CacheEntry {
                fetched_at: Instant::now(),
                models: models.clone(),
            });
            models
        }
        Err(e) => {
            log::warn!("ollama cloud catalog fetch failed: {e}");
            cache()
                .lock()
                .await
                .as_ref()
                .map(|c| c.models.clone())
                .unwrap_or_default()
        }
    }
}

async fn fetch_cloud_models() -> Result<Vec<String>, String> {
    let client = reqwest::Client::builder()
        .timeout(DETAIL_TIMEOUT)
        .user_agent("pengine/1.0")
        .build()
        .map_err(|e| e.to_string())?;
    let body = client
        .get(CLOUD_SEARCH_URL)
        .timeout(SEARCH_TIMEOUT)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let mut slugs: Vec<String> = slug_re()
        .captures_iter(&body)
        .map(|c| c[1].to_string())
        .collect();
    slugs.sort();
    slugs.dedup();
    if slugs.is_empty() {
        return Err("no cloud slugs found in /search?c=cloud".to_string());
    }

    let mut tasks = Vec::with_capacity(slugs.len());
    for slug in slugs {
        let client = client.clone();
        tasks.push(tokio::spawn(async move {
            cloud_models_for_slug(&client, &slug).await
        }));
    }
    let mut out: Vec<String> = Vec::new();
    for t in tasks {
        if let Ok(Ok(names)) = t.await {
            out.extend(names);
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

async fn cloud_models_for_slug(
    client: &reqwest::Client,
    slug: &str,
) -> Result<Vec<String>, String> {
    let url = format!("{CLOUD_LIBRARY_PREFIX}{slug}");
    let body = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for cap in run_re().captures_iter(&body) {
        let name = &cap[1];
        let tag = name.split_once(':').map(|(_, t)| t).unwrap_or("");
        if tag == "cloud" || tag.ends_with("-cloud") {
            out.push(name.to_string());
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}
