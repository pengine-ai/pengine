/**
 * Mirrors `tool_engine::service::sanitize_mount_label` + `workspace_app_bind_pairs`
 * so UI previews match container `/app/<label>` paths.
 */
function sanitizeMountLabel(name: string): string {
  const s = [...name]
    .map((ch) => (/\p{L}/u.test(ch) || /\p{N}/u.test(ch) || ch === "-" || ch === "_" ? ch : "_"))
    .join("");
  if (s.length === 0 || [...s].every((c) => c === "_")) {
    return "folder";
  }
  return s;
}

/** Same basename rule as Rust `Path::new(h.trim()).file_name()`. */
function fileNameFromHostPath(host: string): string {
  const t = host.trim().replace(/\\/g, "/");
  const parts = t.split("/").filter(Boolean);
  if (parts.length === 0) {
    return "folder";
  }
  return parts[parts.length - 1] || "folder";
}

/** Container mount paths only (`/app/...`), same order and labels as the backend. */
export function workspaceAppContainerMountPaths(hostPaths: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const h of hostPaths) {
    const label = sanitizeMountLabel(fileNameFromHostPath(h));
    let key = label;
    let n = 0;
    while (seen.has(key)) {
      n += 1;
      key = `${label}_${n}`;
    }
    seen.add(key);
    out.push(`/app/${key}`);
  }
  return out;
}
