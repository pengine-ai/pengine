import * as Accordion from "@radix-ui/react-accordion";
import {
  closestCenter,
  defaultDropAnimation,
  DndContext,
  DragOverlay,
  type DragEndEvent,
  type DragStartEvent,
  KeyboardSensor,
  PointerSensor,
  useSensor,
  useSensors,
} from "@dnd-kit/core";
import { restrictToVerticalAxis } from "@dnd-kit/modifiers";
import {
  arrayMove,
  SortableContext,
  sortableKeyboardCoordinates,
  useSortable,
  verticalListSortingStrategy,
} from "@dnd-kit/sortable";
import { CSS } from "@dnd-kit/utilities";
import { useCallback, useEffect, useRef, useState } from "react";
import { fetchUserSettings, putUserSettings } from "../../settings";
import { addSkill, deleteSkill, fetchSkills, putSkillSlugOrder, setSkillEnabled } from "../api";
import { type Skill, skillMandatoryMarkdown } from "../types";
import { ClawHubBrowse } from "./ClawHubBrowse";
import { SkillsContextBytesSlider } from "./SkillsContextBytesSlider";

function formatContextKiB(bytes: number): string {
  const kb = bytes / 1024;
  if (kb >= 1024 && kb % 1024 === 0) return `${kb / 1024} MiB`;
  if (kb >= 100) return `${Math.round(kb)} KiB`;
  if (kb >= 10) return `${kb.toFixed(1)} KiB`;
  return `${kb.toFixed(2)} KiB`;
}

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

type SortableSkillCardProps = {
  skill: Skill;
  togglingSlug: string | null;
  onToggleEnabled: (skill: Skill) => void;
  onEdit: (skill: Skill) => void;
  onDelete: (slug: string) => void;
};

function SortableSkillCard({
  skill,
  togglingSlug,
  onToggleEnabled,
  onEdit,
  onDelete,
}: SortableSkillCardProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: skill.slug,
  });

  const style = {
    transform: CSS.Transform.toString(transform),
    transition,
    ...(isDragging ? { opacity: 0, pointerEvents: "none" as const } : {}),
  };

  return (
    <div
      ref={setNodeRef}
      style={style}
      className={`rounded-xl border px-3 py-2.5 transition ${
        skill.enabled ? "border-white/10 bg-white/5" : "border-white/5 bg-white/[0.015] opacity-60"
      }`}
    >
      <div className="flex min-w-0 items-start justify-between gap-3">
        <button
          type="button"
          {...attributes}
          {...listeners}
          className="mt-0.5 shrink-0 cursor-grab select-none border-0 bg-transparent p-0 font-mono text-sm text-white/30 outline-none active:cursor-grabbing"
          title="Drag to reorder"
          aria-label={`Drag to reorder ${skill.slug}`}
        >
          ::
        </button>
        <div className="min-w-0 flex-1">
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
            onClick={() => void onToggleEnabled(skill)}
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
                onClick={() => void onEdit(skill)}
                className="rounded-lg border border-white/15 bg-white/5 px-3 py-1 font-mono text-[11px] text-white/80 transition hover:bg-white/10"
              >
                Edit
              </button>
              <button
                type="button"
                onClick={() => void onDelete(skill.slug)}
                className="rounded-lg border border-rose-300/20 bg-transparent px-3 py-1 font-mono text-[11px] text-rose-300/70 transition hover:bg-rose-300/10 hover:text-rose-200"
              >
                Delete
              </button>
            </>
          )}
        </div>
      </div>

      <Accordion.Root type="single" collapsible className="mt-2 border-t border-white/5 pt-2">
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
  );
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
  const [newMandatoryMarkdown, setNewMandatoryMarkdown] = useState("");
  const [editingSlug, setEditingSlug] = useState<string | null>(null);
  const [addBusy, setAddBusy] = useState(false);
  const [addError, setAddError] = useState<string | null>(null);

  const [showBrowse, setShowBrowse] = useState(false);
  const [browseKey, setBrowseKey] = useState(0);

  const [ctxBytes, setCtxBytes] = useState(10 * 1024);
  const [ctxSaved, setCtxSaved] = useState(10 * 1024);
  const [ctxMin, setCtxMin] = useState(4 * 1024);
  const [ctxMax, setCtxMax] = useState(256 * 1024);
  const [ctxDefault, setCtxDefault] = useState(10 * 1024);
  const [ctxLoaded, setCtxLoaded] = useState(false);
  const [ctxSaving, setCtxSaving] = useState(false);
  const [ctxErr, setCtxErr] = useState<string | null>(null);
  const [ctxSettingsErr, setCtxSettingsErr] = useState<string | null>(null);

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

  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const [activeDragSkill, setActiveDragSkill] = useState<Skill | null>(null);

  const handleSkillDragStart = useCallback(
    (event: DragStartEvent) => {
      const slug = String(event.active.id);
      setActiveDragSkill(skills?.find((s) => s.slug === slug) ?? null);
    },
    [skills],
  );

  const clearSkillDrag = useCallback(() => {
    setActiveDragSkill(null);
  }, []);

  const handleSkillOrderDragEnd = useCallback(
    (event: DragEndEvent) => {
      clearSkillDrag();
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      setSkills((current) => {
        if (!current) return current;
        const oldIndex = current.findIndex((s) => s.slug === active.id);
        const newIndex = current.findIndex((s) => s.slug === over.id);
        if (oldIndex < 0 || newIndex < 0) return current;
        const next = arrayMove(current, oldIndex, newIndex);
        void (async () => {
          const r = await putSkillSlugOrder(next.map((s) => s.slug));
          if (!r.ok) {
            setError(r.error ?? "Could not save skill order");
            void load();
          }
        })();
        return next;
      });
    },
    [clearSkillDrag, load],
  );

  const handleSkillDragCancel = useCallback(() => {
    clearSkillDrag();
  }, [clearSkillDrag]);

  useEffect(() => {
    cancelledRef.current = false;
    void load();
    return () => {
      cancelledRef.current = true;
    };
  }, [load]);

  useEffect(() => {
    let gone = false;
    (async () => {
      const us = await fetchUserSettings(4000);
      if (gone) return;
      if (!us) {
        setCtxSettingsErr(
          "Could not load settings from Pengine (offline or server error). Using built-in defaults for the slider limits.",
        );
        setCtxLoaded(true);
        return;
      }
      setCtxSettingsErr(null);
      setCtxBytes(us.skills_hint_max_bytes);
      setCtxSaved(us.skills_hint_max_bytes);
      setCtxMin(us.skills_hint_max_bytes_min);
      setCtxMax(us.skills_hint_max_bytes_max);
      setCtxDefault(us.skills_hint_max_bytes_default);
      setCtxLoaded(true);
    })();
    return () => {
      gone = true;
    };
  }, []);

  const ctxDirty = ctxLoaded && ctxBytes !== ctxSaved;

  const saveContextBytes = async (bytes: number) => {
    setCtxErr(null);
    setCtxSaving(true);
    const result = await putUserSettings(bytes);
    setCtxSaving(false);
    if (result.ok) {
      const v = result.settings.skills_hint_max_bytes;
      setCtxBytes(v);
      setCtxSaved(v);
      return;
    }
    setCtxErr(result.error);
  };

  const handleAdd = async () => {
    const trimmedSlug = newSlug.trim();
    const isEditing = editingSlug !== null;
    setAddBusy(true);
    setAddError(null);
    const result = await addSkill(trimmedSlug, newMarkdown, newMandatoryMarkdown);
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
      setNewMandatoryMarkdown("");
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

  const handleEdit = async (skill: Skill) => {
    const resp = await fetchSkills();
    if (resp) {
      setSkills(resp.skills);
      setCustomDir(resp.custom_dir);
    }
    const fresh = resp?.skills.find((s) => s.slug === skill.slug) ?? skill;
    setShowAdd(true);
    setAddError(null);
    setEditingSlug(fresh.slug);
    setNewSlug(fresh.slug);
    setNewMarkdown(composeSkillMarkdown(fresh));
    setNewMandatoryMarkdown(skillMandatoryMarkdown(fresh));
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
                  setNewMandatoryMarkdown("");
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

      <div
        className="mt-3 rounded-lg border border-white/8 bg-white/2 px-2.5 py-2 sm:px-3"
        title="Max UTF-8 bytes for the combined skills block in the system prompt (enabled skills + mandatory). Lower = less context for the first model step; higher = more recipe text before truncation."
      >
        <div className="flex flex-col gap-2 sm:flex-row sm:items-center sm:gap-3">
          <div className="flex min-w-0 shrink-0 items-baseline gap-2 sm:w-36">
            <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
              Context
            </span>
            <span className="font-mono text-xs tabular-nums text-cyan-200/90">
              {ctxLoaded ? formatContextKiB(ctxBytes) : "…"}
            </span>
          </div>
          <div className="flex min-w-0 flex-1 flex-col gap-1">
            <SkillsContextBytesSlider
              min={ctxMin}
              max={ctxMax}
              step={1024}
              value={ctxBytes}
              disabled={!ctxLoaded || ctxSaving}
              onValueChange={setCtxBytes}
              aria-label="Skills context size in bytes"
            />
            <p className="font-mono text-[9px] text-white/35">
              {ctxLoaded
                ? `${formatContextKiB(ctxMin)}–${formatContextKiB(ctxMax)} · default ${formatContextKiB(ctxDefault)}`
                : "Loading limits…"}
            </p>
          </div>
          <div className="flex shrink-0 items-center justify-end gap-1.5 sm:justify-start">
            <button
              type="button"
              disabled={!ctxDirty || ctxSaving || !ctxLoaded}
              onClick={() => void saveContextBytes(ctxBytes)}
              className="rounded-md border border-cyan-300/25 bg-cyan-300/10 px-2 py-1 font-mono text-[10px] text-cyan-100 transition hover:bg-cyan-300/15 disabled:pointer-events-none disabled:opacity-35"
            >
              {ctxSaving ? "…" : "Save"}
            </button>
            <button
              type="button"
              disabled={ctxSaving || !ctxLoaded || ctxBytes === ctxDefault}
              onClick={() => void saveContextBytes(ctxDefault)}
              className="rounded-md border border-white/12 bg-transparent px-2 py-1 font-mono text-[10px] text-white/45 transition hover:border-white/20 hover:text-white/65 disabled:pointer-events-none disabled:opacity-35"
            >
              Default
            </button>
          </div>
        </div>
        {ctxSettingsErr && (
          <p className="mt-1.5 font-mono text-[10px] text-amber-200/90" role="status">
            {ctxSettingsErr}
          </p>
        )}
        {ctxErr && (
          <p className="mt-1.5 font-mono text-[10px] text-rose-300" role="alert">
            {ctxErr}
          </p>
        )}
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
          <div className="mt-3 space-y-4 border-t border-white/10 pt-3">
            <div className="min-w-0">
              <label className="block font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
                SKILL.md
              </label>
              <textarea
                value={newMarkdown}
                onChange={(e) => setNewMarkdown(e.target.value)}
                rows={editingSlug ? 22 : 14}
                className="mt-1 max-h-[min(70vh,36rem)] w-full min-h-[12rem] resize-y rounded-lg border border-white/10 bg-black/20 px-2 py-1.5 font-mono text-[11px] text-white outline-none focus:border-emerald-300/40"
                spellCheck={false}
              />
            </div>
            <div className="min-w-0 rounded-lg border border-white/8 bg-black/10 px-2 py-2">
              <label className="block font-mono text-[9px] uppercase tracking-[0.12em] text-(--mid)">
                mandatory.md <span className="normal-case text-white/35">(optional)</span>
              </label>
              <p className="mt-0.5 font-mono text-[8px] leading-snug text-white/32">
                Appended to the system prompt. Empty = none (removes file on save).
              </p>
              <textarea
                value={newMandatoryMarkdown}
                onChange={(e) => setNewMandatoryMarkdown(e.target.value)}
                rows={3}
                placeholder={"# Optional…"}
                className="mt-1 w-full resize-y rounded-md border border-white/10 bg-black/25 px-1.5 py-1 font-mono text-[10px] leading-snug text-white outline-none focus:border-fuchsia-300/35"
                spellCheck={false}
              />
            </div>
          </div>
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
          <p className="font-mono text-[10px] text-white/45">
            Drag the <span className="text-white/55">::</span> handle to reorder. Order is saved to
            disk and used for the skills block in the system prompt.
          </p>
          <DndContext
            sensors={sensors}
            collisionDetection={closestCenter}
            modifiers={[restrictToVerticalAxis]}
            onDragStart={handleSkillDragStart}
            onDragEnd={handleSkillOrderDragEnd}
            onDragCancel={handleSkillDragCancel}
          >
            <SortableContext
              items={skills.map((s) => s.slug)}
              strategy={verticalListSortingStrategy}
            >
              <div className="grid gap-2">
                {skills.map((skill) => (
                  <SortableSkillCard
                    key={skill.slug}
                    skill={skill}
                    togglingSlug={togglingSlug}
                    onToggleEnabled={handleToggleEnabled}
                    onEdit={handleEdit}
                    onDelete={handleDelete}
                  />
                ))}
              </div>
            </SortableContext>
            <DragOverlay dropAnimation={defaultDropAnimation}>
              {activeDragSkill ? (
                <div
                  className="pointer-events-none max-w-[min(100vw-2rem,42rem)] rounded-xl border border-white/20 bg-[#141418] px-3 py-2.5 shadow-xl ring-1 ring-white/10"
                  style={{ cursor: "grabbing" }}
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="shrink-0 font-mono text-sm text-white/35">::</span>
                    <p className="truncate text-sm font-semibold text-white">
                      {activeDragSkill.name}
                    </p>
                    <span className="shrink-0 rounded-full bg-white/10 px-1.5 py-0.5 font-mono text-[9px] uppercase tracking-wider text-white/60">
                      {activeDragSkill.origin}
                    </span>
                  </div>
                  <p className="mt-0.5 truncate font-mono text-[11px] text-(--mid)">
                    {activeDragSkill.slug}
                  </p>
                </div>
              ) : null}
            </DragOverlay>
          </DndContext>
        </div>
      )}
    </div>
  );
}
