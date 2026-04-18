import * as ScrollArea from "@radix-ui/react-scroll-area";
import type { RefObject } from "react";
import type { ClawHubPlugin, ClawHubSkill } from "../types";
import {
  CLAWHUB_PLUGINS_CATALOG_ESTIMATE,
  clawHubSkillDetailUrl,
  formatClawHubStatsInstalls,
  formatClawHubStatsPrimary,
  formatClawHubUpdated,
} from "./clawHubBrowseFormat";

function ClawHubSkillRow({
  entry,
  installingSlug,
  onInstall,
}: {
  entry: ClawHubSkill;
  installingSlug: string | null;
  onInstall: (entry: ClawHubSkill) => void;
}) {
  const installsSub = formatClawHubStatsInstalls(entry);
  return (
    <div
      role="row"
      className="min-w-[44rem] px-3 py-2.5 md:grid md:grid-cols-[minmax(0,1.1fr)_minmax(0,2.2fr)_5.5rem_minmax(0,7.5rem)_5.5rem] md:items-start md:gap-x-2 md:py-2"
    >
      <div className="min-w-0 md:pt-0.5">
        <p className="break-words text-sm font-semibold text-white">{entry.displayName}</p>
        <div className="mt-1 flex min-w-0 flex-wrap items-center gap-1">
          {entry.isHighlighted === true && (
            <span className="rounded-full border border-amber-300/30 bg-amber-300/10 px-1.5 py-px font-mono text-[8px] font-medium text-amber-200/95">
              Highlighted
            </span>
          )}
          {entry.isOfficial === true && (
            <span className="rounded-full border border-cyan-300/25 bg-cyan-300/10 px-1.5 py-px font-mono text-[8px] font-medium text-cyan-200/90">
              Official
            </span>
          )}
          {entry.version != null && entry.version !== "" && (
            <span className="font-mono text-[9px] text-white/50">v{entry.version}</span>
          )}
        </div>
        <p className="mt-0.5 break-all font-mono text-[9px] text-white/35">{entry.slug}</p>
        {(entry.updatedAt != null || (entry.score != null && Number.isFinite(entry.score))) && (
          <p className="mt-1 font-mono text-[8px] leading-relaxed text-white/32">
            {entry.updatedAt != null && <>Upd {formatClawHubUpdated(entry.updatedAt)}</>}
            {entry.updatedAt != null &&
              entry.score != null &&
              Number.isFinite(entry.score) &&
              " · "}
            {entry.score != null && Number.isFinite(entry.score) && (
              <>score {entry.score.toFixed(2)}</>
            )}
          </p>
        )}
      </div>
      <div className="mt-2 min-w-0 md:mt-0 md:pt-0.5">
        <p className="break-words text-[11px] leading-snug text-(--mid)">{entry.summary}</p>
      </div>
      <div className="mt-2 font-mono text-[10px] text-white/55 md:mt-0 md:pt-1">
        <span className="text-white/35 md:hidden">Author </span>
        {entry.ownerHandle ? (
          <span className="break-all text-cyan-200/85">@{entry.ownerHandle}</span>
        ) : (
          <span className="text-white/30">—</span>
        )}
      </div>
      <div className="mt-1 min-w-0 font-mono text-[10px] tabular-nums text-white/65 md:mt-0 md:pt-1">
        <span className="text-white/35 md:hidden">Stats </span>
        <span className="break-words">{formatClawHubStatsPrimary(entry)}</span>
        {installsSub && (
          <span className="mt-0.5 block break-words text-[9px] text-white/40">
            {installsSub}
            {entry.commentsCount != null ? ` · ${entry.commentsCount} comments` : ""}
          </span>
        )}
        {!installsSub && entry.commentsCount != null && (
          <span className="mt-0.5 block text-[9px] text-white/40">
            {entry.commentsCount} comments
          </span>
        )}
      </div>
      <div className="mt-2 flex shrink-0 flex-col gap-1.5 md:mt-0 md:items-end md:pt-0.5">
        <button
          type="button"
          onClick={() => void onInstall(entry)}
          disabled={installingSlug !== null}
          className="w-full rounded-lg border border-emerald-300/20 bg-emerald-300/10 px-2.5 py-1 font-mono text-[10px] text-emerald-300 transition hover:bg-emerald-300/20 disabled:opacity-40 md:w-auto"
        >
          {installingSlug === entry.slug ? "Installing…" : "Install"}
        </button>
        <a
          href={clawHubSkillDetailUrl(entry.slug)}
          target="_blank"
          rel="noreferrer"
          title="Open full skill page on ClawHub (OpenClaw registry)"
          className="w-full rounded-lg border border-white/15 bg-white/8 px-2.5 py-1 text-center font-mono text-[10px] text-white/85 transition hover:bg-white/14 md:w-auto"
        >
          Details
        </a>
      </div>
    </div>
  );
}

export function ClawHubSkillsResultsSection({
  active,
  browseLoading,
  results,
  listViewportRef,
  loadMoreRef,
  hasMoreSkills,
  loadingMore,
  installingSlug,
  onInstallSkill,
}: {
  active: boolean;
  browseLoading: boolean;
  results: ClawHubSkill[] | null;
  listViewportRef: RefObject<HTMLDivElement | null>;
  loadMoreRef: RefObject<HTMLDivElement | null>;
  hasMoreSkills: boolean;
  loadingMore: boolean;
  installingSlug: string | null;
  onInstallSkill: (entry: ClawHubSkill) => void;
}) {
  if (!active || !results) return null;
  if (results.length === 0 && !browseLoading) {
    return <p className="mt-3 subtle-copy">No matches.</p>;
  }
  if (results.length === 0) return null;
  return (
    <>
      <p className="mt-2 font-mono text-[10px] text-white/40">
        {results.length} skill{results.length === 1 ? "" : "s"} shown
        <span className="text-white/32"> · author &amp; stats from ClawHub when available</span>
      </p>
      <ScrollArea.Root className="mt-2 h-[440px] overflow-hidden rounded-lg border border-white/10 bg-black/10">
        <ScrollArea.Viewport ref={listViewportRef} className="h-full w-full">
          <div className="hidden min-w-[44rem] grid-cols-[minmax(0,1.1fr)_minmax(0,2.2fr)_5.5rem_minmax(0,7.5rem)_5.5rem] gap-x-2 border-b border-white/10 bg-white/[0.04] px-3 py-1.5 font-mono text-[9px] uppercase tracking-[0.12em] text-white/45 md:grid">
            <div>Skill</div>
            <div>Summary</div>
            <div>Author</div>
            <div>Stats</div>
            <div className="text-right"> </div>
          </div>
          <div className="min-w-0 divide-y divide-white/10">
            {results.map((entry) => (
              <ClawHubSkillRow
                key={entry.slug}
                entry={entry}
                installingSlug={installingSlug}
                onInstall={onInstallSkill}
              />
            ))}
            {(hasMoreSkills || loadingMore) && (
              <div
                ref={loadMoreRef}
                className="py-2 text-center font-mono text-[10px] text-white/45"
              >
                {loadingMore ? "Loading more…" : "Scroll for more"}
              </div>
            )}
          </div>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar
          orientation="vertical"
          className="flex w-2.5 touch-none select-none border-l border-l-white/5 bg-white/5 p-0.5"
        >
          <ScrollArea.Thumb className="relative flex-1 rounded-full bg-white/20" />
        </ScrollArea.Scrollbar>
        <ScrollArea.Corner className="bg-white/5" />
      </ScrollArea.Root>
    </>
  );
}

export function ClawHubPluginsResultsSection({
  active,
  browseLoading,
  pluginResults,
  visiblePlugins,
  pluginTagFilter,
  listViewportRef,
  loadMoreRef,
  hasMorePlugins,
  loadingMore,
}: {
  active: boolean;
  browseLoading: boolean;
  pluginResults: ClawHubPlugin[] | null;
  visiblePlugins: ClawHubPlugin[] | null;
  pluginTagFilter: string;
  listViewportRef: RefObject<HTMLDivElement | null>;
  loadMoreRef: RefObject<HTMLDivElement | null>;
  hasMorePlugins: boolean;
  loadingMore: boolean;
}) {
  if (!active) return null;

  const emptyFiltered =
    visiblePlugins && visiblePlugins.length === 0 && !browseLoading && pluginResults !== null;

  return (
    <>
      {emptyFiltered && (
        <p className="mt-3 subtle-copy">
          {pluginResults!.length ? "No plugins match this tag filter." : "No matches."}
        </p>
      )}
      {pluginResults && (
        <p className="mt-2 font-mono text-[10px] text-white/40">
          {pluginResults.length.toLocaleString()} loaded
          {hasMorePlugins ? " · scroll for more" : " · end of list"}
          {!pluginTagFilter.trim() ? (
            <span className="text-white/32">
              {" "}
              · ~{CLAWHUB_PLUGINS_CATALOG_ESTIMATE.toLocaleString()} in registry
            </span>
          ) : null}
          {pluginTagFilter.trim() && visiblePlugins && pluginResults.length > 0 ? (
            <span className="text-white/32">
              {" "}
              · {visiblePlugins.length.toLocaleString()} match tag filter
            </span>
          ) : null}
        </p>
      )}
      {visiblePlugins && visiblePlugins.length > 0 && (
        <ScrollArea.Root className="mt-2 h-[440px] overflow-hidden rounded-lg border border-white/10 bg-black/10">
          <ScrollArea.Viewport ref={listViewportRef} className="h-full w-full">
            <div className="grid min-w-0 gap-2 p-2">
              {visiblePlugins.map((p) => (
                <div
                  key={p.name}
                  className="flex min-w-0 flex-col gap-3 rounded-lg border border-white/10 bg-white/[0.03] px-3 py-2 sm:flex-row sm:items-start sm:justify-between sm:gap-3"
                >
                  <div className="min-w-0 flex-1">
                    <p className="break-words text-sm font-semibold text-white">{p.displayName}</p>
                    <p className="mt-0.5 break-all font-mono text-[10px] text-(--mid)">
                      {p.name}
                      {p.ownerHandle ? ` · @${p.ownerHandle}` : ""}
                    </p>
                    <p className="mt-1 break-words text-[11px] leading-snug text-(--mid)">
                      {p.summary}
                    </p>
                    {p.capabilityTags.length > 0 && (
                      <p className="mt-1.5 break-words font-mono text-[9px] leading-snug text-white/40">
                        {p.capabilityTags.join(" · ")}
                      </p>
                    )}
                  </div>
                  <a
                    href={`https://clawhub.ai/plugins/${encodeURIComponent(p.name)}`}
                    target="_blank"
                    rel="noreferrer"
                    className="shrink-0 self-stretch rounded-lg border border-white/15 bg-white/8 px-3 py-1 text-center font-mono text-[11px] text-white/85 transition hover:bg-white/14 sm:self-auto"
                  >
                    Open
                  </a>
                </div>
              ))}
              {(hasMorePlugins || loadingMore) && (
                <div
                  ref={loadMoreRef}
                  className="py-1 text-center font-mono text-[10px] text-white/45"
                >
                  {loadingMore ? "Loading more…" : "Scroll for more"}
                </div>
              )}
            </div>
          </ScrollArea.Viewport>
          <ScrollArea.Scrollbar
            orientation="vertical"
            className="flex w-2.5 touch-none select-none border-l border-l-white/5 bg-white/5 p-0.5"
          >
            <ScrollArea.Thumb className="relative flex-1 rounded-full bg-white/20" />
          </ScrollArea.Scrollbar>
          <ScrollArea.Corner className="bg-white/5" />
        </ScrollArea.Root>
      )}
    </>
  );
}
