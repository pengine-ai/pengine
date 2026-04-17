use super::types::{ClawHubSearchResponse, ClawHubSkill, Skill, SkillOrigin};
use std::collections::HashSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::time::Duration;
use zip::ZipArchive;

const CLAWHUB_SEARCH_URL: &str = "https://clawhub.ai/api/search";
const CLAWHUB_DOWNLOAD_URL: &str = "https://clawhub.ai/api/v1/download";

const CLAWHUB_TIMEOUT: Duration = Duration::from_secs(10);

/// Max README size we are willing to fetch/write (skills are small by design).
const MAX_README_BYTES: usize = 256 * 1024;
/// Max zip size to accept from ClawHub before extracting.
const MAX_ZIP_BYTES: usize = 1024 * 1024;

/// Disabled-slug registry lives next to the custom skills dir.
const DISABLED_FILE: &str = ".disabled.json";

/// `$APP_DATA/skills/`. Created on demand.
pub fn custom_skills_dir(store_path: &Path) -> PathBuf {
    store_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("skills")
}

fn disabled_file_path(store_path: &Path) -> PathBuf {
    custom_skills_dir(store_path).join(DISABLED_FILE)
}

/// Walk up from `CARGO_MANIFEST_DIR` and `current_dir` looking for `tools/skills/`.
/// Mirrors the lookup in `tool_engine::service` so `tauri dev` finds the bundled
/// folder regardless of where the binary is launched from.
pub fn bundled_skills_dir() -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = Vec::new();
    candidates.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../tools/skills"));
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            candidates.push(dir.join("tools/skills"));
        }
    }
    if let Ok(mut cwd) = std::env::current_dir() {
        for _ in 0..8 {
            candidates.push(cwd.join("tools/skills"));
            if !cwd.pop() {
                break;
            }
        }
    }
    candidates.into_iter().find(|p| p.is_dir())
}

fn read_disabled_set(store_path: &Path) -> HashSet<String> {
    let path = disabled_file_path(store_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return HashSet::new();
    };
    match serde_json::from_str::<Vec<String>>(&raw) {
        Ok(v) => v.into_iter().collect(),
        Err(e) => {
            log::warn!(
                "invalid JSON in {} — treating as no disabled skills: {e}",
                path.display()
            );
            HashSet::new()
        }
    }
}

fn write_disabled_set(store_path: &Path, set: &HashSet<String>) -> Result<(), String> {
    let dir = custom_skills_dir(store_path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let mut list: Vec<&String> = set.iter().collect();
    list.sort();
    let json = serde_json::to_string_pretty(&list).map_err(|e| format!("encode disabled: {e}"))?;
    let path = disabled_file_path(store_path);
    std::fs::write(&path, json).map_err(|e| format!("write {}: {e}", path.display()))
}

/// Mark `slug` enabled or disabled. Persisted in `.disabled.json`.
pub fn set_skill_enabled(store_path: &Path, slug: &str, enabled: bool) -> Result<(), String> {
    validate_slug(slug)?;
    let mut set = read_disabled_set(store_path);
    if enabled {
        set.remove(slug);
    } else {
        set.insert(slug.to_string());
    }
    write_disabled_set(store_path, &set)
}

/// Per-skill body cap in the system-prompt hint. Keeps the prompt short so local
/// models re-read it cheaply on each turn. Skills needing more detail should
/// front-load the critical URL/recipe in the first ~1600 chars; use `mandatory.md` for rules that must not truncate away.
pub const SKILL_HINT_BODY_CAP: usize = 1600;

/// Hard cap for the full skills fragment (intro + every enabled skill body + mandatory snippets).
pub const MAX_TOTAL_SKILL_HINT_BYTES: usize = SKILL_HINT_BODY_CAP * 8;

const SKILL_HINT_INTRO: &str =
    "\n\nAvailable skills. Follow each skill's recipe exactly — it tells you \
WHICH URL to fetch and HOW MANY calls to make. Stop once you can answer; do not \
hop to unrelated hosts or invent alternate pages. If a skill orders retries on \
the same documented API (e.g. shorter geocode query + countryCode), do those \
steps — they are part of the recipe, not forbidden probing. \
Never claim you lack access — the fetch tool is available.";

/// Build a system-prompt fragment describing the enabled skills so the agent
/// knows when/how to invoke fetch tools for each. Returns `""` if there are
/// none enabled.
pub fn skills_prompt_hint(store_path: &Path) -> String {
    let skills: Vec<Skill> = list_skills(store_path)
        .into_iter()
        .filter(|s| s.enabled)
        .collect();
    if skills.is_empty() {
        return String::new();
    }
    let mut out = String::from(SKILL_HINT_INTRO);
    for s in &skills {
        let trimmed = s.body.trim();
        let body = truncate_for_prompt(trimmed, SKILL_HINT_BODY_CAP);
        out.push_str(&format!(
            "\n\n── skill:{slug} — {name} ──\n{desc}\n{body}",
            slug = s.slug,
            name = s.name,
            desc = s.description,
        ));
        if let Some(m) = &s.mandatory_hint {
            let m = m.trim();
            if !m.is_empty() {
                out.push_str("\n\n");
                out.push_str(m);
            }
        }
    }
    out
}

/// If the skills hint exceeds `max` bytes, truncate with the same rules as per-skill bodies.
pub fn limit_skills_hint_bytes(s: String, max: usize) -> (String, bool) {
    if s.len() <= max {
        return (s, false);
    }
    (truncate_for_prompt(&s, max), true)
}

/// Truncate on a char boundary and append an ellipsis marker if we cut.
fn truncate_for_prompt(s: &str, max: usize) -> String {
    if s.len() <= max {
        return s.to_string();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    // Prefer the last newline before `end` so we don't cut mid-line / mid-fence.
    if let Some(nl) = s[..end].rfind('\n') {
        if nl > max / 2 {
            end = nl;
        }
    }
    format!("{}\n…", &s[..end])
}

/// List every discoverable skill. Custom skills shadow bundled ones with the same slug.
pub fn list_skills(store_path: &Path) -> Vec<Skill> {
    let disabled = read_disabled_set(store_path);
    let mut out: Vec<Skill> = Vec::new();

    if let Some(dir) = bundled_skills_dir() {
        out.extend(read_dir_skills(&dir, SkillOrigin::Bundled));
    }

    let custom = custom_skills_dir(store_path);
    if custom.is_dir() {
        for skill in read_dir_skills(&custom, SkillOrigin::Custom) {
            if let Some(i) = out.iter().position(|s| s.slug == skill.slug) {
                out.remove(i);
            }
            out.push(skill);
        }
    }

    for skill in &mut out {
        skill.enabled = !disabled.contains(&skill.slug);
    }

    out.sort_by(|a, b| a.slug.cmp(&b.slug));
    out
}

fn read_dir_skills(dir: &Path, origin: SkillOrigin) -> Vec<Skill> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skills = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(slug) = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(str::to_string)
        else {
            continue;
        };
        if slug.starts_with('.') {
            continue;
        }
        let readme = path.join("README.md");
        let raw = match std::fs::read_to_string(&readme) {
            Ok(s) => s,
            Err(_) => match std::fs::read_to_string(path.join("SKILL.md")) {
                Ok(s) => s,
                Err(_) => continue,
            },
        };
        match parse_skill(&slug, &raw, origin) {
            Ok(mut s) => {
                let mandatory_path = path.join("mandatory.md");
                s.mandatory_hint = std::fs::read_to_string(&mandatory_path)
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                skills.push(s);
            }
            Err(e) => log::warn!("skipping skill {}: {e}", readme.display()),
        }
    }
    skills
}

/// Parse a skill README. Frontmatter is the `---`-delimited YAML-ish block at the top.
/// The parser is deliberately tiny — scalars, quoted strings, and inline `[a, b]` arrays only.
pub fn parse_skill(slug: &str, raw: &str, origin: SkillOrigin) -> Result<Skill, String> {
    let (fm, body) = split_frontmatter(raw).ok_or("missing frontmatter block")?;
    let fields = parse_frontmatter(fm)?;

    let name = fields
        .get("name")
        .cloned()
        .unwrap_or_else(|| slug.to_string());
    let description = fields
        .get("description")
        .cloned()
        .ok_or("frontmatter: missing `description`")?;

    // ClawHub skills use `homepage` where our local format uses `source`.
    let source = fields
        .get("source")
        .or_else(|| fields.get("homepage"))
        .cloned();

    Ok(Skill {
        slug: slug.to_string(),
        name,
        description,
        tags: fields.get_list("tags"),
        author: fields.get("author").cloned(),
        version: fields.get("version").cloned(),
        source,
        license: fields.get("license").cloned(),
        requires: fields.get_list("requires"),
        origin,
        mandatory_hint: None,
        enabled: true,
        body: body.trim_start_matches(['\n', '\r']).to_string(),
    })
}

fn split_frontmatter(raw: &str) -> Option<(&str, &str)> {
    let trimmed = raw.trim_start_matches('\u{feff}');
    let rest = trimmed.strip_prefix("---")?;
    let rest = rest
        .strip_prefix('\n')
        .or_else(|| rest.strip_prefix("\r\n"))?;
    let end = rest.find("\n---")?;
    let fm = &rest[..end];
    let after = &rest[end + 4..];
    let after = after
        .strip_prefix('\n')
        .or_else(|| after.strip_prefix("\r\n"))
        .unwrap_or(after);
    Some((fm, after))
}

/// Case-sensitive key→value/list field bag.
#[derive(Default)]
struct Fields {
    scalars: std::collections::HashMap<String, String>,
    lists: std::collections::HashMap<String, Vec<String>>,
}

impl Fields {
    fn get(&self, key: &str) -> Option<&String> {
        self.scalars.get(key)
    }
    fn get_list(&self, key: &str) -> Vec<String> {
        self.lists.get(key).cloned().unwrap_or_default()
    }
}

fn parse_frontmatter(fm: &str) -> Result<Fields, String> {
    let mut fields = Fields::default();
    for (lineno, line) in fm.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (key, value) = trimmed
            .split_once(':')
            .ok_or_else(|| format!("frontmatter line {}: missing ':'", lineno + 1))?;
        let key = key.trim().to_string();
        let value = value.trim();
        if key.is_empty() {
            return Err(format!("frontmatter line {}: empty key", lineno + 1));
        }
        if let Some(list) = parse_inline_list(value) {
            fields.lists.insert(key, list);
        } else {
            fields.scalars.insert(key, unquote(value).to_string());
        }
    }
    Ok(fields)
}

fn parse_inline_list(v: &str) -> Option<Vec<String>> {
    let inner = v.strip_prefix('[')?.strip_suffix(']')?;
    Some(
        inner
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(|s| unquote(s).to_string())
            .collect(),
    )
}

fn unquote(s: &str) -> &str {
    s.strip_prefix('"')
        .and_then(|rest| rest.strip_suffix('"'))
        .or_else(|| {
            s.strip_prefix('\'')
                .and_then(|rest| rest.strip_suffix('\''))
        })
        .unwrap_or(s)
}

/// Slugs must be filesystem-safe and URL-safe.
fn validate_slug(slug: &str) -> Result<(), String> {
    if slug.is_empty() || slug.len() > 64 {
        return Err("slug must be 1–64 chars".into());
    }
    if !slug
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    {
        return Err("slug may only contain a-z, 0-9, '-', '_'".into());
    }
    Ok(())
}

/// Create or overwrite a custom skill from its full README markdown.
pub fn write_custom_skill(store_path: &Path, slug: &str, markdown: &str) -> Result<Skill, String> {
    validate_slug(slug)?;
    if markdown.len() > MAX_README_BYTES {
        return Err(format!("README exceeds {} byte limit", MAX_README_BYTES));
    }

    let skill = parse_skill(slug, markdown, SkillOrigin::Custom)
        .map_err(|e| format!("invalid skill markdown: {e}"))?;

    let dir = custom_skills_dir(store_path).join(slug);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join("README.md");
    std::fs::write(&path, markdown).map_err(|e| format!("write {}: {e}", path.display()))?;
    Ok(skill)
}

/// Remove a custom skill's folder. Bundled skills cannot be deleted.
pub fn delete_custom_skill(store_path: &Path, slug: &str) -> Result<(), String> {
    validate_slug(slug)?;
    let dir = custom_skills_dir(store_path).join(slug);
    if !dir.exists() {
        return Err(format!("custom skill '{slug}' not found"));
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("remove {}: {e}", dir.display()))?;

    // Clean stale disabled-entry if present.
    let mut set = read_disabled_set(store_path);
    if set.remove(slug) {
        let _ = write_disabled_set(store_path, &set);
    }
    Ok(())
}

fn build_clawhub_client() -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(CLAWHUB_TIMEOUT)
        .user_agent("pengine-skills/1.0")
        .build()
        .map_err(|e| format!("http client: {e}"))
}

/// Search the ClawHub registry. An empty `query` returns an empty list; the
/// registry has no "list all" endpoint.
pub async fn search_clawhub(query: &str) -> Result<Vec<ClawHubSkill>, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(Vec::new());
    }
    let client = build_clawhub_client()?;
    let url = reqwest::Url::parse_with_params(CLAWHUB_SEARCH_URL, &[("q", q)])
        .map_err(|e| format!("build ClawHub search URL: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("search ClawHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub returned HTTP {}", resp.status()));
    }
    let body = resp
        .json::<ClawHubSearchResponse>()
        .await
        .map_err(|e| format!("parse ClawHub search: {e}"))?;
    Ok(body.results)
}

/// Install a ClawHub skill by downloading its zip, extracting `SKILL.md`,
/// and writing it under `$APP_DATA/skills/<slug>/README.md`.
pub async fn install_clawhub_skill(store_path: &Path, slug: &str) -> Result<Skill, String> {
    validate_slug(slug)?;
    let client = build_clawhub_client()?;
    let url = reqwest::Url::parse_with_params(CLAWHUB_DOWNLOAD_URL, &[("slug", slug)])
        .map_err(|e| format!("build ClawHub download URL: {e}"))?;
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("download ClawHub skill: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub download returned HTTP {}", resp.status()));
    }
    let bytes = resp
        .bytes()
        .await
        .map_err(|e| format!("read download body: {e}"))?;
    if bytes.len() > MAX_ZIP_BYTES {
        return Err(format!(
            "ClawHub archive exceeds {MAX_ZIP_BYTES} byte limit"
        ));
    }

    let markdown = extract_skill_md(bytes.as_ref())?;
    write_custom_skill(store_path, slug, &markdown)
}

/// Find the first `SKILL.md` in `zip_bytes` and return it as a UTF-8 string.
fn extract_skill_md(zip_bytes: &[u8]) -> Result<String, String> {
    let mut archive =
        ZipArchive::new(Cursor::new(zip_bytes)).map_err(|e| format!("invalid zip archive: {e}"))?;

    let entry_count = archive.len();
    for i in 0..entry_count {
        let mut file = archive
            .by_index(i)
            .map_err(|e| format!("zip open entry index {i} (of {entry_count}): {e}"))?;
        let name = file.name().to_string();
        let basename = name.rsplit('/').next().unwrap_or(&name);
        if basename.eq_ignore_ascii_case("SKILL.md") {
            let ctx = format!(
                "SKILL.md (zip index {i}, path {name:?}, compression {:?}, encrypted {})",
                file.compression(),
                file.encrypted()
            );
            if file.size() > MAX_README_BYTES as u64 {
                return Err(format!("{ctx}: exceeds {MAX_README_BYTES} byte limit"));
            }
            let mut buf = String::new();
            file.read_to_string(&mut buf).map_err(|e| {
                format!(
                    "{ctx}: read/decompress failed — {e}. \
This build only supports deflate/store zip entries; re-export the archive or enable the matching `zip` crate feature."
                )
            })?;
            return Ok(buf);
        }
    }
    Err("archive does not contain SKILL.md".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const SAMPLE: &str = "---\nname: demo\ndescription: A demo skill.\ntags: [a, b]\nrequires: [curl]\n---\n\n# body\n";

    #[test]
    fn parses_minimal_frontmatter() {
        let s = parse_skill("demo", SAMPLE, SkillOrigin::Custom).unwrap();
        assert_eq!(s.name, "demo");
        assert_eq!(s.description, "A demo skill.");
        assert_eq!(s.tags, vec!["a", "b"]);
        assert_eq!(s.requires, vec!["curl"]);
        assert!(s.enabled);
        assert!(s.body.starts_with("# body"));
    }

    #[test]
    fn rejects_missing_description() {
        let raw = "---\nname: demo\n---\nbody\n";
        let err = parse_skill("demo", raw, SkillOrigin::Custom).unwrap_err();
        assert!(err.contains("description"), "got: {err}");
    }

    #[test]
    fn accepts_clawhub_homepage_as_source() {
        let raw = "---\nname: weather\ndescription: d\nhomepage: https://wttr.in\n---\nbody\n";
        let s = parse_skill("weather", raw, SkillOrigin::Custom).unwrap();
        assert_eq!(s.source.as_deref(), Some("https://wttr.in"));
    }

    #[test]
    fn rejects_bad_slug() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        assert!(write_custom_skill(&fake_store, "bad slug!", SAMPLE).is_err());
        assert!(write_custom_skill(&fake_store, "good-slug", SAMPLE).is_ok());
    }

    #[test]
    fn write_then_list_roundtrip() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE).unwrap();
        let list = list_skills(&fake_store);
        assert!(list.iter().any(|s| s.slug == "demo"));
    }

    #[test]
    fn disabled_flag_roundtrips() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE).unwrap();
        set_skill_enabled(&fake_store, "demo", false).unwrap();

        let list = list_skills(&fake_store);
        let s = list.iter().find(|s| s.slug == "demo").unwrap();
        assert!(!s.enabled);

        set_skill_enabled(&fake_store, "demo", true).unwrap();
        let list = list_skills(&fake_store);
        let s = list.iter().find(|s| s.slug == "demo").unwrap();
        assert!(s.enabled);
    }

    #[test]
    fn delete_removes_custom_skill() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE).unwrap();
        delete_custom_skill(&fake_store, "demo").unwrap();
        assert!(delete_custom_skill(&fake_store, "demo").is_err());
    }

    #[test]
    fn delete_clears_disabled_entry() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE).unwrap();
        set_skill_enabled(&fake_store, "demo", false).unwrap();
        delete_custom_skill(&fake_store, "demo").unwrap();
        assert!(!read_disabled_set(&fake_store).contains("demo"));
    }

    #[test]
    fn weather_skill_appends_mandatory_from_mandatory_md() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let weather_md = "---\nname: weather\ndescription: test\ntags: []\n---\n\n# x\n";
        write_custom_skill(&fake_store, "weather", weather_md).unwrap();
        let mandatory = "**MANDATORY for skill:weather:** use wttr.in; Open-Meteo retry with countryCode; see How to answer.\n";
        std::fs::write(
            custom_skills_dir(&fake_store)
                .join("weather")
                .join("mandatory.md"),
            mandatory,
        )
        .unwrap();
        let hint = skills_prompt_hint(&fake_store);
        assert!(
            hint.contains("MANDATORY for skill:weather"),
            "expected mandatory block in:\n{hint}"
        );
        assert!(hint.contains("wttr.in"), "expected wttr in:\n{hint}");
        assert!(hint.contains("countryCode"), "hint={hint}");
        assert!(
            hint.contains("How to answer"),
            "expected answer-style reminder in:\n{hint}"
        );
    }
}
