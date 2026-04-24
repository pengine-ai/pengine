//! Extract HTTP(S) URLs from `brave_web_search` tool text and cap how many the host prefetches.
//!
//! The model often answers from SERP snippets alone; one search per message is kept for cost
//! control, but we still attach `fetch` tool output for the top distinct result URLs so the
//! next model step can reason over page text.

use serde_json::Value;
use std::collections::HashSet;

/// Maximum number of distinct URLs the host will `fetch` after a single `brave_web_search`.
pub const DEFAULT_AUTO_FETCH_CAP: usize = 5;

fn trim_url_trailing_junk(mut s: String) -> String {
    while let Some(c) = s.chars().last() {
        if matches!(c, ')' | ']' | '>' | '.' | ',' | ';' | '"' | '\'') {
            s.pop();
        } else {
            break;
        }
    }
    s
}

fn should_skip_url(url: &str) -> bool {
    let u = url.to_lowercase();
    // Brave / tracker noise sometimes appears in raw payloads.
    u.contains("cdn.search.brave")
        || u.contains("brave.com/static")
        || u.starts_with("mailto:")
        || u.starts_with("tel:")
}

/// Hosts we skip for host-prefetch after web search — social / aggregators rarely carry the article body.
const SOCIAL_OR_PORTAL_HOST_MARKERS: &[&str] = &[
    "facebook.com",
    "instagram.com",
    "twitter.com",
    "x.com",
    "tiktok.com",
    "reddit.com",
    "linkedin.com",
    "pinterest.com",
    "wikipedia.org",
];

fn url_host_for_policy(url: &str) -> Option<String> {
    let t = url.trim();
    let rest = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))?;
    let host_end = rest
        .find(|c| ['/', '?', '#'].contains(&c))
        .unwrap_or(rest.len());
    let host = rest.get(..host_end)?;
    if host.is_empty() {
        return None;
    }
    Some(host.to_lowercase())
}

fn host_is_social_or_wikipedia(host: &str) -> bool {
    let h = host.to_lowercase();
    SOCIAL_OR_PORTAL_HOST_MARKERS
        .iter()
        .any(|m| h == *m || h.ends_with(&format!(".{m}")) || h.contains(&format!(".{m}")))
}

fn host_should_skip_auto_fetch(url: &str) -> bool {
    url_host_for_policy(url)
        .map(|h| host_is_social_or_wikipedia(&h))
        .unwrap_or(true)
}

/// Brave JSON: preserve `web.results[].url` order (not a full-tree walk, which reorders URLs).
fn ordered_brave_result_urls(v: &Value) -> Vec<String> {
    let Some(web) = v.get("web") else {
        return Vec::new();
    };
    let Some(results) = web.get("results").and_then(|x| x.as_array()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for r in results {
        let Some(s) = r.get("url").and_then(|x| x.as_str()) else {
            continue;
        };
        let u = trim_url_trailing_junk(s.to_string());
        if looks_like_http_url(&u) && !should_skip_url(&u) && !host_should_skip_auto_fetch(&u) {
            out.push(u);
        }
    }
    out
}

fn registrable_core_host(host: &str) -> String {
    let h = host.trim().to_lowercase();
    let parts: Vec<&str> = h.split('.').collect();
    if parts.len() >= 2 {
        format!("{}.{}", parts[parts.len() - 2], parts[parts.len() - 1])
    } else {
        h
    }
}

/// Prefer URLs on the same site as the first organic hit, then other allowed URLs.
fn prioritize_same_site_first(urls: &[String]) -> Vec<String> {
    if urls.is_empty() {
        return Vec::new();
    }
    let Some(first_h) = urls.first().and_then(|u| url_host_for_policy(u)) else {
        return urls.to_vec();
    };
    let core = registrable_core_host(&first_h);
    let mut same = Vec::new();
    let mut other = Vec::new();
    for u in urls {
        if host_should_skip_auto_fetch(u) {
            continue;
        }
        if let Some(h) = url_host_for_policy(u) {
            if registrable_core_host(&h) == core {
                same.push(u.clone());
            } else {
                other.push(u.clone());
            }
        }
    }
    same.extend(other);
    same
}

fn looks_like_http_url(s: &str) -> bool {
    let t = s.trim();
    if !(t.starts_with("http://") || t.starts_with("https://")) || t.len() < 12 {
        return false;
    }
    let Some(rest) = t
        .strip_prefix("https://")
        .or_else(|| t.strip_prefix("http://"))
    else {
        return false;
    };
    let host_end = rest
        .find(|c| ['/', '?', '#'].contains(&c))
        .unwrap_or(rest.len());
    let host = &rest[..host_end];
    !host.is_empty() && (host.contains('.') || host == "localhost")
}

fn collect_urls_from_json(v: &Value, acc: &mut Vec<String>) {
    match v {
        Value::Object(map) => {
            for (k, val) in map {
                if k.eq_ignore_ascii_case("url") || k.eq_ignore_ascii_case("link") {
                    if let Some(s) = val.as_str() {
                        if looks_like_http_url(s) && !should_skip_url(s) {
                            acc.push(trim_url_trailing_junk(s.to_string()));
                        }
                    }
                }
                collect_urls_from_json(val, acc);
            }
        }
        Value::Array(a) => {
            for x in a {
                collect_urls_from_json(x, acc);
            }
        }
        _ => {}
    }
}

fn collect_urls_regex(text: &str, acc: &mut Vec<String>) {
    // Broad but conservative: stop at common delimiters after the path.
    let re = regex::Regex::new(r#"https?://[^\s\]`"'<>)\]]+"#).expect("valid regex");
    for m in re.find_iter(text) {
        let u = trim_url_trailing_junk(m.as_str().to_string());
        if looks_like_http_url(&u) && !should_skip_url(&u) {
            acc.push(u);
        }
    }
}

/// Ordered, deduplicated HTTP(S) URLs suitable for follow-up `fetch` calls.
pub fn extract_fetchable_urls(search_output: &str, max: usize) -> Vec<String> {
    let trimmed = search_output.trim();
    let json_val = serde_json::from_str::<Value>(trimmed).ok();

    let mut raw: Vec<String> = Vec::new();
    if let Some(ref v) = json_val {
        let ordered = ordered_brave_result_urls(v);
        if !ordered.is_empty() {
            raw.extend(prioritize_same_site_first(&ordered));
        }
    }
    if raw.is_empty() {
        if let Some(ref v) = json_val {
            collect_urls_from_json(v, &mut raw);
        }
        collect_urls_regex(trimmed, &mut raw);
    }

    let mut seen = HashSet::new();
    let mut out = Vec::new();
    for u in raw {
        let key = u.trim().to_string();
        if key.is_empty()
            || !looks_like_http_url(&key)
            || should_skip_url(&key)
            || host_should_skip_auto_fetch(&key)
            || !seen.insert(key.clone())
        {
            continue;
        }
        out.push(key);
        if out.len() >= max {
            break;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_urls_from_brave_style_json() {
        let j = r#"{"web":{"results":[
          {"url":"https://www.oesterreich.gv.at/de/a","title":"A"},
          {"url":"https://www.oesterreich.gv.at/de/b","title":"B"}
        ]}}"#;
        let u = extract_fetchable_urls(j, 10);
        assert_eq!(u.len(), 2);
        assert!(u[0].contains("oesterreich.gv.at"));
    }

    #[test]
    fn dedupes_and_caps() {
        let j = r#"{"url":"https://example.com/x"}
        https://example.com/x
        https://other.test/y"#;
        let u = extract_fetchable_urls(j, 2);
        assert_eq!(u.len(), 2);
        assert!(u.iter().any(|s| s.contains("example.com")));
        assert!(u.iter().any(|s| s.contains("other.test")));
    }

    #[test]
    fn brave_results_skip_social_and_keep_news_site() {
        let j = r#"{"web":{"results":[
          {"url":"https://www.facebook.com/officialgameinformer/"},
          {"url":"https://www.gameinformer.com/news"},
          {"url":"https://en.wikipedia.org/wiki/Game_Informer"}
        ]}}"#;
        let u = extract_fetchable_urls(j, 5);
        assert_eq!(u.len(), 1);
        assert!(u[0].contains("gameinformer.com"));
    }
}
