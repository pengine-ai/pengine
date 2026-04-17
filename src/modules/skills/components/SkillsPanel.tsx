import * as Accordion from "@radix-ui/react-accordion";
import { useCallback, useEffect, useRef, useState } from "react";
import { addSkill, deleteSkill, fetchSkills, setSkillEnabled } from "../api";
import type { Skill } from "../types";
import { ClawHubBrowse } from "./ClawHubBrowse";

const TEMPLATE = `---
name: my-skill
description: One-line summary of what this skill does.
tags: []
---

# My Skill

## Request

\`\`\`bash
curl -s "https://example.com/api/..."
\`\`\`

## Response schema

\`\`\`json
{}
\`\`\`

## When to use

- ...
`;

function yamlString(value: string): string {
  return `"${value.replace(/\\/g, "\\\\").replace(/"/g, '\\"')}"`;
}

function toInlineYamlArray(values: string[]): string {
  if (values.length === 0) return "[]";
  return `[${values.map(yamlString).join(", ")}]`;
}

function composeSkillMarkdown(skill: Skill): string {
  const lines: string[] = [
    "---",
    `name: ${yamlString(skill.name)}`,
    `description: ${yamlString(skill.description)}`,
    `tags: ${toInlineYamlArray(skill.tags)}`,
    `requires: ${toInlineYamlArray(skill.requires)}`,
  ];
  if (skill.author?.trim()) lines.push(`author: ${yamlString(skill.author.trim())}`);
  if (skill.version?.trim()) lines.push(`version: ${yamlString(skill.version.trim())}`);
  if (skill.source?.trim()) lines.push(`source: ${yamlString(skill.source.trim())}`);
  if (skill.license?.trim()) lines.push(`license: ${yamlString(skill.license.trim())}`);
  lines.push("---", "", skill.body.trim());
  return `${lines.join("\n")}\n`;
}

export function SkillsPanel() {
  const [skills, setSkills] = useState<Skill[] | null>(null);
  const [customDir, setCustomDir] = useState<string>("");
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [togglingSlug, setTogglingSlug] = useState<string | null>(null);

  const [showAdd, setShowAdd] = useState(false);
  const [newSlug, setNewSlug] = useState("");
  const [newMarkdown, setNewMarkdown] = useState(TEMPLATE);
  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const [addBusy, setAddBusy] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);

  const [showBrowse, setShowBrowse] = useState(false);
  const [browseKey, setBrowseKey] = useState(0);

  const cancelledRef = useRef(false);

  const load = useCallback(async () => {
    const resp = await fetchSkills();
    if (cancelledRef.current) return;
    setLoading(false);
    if (resp) {
      setSkills(resp.skills);
      setCustomDir(resp.custom_dir);
      setError(null);
    } else {
      setError("Could not load skills");
    }
  }, []);

  useEffect(() => {
    cancelledRef.current = false;
    void load();
    return () => {
      cancelledRef.current = true;
    };
  }, [load]);

  const handleAdd = async () => {
    const trimmedSlug = newSlug.trim();
    const isEditing = editingSlug !== null;
    setAddBusy(true);
    setAddError(null);
    const result = await addSkill(trimmedSlug, newMarkdown);
    setAddBusy(false);
    if (result.ok) {
      setNotice(
        isEditing
          ? `Skill '${result.skill?.slug ?? trimmedSlug}' updated`
          : `Skill '${result.skill?.slug ?? trimmedSlug}' saved`,
      );
      setShowAdd(false);
      setNewSlug("");
      setNewMarkdown(TEMPLATE);
      setEditingSlug(null);
      void load();
    } else {
      setAddError(result.error ?? "Could not save skill");
    }
  };

  const handleDelete = async (slug: string) => {
    setNotice(null);
    const result = await deleteSkill(slug);
    if (result.ok) {
      setNotice(`Skill '${slug}' removed`);
      void load();
    } else {
      setError(result.error ?? "Could not delete skill");
    }
  };

  const handleToggleEnabled = async (skill: Skill) => {
    setTogglingSlug(skill.slug);
    const next = !skill.enabled;
    // optimistic
    setSkills((prev) =>
      prev ? prev.map((s) => (s.slug === skill.slug ? { ...s, enabled: next } : s)) : prev,
    );
    const result = await setSkillEnabled(skill.slug, next);
    setTogglingSlug(null);
    if (!result.ok) {
      setError(result.error ?? "Could not update skill");
      void load();
    }
  };

  const openBrowse = () => {
    setBrowseKey((k) => k + 1);
    setShowBrowse(true);
  };

  const handleEdit = (skill: Skill) => {
    setShowAdd(true);
    setAddError(null);
    setEditingSlug(skill.slug);
    setNewSlug(skill.slug);
    setNewMarkdown(composeSkillMarkdown(skill));
  };

  return (
    <div className="panel p-4 sm:p-6">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <p className="mono-label">Skills</p>
        <div className="flex items-center gap-2">
          <button
            type="button"
            onClick={openBrowse}
            className="rounded-lg border border-white/15 bg-white/5 px-3 py-1 font-mono text-[11px] text-white/80 transition hover:bg-white/10"
          >
            Browse ClawHub
          </button>
          <button
            type="button"
            onClick={() => {
              setShowAdd((v) => {
                if (v) {
                  setAddError(null);
                  setEditingSlug(null);
                  setNewSlug("");
                  setNewMarkdown(TEMPLATE);
                }
                return !v;
              });
            }}
            className="rounded-lg border border-emerald-300/20 bg-emerald-300/10 px-3 py-1 font-mono text-[11px] text-emerald-300 transition hover:bg-emerald-300/20"
          >
            {showAdd ? "Cancel" : "Add custom skill"}
          </button>
        </div>
      </div>

      <p className="mt-2 font-mono text-[10px] text-white/40" title={customDir}>
        Custom dir: {customDir || "—"}
      </p>

      {notice && (
        <p className="mt-3 font-mono text-[11px] text-fuchsia-200/90" role="status">
          {notice}
        </p>
      )}
      {error && (
        <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
          {error}
        </p>
      )}

      {showAdd && (
        <div className="mt-4 rounded-xl border border-white/10 bg-white/5 p-3">
          <label className="block font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
            Slug (a-z, 0-9, -, _)
          </label>
          <input
            value={newSlug}
            onChange={(e) => setNewSlug(e.target.value)}
            placeholder="my-skill"
            disabled={editingSlug !== null}
            className="mt-1 w-full rounded-lg border border-white/10 bg-black/20 px-2 py-1 font-mono text-xs text-white outline-none focus:border-emerald-300/40"
          />
          <label className="mt-3 block font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
            SKILL.md
          </label>
          <textarea
            value={newMarkdown}
            onChange={(e) => setNewMarkdown(e.target.value)}
            rows={14}
            className="mt-1 w-full rounded-lg border border-white/10 bg-black/20 px-2 py-1.5 font-mono text-[11px] text-white outline-none focus:border-emerald-300/40"
            spellCheck={false}
          />
          {addError && (
            <p className="mt-2 font-mono text-[11px] text-rose-300" role="alert">
              {addError}
            </p>
          )}
          <div className="mt-3 flex justify-end">
            <button
              type="button"
              onClick={() => void handleAdd()}
              disabled={addBusy || !newSlug.trim()}
              className="rounded-lg border border-emerald-300/20 bg-emerald-300/10 px-3 py-1 font-mono text-[11px] text-emerald-300 transition hover:bg-emerald-300/20 disabled:opacity-40"
            >
              {addBusy ? "Saving…" : editingSlug ? "Save changes" : "Save skill"}
            </button>
          </div>
        </div>
      )}

      {showBrowse && (
        <ClawHubBrowse
          key={browseKey}
          onClose={() => setShowBrowse(false)}
          onAfterSkillInstall={(slug) => {
            setNotice(`Installed '${slug}' from ClawHub`);
            setShowBrowse(false);
            void load();
          }}
        />
      )}

      {loading && skills === null && (
        <p className="mt-3 font-mono text-[11px] uppercase tracking-[0.14em] text-(--mid)">
          Loading…
        </p>
      )}

      {skills !== null && skills.length === 0 && (
        <p className="mt-3 subtle-copy">No skills yet. Add one or browse ClawHub.</p>
      )}

      {skills !== null && skills.length > 0 && (
        <div className="mt-4 grid gap-2">
          {skills.map((skill) => (
            <div
              key={skill.slug}
              className={`rounded-xl border px-3 py-2.5 transition ${
                skill.enabled
                  ? "border-white/10 bg-white/5"
                  : "border-white/5 bg-white/[0.015] opacity-60"
              }`}
            >
              <div className="flex min-w-0 items-start justify-between gap-3">
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <p className="truncate text-sm font-semibold text-white">{skill.name}</p>
                    <span
                      className={`rounded-full px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider ${
                        skill.origin === "bundled"
                          ? "bg-white/10 text-white/60"
                          : "bg-emerald-300/10 text-emerald-300/80"
                      }`}
                    >
                      {skill.origin}
                    </span>
                  </div>
                  <p className="mt-0.5 font-mono text-[11px] text-(--mid)">
                    {skill.slug}
                    {skill.version ? ` · v${skill.version}` : ""}
                    {skill.tags.length > 0 ? ` · ${skill.tags.join(", ")}` : ""}
                  </p>
                  <p className="mt-1 text-[11px] leading-snug text-(--mid)">{skill.description}</p>
                </div>
                <div className="flex shrink-0 items-center gap-2">
                  <button
                    type="button"
                    role="switch"
                    aria-checked={skill.enabled}
                    onClick={() => void handleToggleEnabled(skill)}
                    disabled={togglingSlug === skill.slug}
                    title={skill.enabled ? "Disable skill" : "Enable skill"}
                    className={`relative h-5 w-9 rounded-full border transition disabled:opacity-50 ${
                      skill.enabled
                        ? "border-emerald-300/30 bg-emerald-300/20"
                        : "border-white/10 bg-white/5"
                    }`}
                  >
                    <span
                      className={`absolute top-1/2 block h-3.5 w-3.5 -translate-y-1/2 rounded-full transition ${
                        skill.enabled
                          ? "left-[18px] bg-emerald-300 shadow-[0_0_6px_rgba(52,211,153,0.5)]"
                          : "left-[2px] bg-white/40"
                      }`}
                    />
                  </button>
                  {skill.origin === "custom" && (
                    <>
                      <button
                        type="button"
                        onClick={() => handleEdit(skill)}
                        className="rounded-lg border border-white/15 bg-white/5 px-3 py-1 font-mono text-[11px] text-white/80 transition hover:bg-white/10"
                      >
                        Edit
                      </button>
                      <button
                        type="button"
                        onClick={() => void handleDelete(skill.slug)}
                        className="rounded-lg border border-rose-300/20 bg-transparent px-3 py-1 font-mono text-[11px] text-rose-300/70 transition hover:bg-rose-300/10 hover:text-rose-200"
                      >
                        Delete
                      </button>
                    </>
                  )}
                </div>
              </div>

              <Accordion.Root
                type="single"
                collapsible
                className="mt-2 border-t border-white/5 pt-2"
              >
                <Accordion.Item
                  value={`${skill.slug}-body`}
                  className="overflow-hidden rounded-lg border border-white/10 bg-white/3"
                >
                  <Accordion.Header>
                    <Accordion.Trigger className="group flex w-full items-center justify-between gap-3 px-3 py-2 text-left">
                      <p className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                        SKILL.md body
                      </p>
                      <span className="shrink-0 font-mono text-xs text-(--mid) transition group-data-[state=open]:rotate-45">
                        +
                      </span>
                    </Accordion.Trigger>
                  </Accordion.Header>
                  <Accordion.Content className="border-t border-white/10 px-3 py-2">
                    <pre className="max-h-72 overflow-auto whitespace-pre-wrap break-words font-mono text-[11px] leading-relaxed text-white/70">
                      {skill.body}
                    </pre>
                  </Accordion.Content>
                </Accordion.Item>
              </Accordion.Root>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
