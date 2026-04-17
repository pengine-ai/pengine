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

/// A skill is a folder with a `README.md` whose YAML frontmatter declares the
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
    #[serde(default)]
    pub origin: SkillOrigin,
    /// Optional extra rules from `mandatory.md` next to README (server-only; not serialized to clients).
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
/// The extra fields ClawHub returns (e.g. `score`) are ignored.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawHubSkill {
    pub slug: String,
    #[serde(default, rename = "displayName")]
    pub display_name: String,
    #[serde(default)]
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, rename = "updatedAt", skip_serializing_if = "Option::is_none")]
    pub updated_at: Option<u64>,
}

/// Wrapper that matches the raw ClawHub `/api/search` response shape.
#[derive(Debug, Clone, Deserialize)]
pub struct ClawHubSearchResponse {
    #[serde(default)]
    pub results: Vec<ClawHubSkill>,
}
