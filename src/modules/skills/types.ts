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
  origin: SkillOrigin;
  enabled: boolean;
  body: string;
};

/** One row from ClawHub's `/api/search?q=<term>` response. */
export type ClawHubSkill = {
  slug: string;
  displayName: string;
  summary: string;
  version?: string;
  updatedAt?: number;
};
