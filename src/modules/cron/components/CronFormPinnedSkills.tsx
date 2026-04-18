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
import { useCallback, useState } from "react";
import type { Skill } from "../../skills/types";

type SortableCronSkillSlugRowProps = {
  slug: string;
  metaName: string;
  onRemove: () => void;
};

function SortableCronSkillSlugRow({ slug, metaName, onRemove }: SortableCronSkillSlugRowProps) {
  const { attributes, listeners, setNodeRef, transform, transition, isDragging } = useSortable({
    id: slug,
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
      role="listitem"
      className="flex items-center gap-2 rounded border border-white/10 bg-black/25 px-2 py-1 font-mono text-[11px] text-white/85"
    >
      <button
        type="button"
        {...attributes}
        {...listeners}
        className="shrink-0 cursor-grab select-none border-0 bg-transparent p-0 font-mono text-white/35 outline-none active:cursor-grabbing"
        title="Drag to reorder"
        aria-label={`Drag to reorder ${slug}`}
      >
        ::
      </button>
      <span className="shrink-0 text-cyan-200/90">{slug}</span>
      <span className="min-w-0 flex-1 truncate text-white/45">{metaName}</span>
      <button
        type="button"
        className="shrink-0 rounded px-1 font-mono text-[10px] text-rose-300/90 hover:bg-rose-300/10"
        onClick={onRemove}
      >
        Remove
      </button>
    </div>
  );
}

export type CronFormPinnedSkillsProps = {
  skillsCatalog: Skill[];
  skillSlugs: string[];
  onSkillSlugsChange: (skillSlugs: string[]) => void;
};

export function CronFormPinnedSkills({
  skillsCatalog,
  skillSlugs,
  onSkillSlugsChange,
}: CronFormPinnedSkillsProps) {
  const sensors = useSensors(
    useSensor(PointerSensor, { activationConstraint: { distance: 8 } }),
    useSensor(KeyboardSensor, { coordinateGetter: sortableKeyboardCoordinates }),
  );

  const [activeSlug, setActiveSlug] = useState<string | null>(null);

  const handleDragStart = useCallback((event: DragStartEvent) => {
    setActiveSlug(String(event.active.id));
  }, []);

  const clearDrag = useCallback(() => {
    setActiveSlug(null);
  }, []);

  const handleDragEnd = useCallback(
    (event: DragEndEvent) => {
      clearDrag();
      const { active, over } = event;
      if (!over || active.id === over.id) return;
      const oldIndex = skillSlugs.findIndex((s) => s === active.id);
      const newIndex = skillSlugs.findIndex((s) => s === over.id);
      if (oldIndex < 0 || newIndex < 0) return;
      onSkillSlugsChange(arrayMove(skillSlugs, oldIndex, newIndex));
    },
    [clearDrag, onSkillSlugsChange, skillSlugs],
  );

  return (
    <div className="grid gap-1">
      <span className="font-mono text-[10px] uppercase tracking-[0.14em] text-(--mid)">
        Skills (optional)
      </span>
      <p className="font-mono text-[10px] text-white/45">
        Check recipes below to pin them for this job. Leave all unchecked to use every enabled skill
        (default). Drag the <span className="text-white/55">::</span> handle in the ordered list to
        set prompt order (top = first in the model context).
      </p>
      {skillSlugs.length > 0 && (
        <DndContext
          sensors={sensors}
          collisionDetection={closestCenter}
          modifiers={[restrictToVerticalAxis]}
          onDragStart={handleDragStart}
          onDragEnd={handleDragEnd}
          onDragCancel={clearDrag}
        >
          <SortableContext items={skillSlugs} strategy={verticalListSortingStrategy}>
            <div
              className="space-y-1 rounded-md border border-cyan-300/15 bg-cyan-300/[0.06] p-2"
              role="list"
              aria-label="Skill order"
            >
              {skillSlugs.map((slug) => {
                const meta = skillsCatalog.find((s) => s.slug === slug);
                return (
                  <SortableCronSkillSlugRow
                    key={slug}
                    slug={slug}
                    metaName={meta?.name ?? ""}
                    onRemove={() => onSkillSlugsChange(skillSlugs.filter((x) => x !== slug))}
                  />
                );
              })}
            </div>
          </SortableContext>
          <DragOverlay dropAnimation={defaultDropAnimation}>
            {activeSlug ? (
              <div
                className="pointer-events-none flex max-w-md items-center gap-2 rounded border border-cyan-300/35 bg-[#141418] px-2 py-1 font-mono text-[11px] text-white/90 shadow-lg ring-1 ring-cyan-300/20"
                style={{ cursor: "grabbing" }}
              >
                <span className="text-white/35">::</span>
                <span className="shrink-0 text-cyan-200/90">{activeSlug}</span>
                <span className="min-w-0 flex-1 truncate text-white/50">
                  {skillsCatalog.find((s) => s.slug === activeSlug)?.name ?? ""}
                </span>
              </div>
            ) : null}
          </DragOverlay>
        </DndContext>
      )}
      <div className="max-h-40 overflow-y-auto rounded-md border border-white/10 bg-black/20 p-2">
        {[...skillsCatalog]
          .filter((s) => s.enabled)
          .sort((a, b) => a.slug.localeCompare(b.slug))
          .map((s) => (
            <label
              key={s.slug}
              className="flex cursor-pointer items-center gap-2 py-0.5 font-mono text-[11px] text-white/80"
            >
              <input
                type="checkbox"
                checked={skillSlugs.includes(s.slug)}
                onChange={() => {
                  const has = skillSlugs.includes(s.slug);
                  onSkillSlugsChange(
                    has ? skillSlugs.filter((x) => x !== s.slug) : [...skillSlugs, s.slug],
                  );
                }}
              />
              <span className="shrink-0 text-cyan-200/90">{s.slug}</span>
              <span className="min-w-0 truncate text-white/50">{s.name}</span>
            </label>
          ))}
      </div>
    </div>
  );
}
