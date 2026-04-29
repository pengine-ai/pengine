#!/usr/bin/env node
/**
 * Smoke-test @modelcontextprotocol/server-filesystem (same package as Pengine File Manager).
 *
 * Host (npm package on your machine):
 *   cd tools/mcp-probe-filemanager && npm install && node probe.mjs [directory]
 *
 * Same Docker image as Tool Engine (matches `podman_run_argv_for_tool` one-mount layout):
 *   ./tools/build-local-images.sh   # tag: ghcr.io/pengine-ai/pengine-file-manager:0.1.0
 *   MCP_PROBE_IN_CONTAINER=1 node probe.mjs /path/on/host
 *   # or: ./probe-in-image.sh /path/on/host
 *
 * Optional: PENGINE_CONTAINER_RUNTIME=docker|podman, MCP_FILE_MANAGER_IMAGE=...
 *
 * Uses fs.realpathSync so macOS /tmp → /private/tmp matches the server allowlist.
 *
 * Note: Pengine’s Rust-side `excludePatterns` merge for `directory_tree` applies only when
 * tools run through the app — this probe only validates the MCP server + container wiring.
 */
import { Client } from "@modelcontextprotocol/sdk/client/index.js";
import { StdioClientTransport } from "@modelcontextprotocol/sdk/client/stdio.js";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const rawRoot = path.resolve(process.argv[2] ?? path.join(__dirname, "..", "..", "."));
let hostRoot;
try {
  hostRoot = fs.realpathSync(rawRoot);
} catch {
  console.error("Directory does not exist:", rawRoot);
  process.exit(1);
}

const inContainer = process.env.MCP_PROBE_IN_CONTAINER === "1";
const label = path.basename(hostRoot.replace(/\/$/, ""));
const containerRoot = `/app/${label}`;
/** Paths sent in MCP `tools/call` — `/app/…` inside Docker, host path when local node server. */
const base = inContainer ? containerRoot : hostRoot;

const serverJs = path.join(
  __dirname,
  "node_modules/@modelcontextprotocol/server-filesystem/dist/index.js",
);

let transport;
if (inContainer) {
  const runtime = process.env.PENGINE_CONTAINER_RUNTIME || "podman";
  const image =
    process.env.MCP_FILE_MANAGER_IMAGE ||
    "ghcr.io/pengine-ai/pengine-file-manager:0.1.0";
  console.error(`[probe] container mode: ${runtime} ${image}`);
  console.error(`[probe] bind ${hostRoot} -> ${containerRoot}`);
  transport = new StdioClientTransport({
    command: runtime,
    args: [
      "run",
      "--rm",
      "-i",
      "--cpus=0.5",
      "--memory=256m",
      `-v=${hostRoot}:${containerRoot}:rw`,
      image,
      containerRoot,
    ],
  });
} else {
  if (!fs.existsSync(serverJs)) {
    console.error(
      "Missing package. Run: npm install (in tools/mcp-probe-filemanager)",
    );
    process.exit(1);
  }
  transport = new StdioClientTransport({
    command: "node",
    args: [serverJs, hostRoot],
  });
}

const client = new Client({ name: "mcp-probe", version: "0.0.1" });
await client.connect(transport);

const { tools } = await client.listTools();
console.log("--- tools/list ---");
console.log("mode:", inContainer ? "docker/podman (file-manager image)" : "host node");
console.log("host directory:", hostRoot);
console.log("tool paths use:", base);
console.log("count:", tools.length);
for (const t of tools) {
  console.log(" -", t.name);
}

/** Created on the host before smoke calls; visible in-container at ${base}/… */
const READ_FIXTURE = "_mcp_probe_fixture.txt";

const preset = {
  read_file: { path: path.join(base, READ_FIXTURE) },
  read_text_file: { path: path.join(base, READ_FIXTURE) },
  read_media_file: { path: path.join(base, READ_FIXTURE) },
  read_multiple_files: { paths: [path.join(base, READ_FIXTURE)] },
  write_file: { path: path.join(base, "_probe_write.txt"), content: "probe" },
  create_directory: { path: path.join(base, "_probe_dir2") },
  list_directory: { path: base },
  list_directory_with_sizes: { path: base },
  move_file: {
    source: path.join(base, "_probe_write.txt"),
    destination: path.join(base, "_probe_moved.txt"),
  },
  search_files: { path: base, pattern: "**/*.txt" },
  directory_tree: {
    path: base,
    excludePatterns: ["**/node_modules/**", "**/.git/**", "**/target/**"],
  },
  get_file_info: { path: path.join(base, READ_FIXTURE) },
  list_allowed_directories: {},
};

console.log("\n--- tools/call (smoke) ---");
const fsp = await import("node:fs/promises");
await fsp.mkdir(hostRoot, { recursive: true });
await fsp.writeFile(
  path.join(hostRoot, READ_FIXTURE),
  "mcp probe fixture\n",
  "utf8",
);
await fsp.writeFile(path.join(hostRoot, "_probe_write.txt"), "probe", "utf8");

for (const t of tools) {
  const name = t.name;
  let args = preset[name];
  if (name === "edit_file") {
    const ep = path.join(hostRoot, "_probe_edit.txt");
    await fsp.writeFile(ep, "before\n", "utf8");
    args = {
      path: path.join(base, "_probe_edit.txt"),
      edits: [{ oldText: "before", newText: "after" }],
      dryRun: false,
    };
  }
  if (args === undefined) {
    console.log(name, "SKIP (add to preset in probe.mjs)");
    continue;
  }
  if (name === "read_media_file") {
    console.log(
      name,
      "SKIP (expects image/audio; use a real media path to test)",
    );
    continue;
  }
  try {
    const res = await client.callTool({ name, arguments: args });
    const text = (res.content || [])
      .map((c) => (c.type === "text" ? c.text : ""))
      .join("\n");
    const ok = !res.isError;
    console.log(
      name,
      ok ? "OK" : "ERR",
      (text || JSON.stringify(res)).slice(0, 140).replace(/\n/g, " "),
    );
  } catch (e) {
    console.log(name, "ERR", String(e));
  }
}

await transport.close();
process.exit(0);
