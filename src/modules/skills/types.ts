export type SkillOrigin = "bundled" | "custom";

export type Skill = {
  slug: string;
  name: string;
  description: string;
  tags: string[];
  author?: string;
  version?: string;
  source?: string;
  license?: string;
  requires: string[];
  /** If set, skill block only injected when the message matches a substring (cron pins ignore). */
  hint_allow_substrings?: string[];
  origin: SkillOrigin;
  enabled: boolean;
  body: string;
  /** From optional `mandatory.md` next to SKILL.md (custom skills). */
  mandatory_markdown?: string | null;
  /** Same as `mandatory_markdown` when the API uses camelCase. */
  mandatoryMarkdown?: string | null;
};

/** Resolved optional `mandatory.md` text from a skill row (GET /v1/skills). */
export function skillMandatoryMarkdown(skill: Skill): string {
  const v = skill.mandatory_markdown ?? skill.mandatoryMarkdown;
  return typeof v === "string" ? v : "";
}

/** One row from ClawHub's `/api/search?q=<term>` response. */
export type ClawHubSkill = {
  slug: string;
  displayName: string;
  summary: string;
  version?: string;
  updatedAt?: number;
  /** Search relevance score from ClawHub. */
  score?: number;
  /** From `/openclaw/{slug}` HTML when enrich is enabled. */
  ownerHandle?: string;
  downloads?: number;
  stars?: number;
  installsCurrent?: number;
  installsAllTime?: number;
  versionCount?: number;
  commentsCount?: number;
  isHighlighted?: boolean;
  isOfficial?: boolean;
};

/** One row from ClawHub's plugin directory (`/api/v1/plugins`). */
export type ClawHubPlugin = {
  name: string;
  displayName: string;
  summary: string;
  ownerHandle: string;
  capabilityTags: string[];
};
