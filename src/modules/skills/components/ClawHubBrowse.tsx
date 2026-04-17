import * as ScrollArea from "@radix-ui/react-scroll-area";
import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { installClawHubSkill, searchClawHub, searchClawHubPlugins } from "../api";
import type { ClawHubPlugin, ClawHubSkill } from "../types";

function formatClawHubUpdated(ms: number): string {
  const d = new Date(ms);
  if (Number.isNaN(d.getTime())) return "—";
  return d.toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });
}

function clawHubSkillDetailUrl(slug: string): string {
  return `https://clawhub.ai/openclaw/${encodeURIComponent(slug)}`;
}

/** Match ClawHub list-style compact numbers (e.g. 39.1k). */
function fmtCompact(n: number): string {
  if (!Number.isFinite(n)) return "—";
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1).replace(/\.0$/, "")}M`;
  if (n >= 100_000) return `${Math.round(n / 1000)}k`;
  if (n >= 1000) return `${(n / 1000).toFixed(1).replace(/\.0$/, "")}k`;
  return String(n);
}

function formatClawHubStatsPrimary(entry: ClawHubSkill): string {
  const parts: string[] = [];
  if (entry.downloads != null) parts.push(fmtCompact(entry.downloads));
  if (entry.stars != null) parts.push(`★ ${entry.stars}`);
  if (entry.versionCount != null) parts.push(`${entry.versionCount} v`);
  return parts.length > 0 ? parts.join(" · ") : "—";
}

function formatClawHubStatsInstalls(entry: ClawHubSkill): string | null {
  if (entry.installsCurrent == null && entry.installsAllTime == null) return null;
  const bits: string[] = [];
  if (entry.installsCurrent != null) bits.push(`${fmtCompact(entry.installsCurrent)} cur`);
  if (entry.installsAllTime != null) bits.push(`${fmtCompact(entry.installsAllTime)} all`);
  return bits.join(" · ");
}

const CLAW_SORT_OPTIONS = [
  { value: "downloads", label: "Downloads" },
  { value: "relevance", label: "Relevance" },
  { value: "newest", label: "Newest" },
  { value: "updated", label: "Updated" },
  { value: "stars", label: "Stars" },
  { value: "name", label: "Name" },
] as const;

const SEARCH_PAGE_SIZE = 30;
/** Approximate ClawHub plugin registry size (for UI copy); list still loads until `nextCursor` ends. */
const CLAWHUB_PLUGINS_CATALOG_ESTIMATE = 55_561;

function FilterChip({
  active,
  label,
  title,
  onToggle,
}: {
  active: boolean;
  label: string;
  title?: string;
  onToggle: () => void;
}) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={active}
      title={title}
      onClick={onToggle}
      className={`rounded-full border px-2 py-0.5 font-mono text-[10px] transition ${
        active
          ? "border-emerald-300/35 bg-emerald-300/10 text-emerald-200/95"
          : "border-white/10 text-white/45 hover:border-white/18 hover:text-white/65"
      }`}
    >
      {label}
    </button>
  );
}

type ClawHubBrowseProps = {
  onClose: () => void;
  onAfterSkillInstall: (slug: string) => void;
};

export function ClawHubBrowse({ onClose, onAfterSkillInstall }: ClawHubBrowseProps) {
  const [clawRegistry, setClawRegistry] = useState<"skills" | "plugins">("skills");
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ClawHubSkill[] | null>(null);
  const [pluginResults, setPluginResults] = useState<ClawHubPlugin[] | null>(null);
  const [filterCleanOnly, setFilterCleanOnly] = useState(true);
  const [clawSort, setClawSort] = useState<string>("downloads");
  const [pluginTagFilter, setPluginTagFilter] = useState("");
  const [browseError, setBrowseError] = useState<string | null>(null);
  const [browseLoading, setBrowseLoading] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);
  const [installingSlug, setInstallingSlug] = useState<string | null>(null);
  const [hasMoreSkills, setHasMoreSkills] = useState(false);
  const [hasMorePlugins, setHasMorePlugins] = useState(false);

  const queryRef = useRef(query);
  queryRef.current = query;
  const listViewportRef = useRef<HTMLDivElement | null>(null);
  const loadMoreRef = useRef<HTMLDivElement | null>(null);
  const pluginCursorRef = useRef<string | null>(null);

  const runSkillsSearch = useCallback(
    async (q: string, limit: number, loadMore = false) => {
      const trimmed = q.trim();
      if (!trimmed) return;
      if (loadMore) setLoadingMore(true);
      else setBrowseLoading(true);
      setBrowseError(null);
      setPluginResults(null);
      const result = await searchClawHub(trimmed, {
        cleanOnly: filterCleanOnly,
        sort: clawSort,
        limit,
      });
      if (loadMore) setLoadingMore(false);
      else setBrowseLoading(false);
      if (result.results) {
        if (loadMore) {
          setResults((prev) => {
            if (!prev?.length) return result.results!;
            const next = result.results!;
            if (next.length <= prev.length) return prev;
            return [...prev, ...next.slice(prev.length)];
          });
        } else {
          setResults(result.results);
        }
        setHasMoreSkills(result.results.length >= limit);
      } else {
        setBrowseError(result.error ?? "ClawHub is unreachable");
        setHasMoreSkills(false);
      }
    },
    [filterCleanOnly, clawSort],
  );

  const runPluginsSearch = useCallback(async (q: string, loadMore = false) => {
    if (loadMore) {
      if (!pluginCursorRef.current) return;
      setLoadingMore(true);
    } else {
      pluginCursorRef.current = null;
      setBrowseLoading(true);
    }
    setBrowseError(null);
    setResults(null);
    const cursor = loadMore ? (pluginCursorRef.current ?? undefined) : undefined;
    const result = await searchClawHubPlugins(q.trim(), {
      limit: SEARCH_PAGE_SIZE,
      cursor,
      timeoutMs: Math.min(120_000, 22_000 + SEARCH_PAGE_SIZE * 500),
    });
    if (loadMore) setLoadingMore(false);
    else setBrowseLoading(false);
    if (result.items) {
      const nextCur = result.nextCursor?.trim() || null;
      pluginCursorRef.current = nextCur;
      if (loadMore) {
        setPluginResults((prev) => [...(prev ?? []), ...result.items!]);
      } else {
        setPluginResults(result.items);
      }
      setHasMorePlugins(nextCur != null && nextCur.length > 0);
    } else {
      setBrowseError(result.error ?? "ClawHub plugins unreachable");
      setHasMorePlugins(false);
    }
  }, []);

  /** Clean / sort -> re-run when a query is already set. */
  useEffect(() => {
    if (clawRegistry !== "skills") return;
    const q = queryRef.current.trim();
    if (!q) return;
    void runSkillsSearch(q, SEARCH_PAGE_SIZE);
  }, [clawRegistry, filterCleanOnly, clawSort, runSkillsSearch]);

  /** Plugins: first page loads when opening this tab (uses current query; empty = full catalog). */
  useEffect(() => {
    if (clawRegistry !== "plugins") return;
    if (pluginResults !== null) return;
    void runPluginsSearch(queryRef.current.trim(), false);
  }, [clawRegistry, pluginResults, runPluginsSearch]);

  const visiblePlugins = useMemo(() => {
    if (!pluginResults) return null;
    const t = pluginTagFilter.trim().toLowerCase();
    if (!t) return pluginResults;
    return pluginResults.filter((p) =>
      p.capabilityTags.some((tag) => tag.toLowerCase().includes(t)),
    );
  }, [pluginResults, pluginTagFilter]);

  const runSearch = async (q: string) => {
    if (clawRegistry === "skills") {
      const trimmed = q.trim();
      if (!trimmed) return;
      await runSkillsSearch(trimmed, SEARCH_PAGE_SIZE);
    } else {
      await runPluginsSearch(q.trim(), false);
    }
  };

  const loadMore = useCallback(async () => {
    if (browseLoading || loadingMore) return;
    if (clawRegistry === "skills") {
      const q = queryRef.current.trim();
      if (!q) return;
      if (!hasMoreSkills || !results?.length) return;
      const nextLimit = results.length + SEARCH_PAGE_SIZE;
      await runSkillsSearch(q, nextLimit, true);
      return;
    }
    if (!hasMorePlugins) return;
    await runPluginsSearch(queryRef.current.trim(), true);
  }, [
    browseLoading,
    clawRegistry,
    hasMorePlugins,
    hasMoreSkills,
    loadingMore,
    results,
    runPluginsSearch,
    runSkillsSearch,
  ]);

  useEffect(() => {
    const root = listViewportRef.current;
    const target = loadMoreRef.current;
    if (!root || !target) return;
    const obs = new IntersectionObserver(
      (entries) => {
        const first = entries[0];
        if (!first?.isIntersecting) return;
        void loadMore();
      },
      { root, threshold: 0.1 },
    );
    obs.observe(target);
    return () => obs.disconnect();
  }, [loadMore, results, visiblePlugins, clawRegistry, hasMoreSkills, hasMorePlugins]);

  const handleInstall = async (entry: ClawHubSkill) => {
    setInstallingSlug(entry.slug);
    const result = await installClawHubSkill(entry.slug);
    setInstallingSlug(null);
    if (result.ok) {
      onAfterSkillInstall(result.skill?.slug ?? entry.slug);
    } else {
      setBrowseError(result.error ?? "Install failed");
    }
  };

  return (
    <div className="mt-4 min-w-0 overflow-hidden rounded-xl border border-white/10 bg-white/5 p-3">
      <div className="flex items-center justify-between gap-2">
        <p className="font-mono text-[11px] text-white/80">ClawHub</p>
        <button
          type="button"
          onClick={onClose}
          className="shrink-0 font-mono text-[11px] text-white/40 hover:text-white/70"
        >
          Close
        </button>
      </div>
      <p className="mt-1 font-mono text-[10px] leading-relaxed text-white/45">
        Skills install into your custom dir. Plugins are listed on ClawHub only — open there to
        install.
      </p>

      <div className="mt-2 flex max-w-md rounded-lg border border-white/10 p-0.5 font-mono text-[10px]">
        <button
          type="button"
          onClick={() => {
            setClawRegistry("skills");
            setResults(null);
            setPluginResults(null);
            pluginCursorRef.current = null;
            setHasMoreSkills(false);
            setHasMorePlugins(false);
            setBrowseError(null);
          }}
          className={`min-w-0 flex-1 rounded-md py-1 transition ${
            clawRegistry === "skills"
              ? "bg-white/12 text-white"
              : "text-white/50 hover:text-white/75"
          }`}
        >
          Skills
        </button>
        <button
          type="button"
          onClick={() => {
            setClawRegistry("plugins");
            setResults(null);
            setPluginResults(null);
            pluginCursorRef.current = null;
            setHasMoreSkills(false);
            setHasMorePlugins(false);
            setBrowseError(null);
          }}
          className={`min-w-0 flex-1 rounded-md py-1 transition ${
            clawRegistry === "plugins"
              ? "bg-white/12 text-white"
              : "text-white/50 hover:text-white/75"
          }`}
        >
          Plugins
        </button>
      </div>

      <form
        onSubmit={(e) => {
          e.preventDefault();
          void runSearch(query);
        }}
        className="mt-2 flex min-w-0 flex-wrap gap-2"
      >
        <input
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder={
            clawRegistry === "skills" ? "slack, weather, trello…" : "filter catalog (optional)…"
          }
          className="min-w-0 flex-1 basis-40 rounded-lg border border-white/10 bg-black/20 px-2 py-1 font-mono text-xs text-white outline-none focus:border-emerald-300/40"
        />
        <button
          type="submit"
          disabled={browseLoading || (clawRegistry === "skills" && !query.trim())}
          className="shrink-0 rounded-lg border border-white/15 bg-white/10 px-3 py-1 font-mono text-[11px] text-white/90 transition hover:bg-white/20 disabled:opacity-40"
        >
          {browseLoading ? "Searching…" : clawRegistry === "plugins" ? "Apply" : "Search"}
        </button>
      </form>

      {clawRegistry === "skills" ? (
        <div className="mt-2 space-y-1.5">
          <div className="flex min-w-0 flex-col gap-2 sm:flex-row sm:flex-wrap sm:items-center">
            <div className="flex min-w-0 flex-wrap items-center gap-1.5">
              <FilterChip
                active={filterCleanOnly}
                label="Clean only"
                title="ClawHub ?cleanOnly=true — clean-only list when supported"
                onToggle={() => setFilterCleanOnly((v) => !v)}
              />
            </div>
            <div className="flex min-w-0 flex-wrap items-center gap-2 sm:ml-auto">
              <label className="flex items-center gap-1.5 font-mono text-[10px] text-white/50">
                <span className="shrink-0">Sort</span>
                <select
                  value={clawSort}
                  onChange={(e) => setClawSort(e.target.value)}
                  className="max-w-[9.5rem] rounded-md border border-white/10 bg-black/25 py-0.5 pr-1 pl-1 font-mono text-[10px] text-white outline-none focus:border-emerald-300/35"
                >
                  {CLAW_SORT_OPTIONS.map((o) => (
                    <option key={o.value} value={o.value}>
                      {o.label}
                    </option>
                  ))}
                </select>
              </label>
            </div>
          </div>
          <p className="max-w-xl font-mono text-[9px] leading-snug text-white/32">
            Registry filter here is clean only. Search uses ClawHub defaults for highlighted and
            non-suspicious. The Highlighted badge in a row appears when the skill page includes it.
          </p>
        </div>
      ) : (
        <div className="mt-2">
          <input
            value={pluginTagFilter}
            onChange={(e) => setPluginTagFilter(e.target.value)}
            placeholder="Filter by tag (e.g. executes-code)"
            title="Client-side only — plugin API has no tag filter param"
            className="w-full max-w-sm rounded-md border border-white/10 bg-black/20 px-2 py-1 font-mono text-[10px] text-white outline-none placeholder:text-white/35 focus:border-emerald-300/35"
          />
          <p className="mt-1 font-mono text-[9px] text-white/35">
            Tag filter runs in the app (no ClawHub plugins URL for tags).
          </p>
        </div>
      )}

      {browseError && (
        <p className="mt-3 font-mono text-[11px] text-rose-300" role="alert">
          {browseError}
        </p>
      )}

      {clawRegistry === "skills" && results && results.length === 0 && !browseLoading && (
        <p className="mt-3 subtle-copy">No matches.</p>
      )}
      {clawRegistry === "skills" && results && results.length > 0 && (
        <p className="mt-2 font-mono text-[10px] text-white/40">
          {results.length} skill{results.length === 1 ? "" : "s"} shown
          <span className="text-white/32"> · author &amp; stats from ClawHub when available</span>
        </p>
      )}
      {clawRegistry === "skills" && results && results.length > 0 && (
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
              {results.map((entry) => {
                const installsSub = formatClawHubStatsInstalls(entry);
                return (
                  <div
                    key={entry.slug}
                    role="row"
                    className="min-w-[44rem] px-3 py-2.5 md:grid md:grid-cols-[minmax(0,1.1fr)_minmax(0,2.2fr)_5.5rem_minmax(0,7.5rem)_5.5rem] md:items-start md:gap-x-2 md:py-2"
                  >
                    <div className="min-w-0 md:pt-0.5">
                      <p className="break-words text-sm font-semibold text-white">
                        {entry.displayName}
                      </p>
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
                          <span className="font-mono text-[9px] text-white/50">
                            v{entry.version}
                          </span>
                        )}
                      </div>
                      <p className="mt-0.5 break-all font-mono text-[9px] text-white/35">
                        {entry.slug}
                      </p>
                      {(entry.updatedAt != null ||
                        (entry.score != null && Number.isFinite(entry.score))) && (
                        <p className="mt-1 font-mono text-[8px] leading-relaxed text-white/32">
                          {entry.updatedAt != null && (
                            <>Upd {formatClawHubUpdated(entry.updatedAt)}</>
                          )}
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
                      <p className="break-words text-[11px] leading-snug text-(--mid)">
                        {entry.summary}
                      </p>
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
                        onClick={() => void handleInstall(entry)}
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
              })}
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
      )}

      {clawRegistry === "plugins" &&
        visiblePlugins &&
        visiblePlugins.length === 0 &&
        !browseLoading && (
          <p className="mt-3 subtle-copy">
            {pluginResults?.length ? "No plugins match this tag filter." : "No matches."}
          </p>
        )}
      {clawRegistry === "plugins" && pluginResults && (
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
      {clawRegistry === "plugins" && visiblePlugins && visiblePlugins.length > 0 && (
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
    </div>
  );
}
