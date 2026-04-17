use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum SkillOrigin {
    /// Shipped with the app under `tools/skills/`. Read-only from the app.
    Bundled,
    /// Lives under `$APP_DATA/skills/`. Editable by the user.
    #[default]
    Custom,
}

/// A skill is a folder with a `SKILL.md` whose YAML frontmatter declares the
/// fields below. The markdown body after the frontmatter is passed to the agent
/// as context — see `doc/skills.md`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    pub slug: String,
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license: Option<String>,
    #[serde(default)]
    pub requires: Vec<String>,
    /// If `requires` lists `brave_web_search`, optional substrings (case-insensitive) that must
    /// appear in the user message before that tool is exposed — in addition to `tags` (length ≥4).
    #[serde(default)]
    pub brave_allow_substrings: Vec<String>,
    #[serde(default)]
    pub origin: SkillOrigin,
    /// Optional extra rules from `mandatory.md` next to `SKILL.md` (server-only; not serialized to clients).
    #[serde(skip)]
    pub mandatory_hint: Option<String>,
    /// Whether the agent should see this skill. Controlled per-slug in the UI.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Raw markdown body after the frontmatter block.
    pub body: String,
}

fn default_true() -> bool {
    true
}

/// One row returned by `GET /api/search?q=<term>` on ClawHub.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClawHubSkill {
    pub slug: String,
    #[serde(default)]
    pub display_name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
    /// Search relevance score from ClawHub (vector / hybrid ranking).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub score: Option<f64>,
    /// Filled when detail HTML is fetched (`/openclaw/{slug}`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_handle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub downloads: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stars: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installs_current: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installs_all_time: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comments_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_highlighted: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_official: Option<bool>,
}

/// Wrapper that matches the raw ClawHub `/api/search` response shape.
#[derive(Debug, Clone, Deserialize)]
pub struct ClawHubSearchResponse {
    #[serde(default)]
    pub results: Vec<ClawHubSkill>,
}

/// One plugin row from `GET https://clawhub.ai/api/v1/plugins`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClawHubPluginSummary {
    pub name: String,
    pub display_name: String,
    pub summary: String,
    #[serde(default)]
    pub owner_handle: String,
    #[serde(default)]
    pub capability_tags: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ClawHubPluginsResponse {
    #[serde(default)]
    pub items: Vec<ClawHubPluginSummary>,
    #[serde(default, rename = "nextCursor")]
    pub next_cursor: Option<String>,
}

/// One page from `GET /api/v1/plugins` (cursor-based; full catalog is tens of thousands of rows).
#[derive(Debug, Clone)]
pub struct ClawHubPluginsPage {
    pub items: Vec<ClawHubPluginSummary>,
    pub next_cursor: Option<String>,
}
