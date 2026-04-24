use super::types::{
    ClawHubPluginsPage, ClawHubPluginsResponse, ClawHubSearchResponse, ClawHubSkill, Skill,
    SkillOrigin,
};
use futures::future::join_all;
use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::Duration;
use tokio::time::timeout;
use zip::ZipArchive;

const CLAWHUB_SEARCH_URL: &str = "https://clawhub.ai/api/search";
const CLAWHUB_PLUGINS_LIST_URL: &str = "https://clawhub.ai/api/v1/plugins";
const CLAWHUB_DOWNLOAD_URL: &str = "https://clawhub.ai/api/v1/download";
const CLAWHUB_OPENCLAW_PREFIX: &str = "https://clawhub.ai/openclaw";

const CLAWHUB_TIMEOUT: Duration = Duration::from_secs(10);
const CLAWHUB_OPENCLAW_FETCH_TIMEOUT: Duration = Duration::from_secs(5);

/// Max SKILL.md size we are willing to fetch/write (skills are small by design).
const MAX_SKILL_MD_BYTES: usize = 256 * 1024;
/// Max `mandatory.md` size (optional per-skill rules).
const MAX_MANDATORY_MD_BYTES: usize = 64 * 1024;
/// Max zip size to accept from ClawHub before extracting.
const MAX_ZIP_BYTES: usize = 1024 * 1024;

/// Disabled-slug registry lives next to the custom skills dir.
const DISABLED_FILE: &str = ".disabled.json";

/// Dashboard drag-and-drop order for the Skills list (also system-prompt hint order).
const SKILL_ORDER_FILE: &str = ".skill_order.json";

fn skill_order_path(store_path: &Path) -> PathBuf {
    custom_skills_dir(store_path).join(SKILL_ORDER_FILE)
}

fn read_skill_order_slugs(store_path: &Path) -> Vec<String> {
    let path = skill_order_path(store_path);
    let Ok(raw) = std::fs::read_to_string(&path) else {
        return Vec::new();
    };
    serde_json::from_str::<Vec<String>>(&raw).unwrap_or_default()
}

fn apply_user_skill_order(skills: &mut [Skill], store_path: &Path) {
    let order = read_skill_order_slugs(store_path);
    if order.is_empty() {
        return;
    }
    let order_index: HashMap<String, usize> = order
        .iter()
        .enumerate()
        .map(|(i, s)| (s.to_lowercase(), i))
        .collect();
    skills.sort_by(|a, b| {
        let ia = order_index.get(&a.slug.to_lowercase()).copied();
        let ib = order_index.get(&b.slug.to_lowercase()).copied();
        match (ia, ib) {
            (Some(i), Some(j)) => i.cmp(&j),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a.slug.cmp(&b.slug),
        }
    });
}

/// Bundled + custom skills, alphabetically sorted — does not apply `.skill_order.json`.
pub(crate) fn gather_skills_sorted(store_path: &Path) -> Vec<Skill> {
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

/// Persists dashboard order; merges skills missing from `requested` at the end (A–Z).
pub fn set_skill_slug_order(store_path: &Path, requested: &[String]) -> Result<(), String> {
    let skills = gather_skills_sorted(store_path);
    let by_lower: HashMap<String, String> = skills
        .iter()
        .map(|s| (s.slug.to_lowercase(), s.slug.clone()))
        .collect();
    let mut seen = HashSet::<String>::new();
    let mut out: Vec<String> = Vec::new();
    for r in requested {
        let t = r.trim();
        if t.is_empty() {
            continue;
        }
        let k = t.to_lowercase();
        if let Some(canon) = by_lower.get(&k) {
            if seen.insert(k) {
                out.push(canon.clone());
            }
        }
    }
    let mut missing: Vec<String> = skills
        .iter()
        .filter(|s| !seen.contains(&s.slug.to_lowercase()))
        .map(|s| s.slug.clone())
        .collect();
    missing.sort();
    out.extend(missing);

    let dir = custom_skills_dir(store_path);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create skills dir: {e}"))?;
    let json =
        serde_json::to_string_pretty(&out).map_err(|e| format!("encode skill order: {e}"))?;
    std::fs::write(skill_order_path(store_path), json)
        .map_err(|e| format!("write {}: {e}", SKILL_ORDER_FILE))
}

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

/// Per-skill body cap in the system-prompt hint. Keeps the prompt bounded so local
/// models re-read it cheaply on each turn. Skills needing more detail should
/// front-load the critical scope gate / URL / recipe at the top of `SKILL.md`.
pub const SKILL_HINT_BODY_CAP: usize = 2200;

/// Default cap for the full skills fragment (intro + bodies + mandatory snippets), aligned with
/// [`crate::shared::user_settings::DEFAULT_SKILLS_HINT_MAX_BYTES`]. The **runtime** limit is
/// [`crate::shared::state::AppState::skills_hint_max_bytes`] (see `modules::agent` turns).
pub const DEFAULT_SKILL_HINT_BYTES: usize =
    crate::shared::user_settings::DEFAULT_SKILLS_HINT_MAX_BYTES as usize;

/// `mandatory.md` is still high-signal but must not balloon the system prompt unchecked.
const SKILL_MANDATORY_HINT_CAP: usize = 1200;

const SKILL_HINT_INTRO: &str = "\n\nSkills: follow each recipe exactly — \
it lists WHICH URL and HOW MANY calls. Stop when you can answer; \
don't probe alternate hosts. Unless a skill’s **`mandatory.md`** says otherwise, prefer **`fetch`** whenever you have a concrete URL; use **`brave_web_search`** when the recipe lists it in `requires` (and this turn matches that skill), when **`mandatory.md`** orders it, or when the user explicitly asked to search the open web.\n\
**Weather, forecasts, temperature, precipitation:** use **skill:weather** (wttr.in / Open-Meteo) as the only recipe — never government-portal or “.gv.at” skills for those topics.\n\
Portal- or government-specific skills you install yourself apply **only** when the user is clearly asking about that jurisdiction’s government, law, official forms, or public administration — \
not for recipes, hobbies, general knowledge, software, weather, or unrelated chit-chat. If the topic does not match the skill’s scope, ignore that recipe entirely.";

/// True when the user (or cron) message is clearly about weather / forecast.
pub fn user_message_suggests_weather(user_message: &str) -> bool {
    const NEEDLES: &[&str] = &[
        "wetter",
        "weather",
        "forecast",
        "vorhersage",
        "regenwahrscheinlichkeit",
        "temperatur",
        "gewitter",
        "schnee",
        "hagel",
        "wind",
        "niederschlag",
        "wttr",
        "bewölkt",
        "bewoelkt",
        "regen",
        "luftdruck",
        "hitze",
        "kühl",
        "kuehl",
        "eisregen",
    ];
    NEEDLES
        .iter()
        .any(|n| user_message_needle_match(user_message, n))
}

/// Default “only when talking about AT public administration” needles for known portal skill slugs.
pub fn default_hint_needles_for_slug(slug: &str) -> Option<&'static [&'static str]> {
    let s = slug.to_lowercase();
    let portal = s == "austria-gv-data"
        || s == "austrian-gv"
        || s == "austrian-gv-data"
        || s.contains("austria-gv")
        || s.contains("austrian-gv")
        || s.contains("oesterreich-gv")
        || (s.contains("oesterreich") && s.contains("gv"))
        || (s.contains("austria") && s.contains("gv") && s.contains("data"));
    if !portal {
        return None;
    }
    Some(&[
        "oesterreich.gv",
        ".gv.at",
        "oesterreich",
        "bundesrecht",
        "verwaltung",
        "behörde",
        "behoerde",
        "formular",
        "bürgerservice",
        "buergerservice",
        "e-government",
        "egov",
        "ministerium",
        "amt",
        "landesregierung",
        "gemeinde",
        "bescheid",
        "verordnung",
        "österreich",
    ])
}

/// Whether `skill` may appear in the skills system-prompt fragment for this turn.
/// `cron_pins_skills` is true when the caller already restricted to an explicit slug list (cron).
fn skill_passes_hint_gate(
    skill: &Skill,
    user_message: Option<&str>,
    cron_pins_skills: bool,
) -> bool {
    if cron_pins_skills {
        return true;
    }
    if !skill.hint_allow_substrings.is_empty() {
        return match user_message {
            None => true,
            Some(m) if m.trim().is_empty() => true,
            Some(m) => skill
                .hint_allow_substrings
                .iter()
                .any(|n| user_message_needle_match(m, n)),
        };
    }
    if let Some(needles) = default_hint_needles_for_slug(&skill.slug) {
        return match user_message {
            None => true,
            Some(m) if m.trim().is_empty() => true,
            Some(m) => needles.iter().any(|n| user_message_needle_match(m, n)),
        };
    }
    true
}

/// Build a system-prompt fragment describing the enabled skills so the agent
/// knows when/how to invoke fetch tools for each. Returns `""` if there are
/// none enabled.
///
/// When `user_message` is [`Some`] and matches [`user_message_suggests_weather`], the
/// **weather** skill block is moved to the top so it survives aggressive byte caps
/// and is not buried after alphabetically earlier skills (e.g. government portals).
///
/// When `slug_filter` is [`Some`] and non-empty, only those enabled skills are included
/// (e.g. cron jobs with a pinned skill list).
/// Note: `slug_filter == Some(&[])` is intentionally treated like `None` (no filter); pass
/// `Some(non_empty_slice)` to pin skills or `None` to disable filtering.
pub fn skills_prompt_hint_for_turn(
    store_path: &Path,
    user_message: Option<&str>,
    slug_filter: Option<&[String]>,
) -> String {
    let mut skills: Vec<Skill> = list_skills(store_path)
        .into_iter()
        .filter(|s| s.enabled)
        .collect();
    let filtered_run = if let Some(want) = slug_filter {
        if want.is_empty() {
            false
        } else {
            let set: HashSet<String> = want.iter().map(|s| s.to_lowercase()).collect();
            skills.retain(|s| set.contains(&s.slug.to_lowercase()));
            let pos: HashMap<String, usize> = want
                .iter()
                .enumerate()
                .map(|(i, s)| (s.to_lowercase(), i))
                .collect();
            skills.sort_by_key(|s| {
                pos.get(&s.slug.to_lowercase())
                    .copied()
                    .unwrap_or(usize::MAX)
            });
            true
        }
    } else {
        false
    };
    if !filtered_run {
        skills.retain(|s| skill_passes_hint_gate(s, user_message, false));
    }
    if skills.is_empty() {
        return String::new();
    }
    if let Some(msg) = user_message {
        if !filtered_run && user_message_suggests_weather(msg) {
            let (mut w, rest): (Vec<Skill>, Vec<Skill>) = skills
                .into_iter()
                .partition(|s| s.slug.eq_ignore_ascii_case("weather"));
            w.sort_by(|a, b| a.slug.cmp(&b.slug));
            let mut rest = rest;
            rest.sort_by(|a, b| a.slug.cmp(&b.slug));
            skills = w.into_iter().chain(rest).collect();
        }
    }
    let mut out = String::from(SKILL_HINT_INTRO);
    if filtered_run {
        out.push_str("\n\n**(This scheduled run)** Use only the skills listed below; ignore other installed skills for this task.");
    }
    for s in &skills {
        let trimmed = s.body.trim();
        let body = truncate_for_prompt(trimmed, SKILL_HINT_BODY_CAP);
        out.push_str(&format!(
            "\n\n── skill:{slug} — {name} ──\n{desc}\n{body}",
            slug = s.slug,
            name = s.name,
            desc = s.description,
        ));
        if let Some(m) = &s.mandatory_markdown {
            let m = m.trim();
            if !m.is_empty() {
                let m = truncate_for_prompt(m, SKILL_MANDATORY_HINT_CAP);
                out.push_str("\n\n");
                out.push_str(&m);
            }
        }
    }
    out
}

/// Same as [`skills_prompt_hint_for_turn`] without per-turn ordering (tests, callers without context).
pub fn skills_prompt_hint(store_path: &Path) -> String {
    skills_prompt_hint_for_turn(store_path, None, None)
}

/// Deduplicate `requested` and keep only slugs that exist on disk (bundled or custom).
/// Preserves first-seen order using canonical slug spelling from on-disk skills.
pub fn canonicalize_skill_slug_list(store_path: &Path, requested: &[String]) -> Vec<String> {
    let by_lower: HashMap<String, String> = gather_skills_sorted(store_path)
        .into_iter()
        .map(|s| (s.slug.to_lowercase(), s.slug))
        .collect();
    let mut seen = HashSet::<String>::new();
    let mut out = Vec::new();
    for r in requested {
        let t = r.trim();
        if t.is_empty() {
            continue;
        }
        let key = t.to_lowercase();
        if let Some(canonical) = by_lower.get(&key).cloned() {
            if seen.insert(key) {
                out.push(canonical);
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
/// Order follows the Skills dashboard (`.skill_order.json` under the custom skills dir).
pub fn list_skills(store_path: &Path) -> Vec<Skill> {
    let mut v = gather_skills_sorted(store_path);
    apply_user_skill_order(&mut v, store_path);
    v
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
        let skill_md = path.join("SKILL.md");
        let Ok(raw) = std::fs::read_to_string(&skill_md) else {
            continue;
        };
        match parse_skill(&slug, &raw, origin) {
            Ok(mut s) => {
                let mandatory_path = path.join("mandatory.md");
                s.mandatory_markdown = std::fs::read_to_string(&mandatory_path)
                    .ok()
                    .map(|t| t.trim().to_string())
                    .filter(|t| !t.is_empty());
                skills.push(s);
            }
            Err(e) => log::warn!("skipping skill {}: {e}", skill_md.display()),
        }
    }
    skills
}

/// Parse a skill’s `SKILL.md`. Frontmatter is the `---`-delimited YAML-ish block at the top.
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
        brave_allow_substrings: fields.get_list("brave_allow_substrings"),
        hint_allow_substrings: fields.get_list("hint_allow_substrings"),
        origin,
        mandatory_markdown: None,
        enabled: true,
        body: body.trim_start_matches(['\n', '\r']).to_string(),
    })
}

/// Lowercase text with ä/ö/ü/ß folded to ASCII digraphs so `oesterreich` matches `Österreich`.
fn german_ascii_fold(lower: &str) -> String {
    let mut o = String::with_capacity(lower.len() + 4);
    for c in lower.chars() {
        match c {
            'ä' => o.push_str("ae"),
            'ö' => o.push_str("oe"),
            'ü' => o.push_str("ue"),
            'ß' => o.push_str("ss"),
            _ => o.push(c),
        }
    }
    o
}

fn alphanumeric_token_match(haystack_lower: &str, needle_lower: &str) -> bool {
    haystack_lower
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| !t.is_empty())
        .any(|t| t == needle_lower)
}

/// `needle` is already lowercased. Short needles use alphanumeric token equality (avoids `rss` ⊆
/// `progress`); longer needles match as substring, with a German-fold fallback path.
pub(crate) fn user_text_covers_token(
    user_lower: &str,
    user_folded: &str,
    needle_lower: &str,
) -> bool {
    if needle_lower.is_empty() {
        return false;
    }
    if needle_lower.len() <= 4 {
        let needle_folded = german_ascii_fold(needle_lower);
        return alphanumeric_token_match(user_lower, needle_lower)
            || (!needle_folded.is_empty()
                && alphanumeric_token_match(user_folded, &needle_folded));
    }
    if user_lower.contains(needle_lower) {
        return true;
    }
    let needle_folded = german_ascii_fold(needle_lower);
    user_folded.contains(&needle_folded)
}

/// Lowercase + fold helper for callers outside this module (e.g. MCP tool ranking).
pub(crate) fn user_message_needle_match(user_message: &str, needle: &str) -> bool {
    let needle_lower = needle.trim().to_lowercase();
    if needle_lower.is_empty() {
        return false;
    }
    let u = user_message.to_lowercase();
    let u_fold = german_ascii_fold(&u);
    user_text_covers_token(&u, &u_fold, &needle_lower)
}

/// Tags this generic must not alone enable billed web search (e.g. "news" ⊆ "gameinformer news").
pub(crate) const BRAVE_TAG_DENYLIST: &[&str] = &[
    "news", "info", "help", "guide", "tips", "blog", "home", "page", "data", "list", "links",
    "link", "tool", "tools", "apps", "app", "media", "site", "sites", "world", "daily", "live",
];

fn skill_triggers_brave_web_search(skill: &Skill, user_message: &str) -> bool {
    if !skill
        .requires
        .iter()
        .any(|r| r.eq_ignore_ascii_case("brave_web_search"))
    {
        return false;
    }
    if !skill_passes_hint_gate(skill, Some(user_message), false) {
        return false;
    }
    let u = user_message.to_lowercase();
    let u_fold = german_ascii_fold(&u);
    for sub in &skill.brave_allow_substrings {
        let sl = sub.to_lowercase();
        if sl.len() >= 3 && user_text_covers_token(&u, &u_fold, &sl) {
            return true;
        }
    }
    for t in &skill.tags {
        if t.len() < 6 {
            continue;
        }
        if BRAVE_TAG_DENYLIST.iter().any(|g| g.eq_ignore_ascii_case(t)) {
            continue;
        }
        let tl = t.to_lowercase();
        if user_text_covers_token(&u, &u_fold, &tl) {
            return true;
        }
    }
    false
}

/// Expose the billed `brave_web_search` tool when catalogued **search keywords** match
/// ([`super::keywords::brave_search_allowed_by_keywords`]) or when an enabled skill’s
/// `requires` / `brave_allow_substrings` / tags gate this turn.
pub fn allow_brave_web_search_for_message(store_path: &Path, user_message: &str) -> bool {
    if super::keywords::brave_search_allowed_by_keywords(user_message) {
        return true;
    }
    list_skills(store_path)
        .into_iter()
        .filter(|s| s.enabled)
        .any(|s| skill_triggers_brave_web_search(&s, user_message))
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

/// Create or overwrite a custom skill from its full `SKILL.md` markdown.
///
/// `mandatory_update`: `None` = leave `mandatory.md` unchanged; `Some("")` = remove the file if present;
/// `Some(text)` = write trimmed text (must be ≤ [`MAX_MANDATORY_MD_BYTES`]).
pub fn write_custom_skill(
    store_path: &Path,
    slug: &str,
    markdown: &str,
    mandatory_update: Option<&str>,
) -> Result<Skill, String> {
    validate_slug(slug)?;
    if markdown.len() > MAX_SKILL_MD_BYTES {
        return Err(format!(
            "SKILL.md exceeds {} byte limit",
            MAX_SKILL_MD_BYTES
        ));
    }

    let mut skill = parse_skill(slug, markdown, SkillOrigin::Custom)
        .map_err(|e| format!("invalid skill markdown: {e}"))?;

    let dir = custom_skills_dir(store_path).join(slug);
    std::fs::create_dir_all(&dir).map_err(|e| format!("create {}: {e}", dir.display()))?;
    let path = dir.join("SKILL.md");
    std::fs::write(&path, markdown).map_err(|e| format!("write {}: {e}", path.display()))?;

    let mandatory_path = dir.join("mandatory.md");
    if let Some(raw) = mandatory_update {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            let _ = std::fs::remove_file(&mandatory_path);
            skill.mandatory_markdown = None;
        } else if trimmed.len() > MAX_MANDATORY_MD_BYTES {
            return Err(format!(
                "mandatory.md exceeds {} byte limit",
                MAX_MANDATORY_MD_BYTES
            ));
        } else {
            std::fs::write(&mandatory_path, trimmed)
                .map_err(|e| format!("write {}: {e}", mandatory_path.display()))?;
            skill.mandatory_markdown = Some(trimmed.to_string());
        }
    } else {
        skill.mandatory_markdown = std::fs::read_to_string(&mandatory_path)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
    }

    Ok(skill)
}

/// Remove a custom skill's folder (including `SKILL.md` and optional `mandatory.md`). Bundled skills cannot be deleted.
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

static SKILL_STATS_RE: OnceLock<Regex> = OnceLock::new();
static OWNER_HANDLE_RE: OnceLock<Regex> = OnceLock::new();
static SEMVER_RE: OnceLock<Regex> = OnceLock::new();

fn skill_stats_re() -> &'static Regex {
    SKILL_STATS_RE
        .get_or_init(|| Regex::new(r"stats:\$R\[\d+\]=\{([^}]+)\}").expect("skill stats regex"))
}

fn owner_handle_re() -> &'static Regex {
    OWNER_HANDLE_RE.get_or_init(|| Regex::new(r#"handle:\"([^\"]+)\""#).expect("handle regex"))
}

fn semver_re() -> &'static Regex {
    SEMVER_RE.get_or_init(|| Regex::new(r#"version:\"(\d+\.\d+\.\d+)\""#).expect("semver regex"))
}

#[derive(Debug, Clone, Default)]
struct ParsedOpenclawSkillPage {
    owner_handle: Option<String>,
    downloads: Option<u64>,
    stars: Option<u64>,
    installs_current: Option<u64>,
    installs_all_time: Option<u64>,
    version_count: Option<u64>,
    comments_count: Option<u64>,
    version_semver: Option<String>,
    is_highlighted: bool,
    is_official: bool,
}

fn parse_stats_blob(blob: &str, out: &mut ParsedOpenclawSkillPage) {
    for part in blob.split(',') {
        let Some((k, v)) = part.split_once(':') else {
            continue;
        };
        let Ok(n) = v.trim().parse::<u64>() else {
            continue;
        };
        match k.trim() {
            "downloads" => out.downloads = Some(n),
            "stars" => out.stars = Some(n),
            "installsCurrent" => out.installs_current = Some(n),
            "installsAllTime" => out.installs_all_time = Some(n),
            "versions" => out.version_count = Some(n),
            "comments" => out.comments_count = Some(n),
            _ => {}
        }
    }
}

fn extract_owner_before_skill(html: &str) -> Option<String> {
    let idx = html.find("skill:$R")?;
    let prefix = &html[..idx];
    owner_handle_re()
        .captures_iter(prefix)
        .last()
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

fn parse_openclaw_skill_html(html: &str) -> ParsedOpenclawSkillPage {
    let mut out = ParsedOpenclawSkillPage::default();
    if let Some(cap) = skill_stats_re().captures(html) {
        if let Some(m) = cap.get(1) {
            parse_stats_blob(m.as_str(), &mut out);
        }
    }
    out.owner_handle = extract_owner_before_skill(html);
    if let Some(c) = semver_re().captures(html) {
        if let Some(m) = c.get(1) {
            out.version_semver = Some(m.as_str().to_string());
        }
    }
    out.is_highlighted = html.contains("highlighted:$R");
    out.is_official = html.contains("official:$R");
    out
}

fn merge_openclaw_parsed(skill: &mut ClawHubSkill, p: &ParsedOpenclawSkillPage) {
    if let Some(ref h) = p.owner_handle {
        skill.owner_handle = Some(h.clone());
    }
    skill.downloads = p.downloads.or(skill.downloads);
    skill.stars = p.stars.or(skill.stars);
    skill.installs_current = p.installs_current.or(skill.installs_current);
    skill.installs_all_time = p.installs_all_time.or(skill.installs_all_time);
    skill.version_count = p.version_count.or(skill.version_count);
    skill.comments_count = p.comments_count.or(skill.comments_count);
    if let Some(ref v) = p.version_semver {
        skill.version = Some(v.clone());
    }
    // Only set when the OpenClaw page embeds the badge; omitting the block is not "not highlighted".
    if p.is_highlighted {
        skill.is_highlighted = Some(true);
    }
    if p.is_official {
        skill.is_official = Some(true);
    }
}

async fn fetch_openclaw_skill_html(client: &reqwest::Client, slug: &str) -> Option<String> {
    let url = format!("{CLAWHUB_OPENCLAW_PREFIX}/{slug}");
    let Ok(Ok(resp)) = timeout(CLAWHUB_OPENCLAW_FETCH_TIMEOUT, client.get(&url).send()).await
    else {
        return None;
    };
    if !resp.status().is_success() {
        return None;
    }
    resp.text().await.ok()
}

async fn enrich_clawhub_skills_from_openclaw(
    client: &reqwest::Client,
    skills: &mut [ClawHubSkill],
) {
    let futs: Vec<_> = skills
        .iter()
        .map(|s| s.slug.clone())
        .map(|slug| {
            let client = client.clone();
            async move {
                let html = fetch_openclaw_skill_html(&client, &slug).await?;
                let parsed = parse_openclaw_skill_html(&html);
                Some((slug, parsed))
            }
        })
        .collect();
    let pairs: HashMap<String, ParsedOpenclawSkillPage> =
        join_all(futs).await.into_iter().flatten().collect();
    for sk in skills.iter_mut() {
        if let Some(p) = pairs.get(&sk.slug) {
            merge_openclaw_parsed(sk, p);
        }
    }
}

/// Options forwarded as query parameters to `GET /api/search` on ClawHub.
#[derive(Debug, Clone)]
pub struct ClawHubSearchOptions {
    pub highlighted: bool,
    pub non_suspicious: bool,
    pub staff_picks: bool,
    pub clean_only: bool,
    pub sort: Option<String>,
    pub limit: Option<u32>,
    pub tag: Option<String>,
    /// When true, fetch each skill’s public `/openclaw/{slug}` HTML for author + stats (slower).
    pub enrich: bool,
}

impl Default for ClawHubSearchOptions {
    fn default() -> Self {
        Self {
            highlighted: true,
            non_suspicious: true,
            staff_picks: false,
            clean_only: false,
            sort: None,
            limit: None,
            tag: None,
            enrich: true,
        }
    }
}

/// Search the ClawHub skill registry.
///
/// ClawHub returns an empty list for empty `q`, including with `staffPicks=true`.
/// For staff-picks browsing with no query, we seed with a broad single-letter
/// query so staff picks are still discoverable.
pub async fn search_clawhub(
    query: &str,
    opts: &ClawHubSearchOptions,
) -> Result<Vec<ClawHubSkill>, String> {
    let q = query.trim();
    let effective_q = if q.is_empty() && opts.staff_picks {
        "a"
    } else {
        q
    };
    if effective_q.is_empty() {
        return Ok(Vec::new());
    }
    let client = build_clawhub_client()?;
    let mut url = reqwest::Url::parse(CLAWHUB_SEARCH_URL)
        .map_err(|e| format!("parse ClawHub search URL: {e}"))?;
    {
        let lim = opts.limit.unwrap_or(30).clamp(1, 500);
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("q", effective_q);
        pairs.append_pair("limit", &lim.to_string());
        if let Some(ref s) = opts.sort {
            let s = s.trim();
            if !s.is_empty() {
                pairs.append_pair("sort", s);
            }
        }
        if opts.highlighted {
            pairs.append_pair("highlighted", "true");
        }
        if opts.non_suspicious {
            pairs.append_pair("nonSuspicious", "true");
        }
        if opts.staff_picks {
            pairs.append_pair("staffPicks", "true");
        }
        if opts.clean_only {
            pairs.append_pair("cleanOnly", "true");
        }
        if let Some(ref t) = opts.tag {
            let t = t.trim();
            if !t.is_empty() {
                pairs.append_pair("tag", t);
            }
        }
    }
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("search ClawHub: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub returned HTTP {}", resp.status()));
    }
    let mut body = resp
        .json::<ClawHubSearchResponse>()
        .await
        .map_err(|e| format!("parse ClawHub search: {e}"))?;
    if opts.enrich && !body.results.is_empty() {
        enrich_clawhub_skills_from_openclaw(&client, &mut body.results).await;
    }
    Ok(body.results)
}

/// Search ClawHub **plugins** (OpenClaw packages). Distinct from skills; install is not supported here.
/// Empty `query` lists the full catalog (paginate with `cursor` from the previous page).
pub async fn search_clawhub_plugins(
    query: &str,
    limit: Option<u32>,
    cursor: Option<&str>,
) -> Result<ClawHubPluginsPage, String> {
    let client = build_clawhub_client()?;
    let lim = limit.unwrap_or(30).clamp(1, 500);
    let mut url = reqwest::Url::parse(CLAWHUB_PLUGINS_LIST_URL)
        .map_err(|e| format!("parse ClawHub plugins URL: {e}"))?;
    {
        let q = query.trim();
        let mut pairs = url.query_pairs_mut();
        if !q.is_empty() {
            pairs.append_pair("search", q);
        }
        pairs.append_pair("limit", &lim.to_string());
        if let Some(c) = cursor {
            let c = c.trim();
            if !c.is_empty() {
                pairs.append_pair("cursor", c);
            }
        }
    }
    let resp = client
        .get(url)
        .send()
        .await
        .map_err(|e| format!("search ClawHub plugins: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("ClawHub plugins returned HTTP {}", resp.status()));
    }
    let body = resp
        .json::<ClawHubPluginsResponse>()
        .await
        .map_err(|e| format!("parse ClawHub plugins: {e}"))?;
    Ok(ClawHubPluginsPage {
        items: body.items,
        next_cursor: body.next_cursor.filter(|s| !s.trim().is_empty()),
    })
}

/// Install a ClawHub skill by downloading its zip, extracting `SKILL.md`,
/// and writing it under `$APP_DATA/skills/<slug>/SKILL.md`.
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
    write_custom_skill(store_path, slug, &markdown, None)
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
            if file.size() > MAX_SKILL_MD_BYTES as u64 {
                return Err(format!("{ctx}: exceeds {MAX_SKILL_MD_BYTES} byte limit"));
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
        assert!(write_custom_skill(&fake_store, "bad slug!", SAMPLE, None).is_err());
        assert!(write_custom_skill(&fake_store, "good-slug", SAMPLE, None).is_ok());
    }

    #[test]
    fn write_then_list_roundtrip() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE, None).unwrap();
        let list = list_skills(&fake_store);
        assert!(list.iter().any(|s| s.slug == "demo"));
    }

    #[test]
    fn disabled_flag_roundtrips() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE, None).unwrap();
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
        write_custom_skill(&fake_store, "demo", SAMPLE, None).unwrap();
        delete_custom_skill(&fake_store, "demo").unwrap();
        assert!(delete_custom_skill(&fake_store, "demo").is_err());
    }

    #[test]
    fn delete_clears_disabled_entry() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        write_custom_skill(&fake_store, "demo", SAMPLE, None).unwrap();
        set_skill_enabled(&fake_store, "demo", false).unwrap();
        delete_custom_skill(&fake_store, "demo").unwrap();
        assert!(!read_disabled_set(&fake_store).contains("demo"));
    }

    #[test]
    fn weather_skill_appends_mandatory_from_mandatory_md() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let weather_md = "---\nname: weather\ndescription: test\ntags: []\n---\n\n# x\n";
        let mandatory = "**MANDATORY for skill:weather:** use wttr.in; Open-Meteo retry with countryCode; see How to answer.\n";
        write_custom_skill(&fake_store, "weather", weather_md, Some(mandatory)).unwrap();
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

    #[test]
    fn mandatory_cleared_when_saved_as_empty() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let md = "---\nname: x\ndescription: d\ntags: []\n---\n\nbody\n";
        write_custom_skill(&fake_store, "x", md, Some("keep me")).unwrap();
        let path = custom_skills_dir(&fake_store)
            .join("x")
            .join("mandatory.md");
        assert!(path.is_file());
        write_custom_skill(&fake_store, "x", md, Some("")).unwrap();
        assert!(!path.exists());
        let list = list_skills(&fake_store);
        let s = list.iter().find(|s| s.slug == "x").unwrap();
        assert!(s.mandatory_markdown.is_none());
    }

    #[test]
    fn mandatory_unchanged_when_update_is_none() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let md = "---\nname: y\ndescription: d\ntags: []\n---\n\nbody\n";
        write_custom_skill(&fake_store, "y", md, Some("first")).unwrap();
        let md2 = "---\nname: y\ndescription: d2\ntags: []\n---\n\nbody2\n";
        write_custom_skill(&fake_store, "y", md2, None).unwrap();
        let list = list_skills(&fake_store);
        let s = list.iter().find(|s| s.slug == "y").unwrap();
        assert_eq!(s.mandatory_markdown.as_deref(), Some("first"));
    }

    #[test]
    fn user_message_suggests_weather_german_and_not_admin() {
        assert!(user_message_suggests_weather(
            "wie ist das wetter in der Breitenau"
        ));
        assert!(!user_message_suggests_weather(
            "Formular auf oesterreich.gv.at runterladen"
        ));
    }

    #[test]
    fn skills_hint_orders_weather_before_alphabetically_earlier_slugs() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let other = "---\nname: AAA\ndescription: o\ntags: []\n---\n\naaa\n";
        let weather_md = "---\nname: weather\ndescription: w\ntags: []\n---\n\nww\n";
        write_custom_skill(&fake_store, "arch-other", other, None).unwrap();
        write_custom_skill(&fake_store, "weather", weather_md, None).unwrap();
        let hint = skills_prompt_hint_for_turn(&fake_store, Some("Wetter in Wien"), None);
        let pos_w = hint.find("── skill:weather").expect("weather block");
        let pos_a = hint.find("── skill:arch-other").expect("arch-other block");
        assert!(
            pos_w < pos_a,
            "weather should precede alphabetically earlier slugs:\n{hint}"
        );
    }

    #[test]
    fn skills_hint_respects_slug_filter() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let a = "---\nname: A\ndescription: a\ntags: []\n---\n\nAAA\n";
        let b = "---\nname: B\ndescription: b\ntags: []\n---\n\nBBB\n";
        write_custom_skill(&fake_store, "skill-a", a, None).unwrap();
        write_custom_skill(&fake_store, "skill-b", b, None).unwrap();
        let filter = vec!["skill-b".to_string()];
        let hint = skills_prompt_hint_for_turn(&fake_store, None, Some(&filter));
        assert!(hint.contains("BBB"), "hint={hint}");
        assert!(!hint.contains("AAA"), "hint={hint}");
        assert!(
            hint.contains("This scheduled run"),
            "expected cron banner when filtered:\n{hint}"
        );
    }

    #[test]
    fn canonicalize_skill_slug_list_dedupes_and_matches_case() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let md = "---\nname: Z\ndescription: d\ntags: []\n---\n\nz\n";
        write_custom_skill(&fake_store, "my_skill", md, None).unwrap();
        let out = canonicalize_skill_slug_list(
            &fake_store,
            &["MY_SKILL".into(), "nope".into(), "my_skill".into()],
        );
        assert_eq!(out, vec!["my_skill"]);
    }

    #[test]
    fn portal_skill_hint_gated_without_admin_keywords() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let gv = "---\nname: G\ndescription: d\ntags: []\n---\n\nGVONLY\n";
        write_custom_skill(&fake_store, "austria-gv-data", gv, None).unwrap();
        let hint = skills_prompt_hint_for_turn(&fake_store, Some("wie ist das wetter"), None);
        assert!(!hint.contains("GVONLY"), "hint={hint}");
        let hint2 =
            skills_prompt_hint_for_turn(&fake_store, Some("Formular auf oesterreich.gv.at"), None);
        assert!(hint2.contains("GVONLY"), "hint={hint2}");
    }

    #[test]
    fn cron_slug_filter_includes_portal_skill_without_keywords() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let gv = "---\nname: G\ndescription: d\ntags: []\n---\n\nGVONLY\n";
        write_custom_skill(&fake_store, "austria-gv-data", gv, None).unwrap();
        let f = vec!["austria-gv-data".to_string()];
        let hint = skills_prompt_hint_for_turn(&fake_store, Some("weather only"), Some(&f));
        assert!(hint.contains("GVONLY"), "hint={hint}");
    }

    #[test]
    fn skills_hint_follows_slug_filter_order() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let a = "---\nname: A\ndescription: d\ntags: []\n---\n\nFIRST\n";
        let b = "---\nname: B\ndescription: d\ntags: []\n---\n\nSECOND\n";
        write_custom_skill(&fake_store, "skill-a", a, None).unwrap();
        write_custom_skill(&fake_store, "skill-b", b, None).unwrap();
        let order = vec!["skill-b".into(), "skill-a".into()];
        let hint = skills_prompt_hint_for_turn(&fake_store, None, Some(&order));
        let p_first = hint.find("FIRST").expect("FIRST");
        let p_second = hint.find("SECOND").expect("SECOND");
        assert!(
            p_second < p_first,
            "expected skill-b (SECOND) before skill-a (FIRST):\n{hint}"
        );
    }

    #[test]
    fn hint_allow_substrings_in_frontmatter_gates_skill() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let md = "---\nname: X\ndescription: d\ntags: []\nhint_allow_substrings: [zebra]\n---\n\nZ_BODY\n";
        write_custom_skill(&fake_store, "gated-x", md, None).unwrap();
        assert!(!skills_prompt_hint_for_turn(&fake_store, Some("hello"), None).contains("Z_BODY"));
        assert!(
            skills_prompt_hint_for_turn(&fake_store, Some("zebra facts"), None).contains("Z_BODY")
        );
    }

    #[test]
    fn skill_order_file_changes_list_order() {
        let tmp = tempdir().unwrap();
        let fake_store = tmp.path().join("connection.json");
        let a = "---\nname: A\ndescription: d\ntags: []\n---\n\na\n";
        let b = "---\nname: B\ndescription: d\ntags: []\n---\n\nb\n";
        write_custom_skill(&fake_store, "skill-a", a, None).unwrap();
        write_custom_skill(&fake_store, "skill-b", b, None).unwrap();
        let alpha = list_skills(&fake_store);
        assert_eq!(alpha[0].slug, "skill-a");
        set_skill_slug_order(&fake_store, &["skill-b".into(), "skill-a".into()]).unwrap();
        let re = list_skills(&fake_store);
        assert_eq!(re[0].slug, "skill-b");
        assert_eq!(re[1].slug, "skill-a");
    }
}
