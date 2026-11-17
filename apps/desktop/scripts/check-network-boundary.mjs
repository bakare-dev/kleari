/* global process, console */
// Fails when Kleari could reach the internet other than to check for and download updates
// (docs/network.md). Kleari's promise is that the update check is its only request; this
// keeps every way of making another one out of the code. It reads files only, so it runs
// anywhere (`pnpm check:network`, and in CI's frontend job):
// 1. Rust dependencies: the only crates that can speak HTTP or open internet connections are
//    reached through tauri-plugin-updater, and through nothing else (`cargo metadata` for
//    macOS; this part needs Cargo, so the whole check runs in CI's Rust job).
// 2. Tauri plugins: tauri-plugin-updater is the only one, and no capability grants a plugin
//    that could fetch, open a socket or run a shell.
// 3. The updater's endpoints are Kleari's own GitHub releases, over HTTPS.
// 4. Rust source: no internet sockets or native networking APIs (a local Unix socket, as the
//    Docker check uses, is fine); every programme it runs is one of a fixed few, and no
//    AppleScript it runs fetches anything; every web address written in it is Kleari's own
//    GitHub, Apple's plist identifier (never fetched) or a reserved test domain.
// 5. Frontend source: no fetch, XHR, WebSocket, EventSource, beacon or HTTP library, and
//    every web address is Kleari's own GitHub.
// 6. The content security policy blocks every outside source, names connect-src explicitly,
//    and allows nothing beyond the app itself, its IPC and its local asset protocol.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { spawnSync } from "node:child_process";
import { join, relative } from "node:path";

const desktop = join(import.meta.dirname, "..");
const tauriDir = join(desktop, "src-tauri");
const problems = [];
const fail = (message) => problems.push(message);

/** Every file under `dir` with one of `extensions`, skipping build output. */
function files(dir, extensions) {
  const found = [];
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry === "dist" || entry.startsWith(".")) {
      continue;
    }
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      found.push(...files(path, extensions));
    } else if (extensions.some((extension) => entry.endsWith(extension))) {
      found.push(path);
    }
  }
  return found;
}

/** Each match of `pattern` in `text`, with its 1-based line. */
function matches(text, pattern) {
  const found = [];
  for (const [index, line] of text.split("\n").entries()) {
    for (const match of line.matchAll(pattern)) {
      found.push({ line: index + 1, text: match[0] });
    }
  }
  return found;
}

// 1. Rust dependencies, as Cargo resolves them for macOS (Kleari's only platform; Cargo.lock
//    also lists other platforms' dependencies, such as tauri's reqwest for Android and iOS):
//    what kleari reaches through normal and build dependencies without going through the
//    updater. Needs Cargo, with the dependencies already fetched (CI's Rust job).
const NETWORK_CRATES = [
  "reqwest", "hyper", "h2", "h3", "quinn", "ureq", "isahc", "curl", "curl-sys", "surf", "attohttpc",
  "tungstenite", "tokio-tungstenite", "async-tungstenite", "websocket", "native-tls", "openssl",
  "rustls", "tauri-plugin-http", "tauri-plugin-websocket", "tauri-plugin-upload",
];
const UPDATER = "tauri-plugin-updater";
const MAC_TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
for (const target of MAC_TARGETS) {
  const run = spawnSync(
    "cargo",
    ["metadata", "--locked", "--format-version", "1", "--filter-platform", target, "--manifest-path", join(tauriDir, "Cargo.toml"), ...(process.env.CARGO_NET_OFFLINE === "true" ? ["--offline"] : [])],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (run.status !== 0) {
    fail(`cargo metadata for ${target} failed: ${(run.stderr || run.error?.message || "").trim()}`);
    continue;
  }
  const metadata = JSON.parse(run.stdout);
  const nameOf = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const root = metadata.packages.find((pkg) => pkg.name === "kleari")?.id;
  if (root === undefined) {
    fail("cargo metadata has no kleari package; update this check.");
    continue;
  }
  const reached = new Set();
  const queue = [root];
  while (queue.length > 0) {
    const id = queue.pop();
    if (reached.has(id) || nameOf.get(id) === UPDATER) {
      continue;
    }
    reached.add(id);
    for (const dep of nodes.get(id)?.deps ?? []) {
      // Dev-dependencies (tests only) never ship.
      if (dep.dep_kinds.some((kind) => kind.kind !== "dev")) {
        queue.push(dep.pkg);
      }
    }
  }
  for (const id of reached) {
    if (NETWORK_CRATES.includes(nameOf.get(id))) {
      fail(`Rust (${target}): kleari reaches the networking crate "${nameOf.get(id)}" other than through ${UPDATER}.`);
    }
  }
}

// 2. Tauri plugins and capabilities.
const cargoToml = readFileSync(join(tauriDir, "Cargo.toml"), "utf8");
const plugins = [...cargoToml.matchAll(/^(tauri-plugin-[a-z0-9-]+)\s*=/gm)].map((match) => match[1]);
for (const plugin of plugins) {
  if (plugin !== UPDATER) {
    fail(`Cargo.toml: the Tauri plugin ${plugin} isn't allowed (only ${UPDATER}); see docs/network.md.`);
  }
}
const FORBIDDEN_PERMISSIONS = /^(http|websocket|upload|shell|opener|process|fs|deep-link):/;
for (const path of files(join(tauriDir, "capabilities"), [".json"])) {
  const capability = JSON.parse(readFileSync(path, "utf8"));
  for (const permission of capability.permissions ?? []) {
    const id = typeof permission === "string" ? permission : permission.identifier;
    if (typeof id === "string" && FORBIDDEN_PERMISSIONS.test(id)) {
      fail(`${relative(desktop, path)}: the permission "${id}" could reach the network or run programmes.`);
    }
  }
}

// 3. The updater's endpoints.
const conf = JSON.parse(readFileSync(join(tauriDir, "tauri.conf.json"), "utf8"));
const KLEARI_RELEASES = "https://github.com/bakare-dev/kleari/releases/";
for (const endpoint of conf.plugins?.updater?.endpoints ?? []) {
  if (!endpoint.startsWith(KLEARI_RELEASES)) {
    fail(`tauri.conf.json: the updater endpoint ${endpoint} isn't one of Kleari's GitHub releases.`);
  }
}

// 4. Rust source.
const RUST_NETWORK = /\bstd::net\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|\btokio::net\b|\bNSURLSession\b|\bNSURLConnection\b|\bCFSocket\w*|\bCFStream\w*|\bnw_connection\w*|\breqwest\b|\bureq\b|\bhyper\b/g;
const PROGRAMMES = new Set(["/usr/bin/osascript", "/usr/sbin/system_profiler", "/bin/ps", "/usr/bin/tmutil", "/usr/bin/tccutil"]);
const SCRIPT_NETWORK = /\b(curl|wget|nc|ncat|ssh|scp|sftp|ftp|telnet|rsync)\b|open location|https?:\/\//;
// Web addresses Rust may contain: Kleari's own GitHub (the update feed and release pages),
// Apple's plist identifier (a DOCTYPE, never fetched) and reserved test domains (RFC 2606).
const RUST_HOSTS = /^(github\.com\/bakare-dev\/kleari\/|www\.apple\.com\/DTDs\/|([a-z0-9-]+\.)*example(\.(com|org|net))?([:/]|$)|[a-z]+@host\.example)/;
for (const path of files(join(tauriDir, "src"), [".rs"])) {
  const name = relative(desktop, path);
  const text = readFileSync(path, "utf8");
  for (const found of matches(text, RUST_NETWORK)) {
    fail(`${name}:${found.line}: "${found.text}" could open a network connection.`);
  }
  // Test-only files may run other programmes (to set up files): they never ship.
  const testOnly = /(^|\/)tests?\.rs$/.test(name);
  for (const found of testOnly ? [] : matches(text, /Command::new\(([^)]*)\)/g)) {
    const argument = found.text.slice("Command::new(".length, -1).trim();
    const constant = new RegExp(String.raw`const ${argument}: &str = "([^"]+)"`).exec(text)?.[1];
    const programme = constant ?? /^"([^"]+)"$/.exec(argument)?.[1];
    if (programme === undefined || !PROGRAMMES.has(programme)) {
      fail(`${name}:${found.line}: runs ${argument}, which isn't one of the programmes this check allows.`);
    }
  }
  for (const found of matches(text, /do shell script "[^"]*"/g)) {
    if (SCRIPT_NETWORK.test(found.text)) {
      fail(`${name}:${found.line}: an AppleScript shell command could reach the network: ${found.text}`);
    }
  }
  const lines = text.split("\n");
  for (const found of matches(text, /https?:\/\/[^\s"'`)\\]+/g)) {
    // A test's deliberately wrong address is marked on its line, so each one is visible.
    if (lines[found.line - 1].includes("never requested")) {
      continue;
    }
    if (!RUST_HOSTS.test(found.text.replace(/^https?:\/\//, ""))) {
      fail(`${name}:${found.line}: ${found.text} isn't an address this check allows.`);
    }
  }
}

// 5. Frontend source and dependencies.
const WEB_NETWORK = /\bfetch\s*\(|\bXMLHttpRequest\b|\bWebSocket\b|\bEventSource\b|\bsendBeacon\b|\bnew\s+Image\s*\(|@tauri-apps\/plugin-(http|websocket|upload|shell|opener)\b/g;
for (const path of files(join(desktop, "src"), [".ts", ".tsx", ".js", ".jsx", ".css", ".html"])) {
  const name = relative(desktop, path);
  const text = readFileSync(path, "utf8");
  for (const found of matches(text, WEB_NETWORK)) {
    fail(`${name}:${found.line}: "${found.text}" could make a network request.`);
  }
  for (const found of matches(text, /https?:\/\/[^\s"'`)\\]+/g)) {
    if (!found.text.startsWith("https://github.com/bakare-dev/kleari")) {
      fail(`${name}:${found.line}: ${found.text} isn't Kleari's own GitHub.`);
    }
  }
}
const indexHtml = readFileSync(join(desktop, "index.html"), "utf8");
for (const found of matches(indexHtml, /(src|href)\s*=\s*"https?:\/\/[^"]*"/g)) {
  fail(`index.html:${found.line}: loads ${found.text} from outside the app.`);
}
const pkg = JSON.parse(readFileSync(join(desktop, "package.json"), "utf8"));
const HTTP_PACKAGES = /^(axios|ky|got|node-fetch|cross-fetch|isomorphic-fetch|undici|superagent|socket\.io-client|ws|@tauri-apps\/plugin-(http|websocket|upload|shell|opener))$/;
for (const dependency of Object.keys(pkg.dependencies ?? {})) {
  if (HTTP_PACKAGES.test(dependency)) {
    fail(`package.json: the dependency ${dependency} makes network requests.`);
  }
}

// 6. The content security policy.
const csp = conf.app?.security?.csp;
if (typeof csp !== "string") {
  fail("tauri.conf.json: app.security.csp must be a string that blocks outside sources.");
} else {
  const ALLOWED_SOURCES = new Set(["'self'", "'none'", "'unsafe-inline'", "asset:", "http://asset.localhost", "data:", "ipc:", "http://ipc.localhost"]);
  const directives = new Map(
    csp.split(";").map((part) => part.trim()).filter(Boolean).map((part) => {
      const [directive, ...sources] = part.split(/\s+/);
      return [directive, sources];
    }),
  );
  for (const required of ["default-src", "connect-src"]) {
    if (!directives.has(required)) {
      fail(`tauri.conf.json: the content security policy must set ${required}.`);
    }
  }
  for (const [directive, sources] of directives) {
    for (const source of sources) {
      if (!ALLOWED_SOURCES.has(source)) {
        fail(`tauri.conf.json: the content security policy's ${directive} allows ${source}.`);
      }
    }
  }
  if ((directives.get("script-src") ?? directives.get("default-src") ?? []).includes("'unsafe-inline'")) {
    fail("tauri.conf.json: scripts must not allow 'unsafe-inline'.");
  }
}

if (problems.length > 0) {
  console.error(`Kleari could reach the network beyond its update check (docs/network.md):\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log("Network boundary: only the update check can reach the internet.");
