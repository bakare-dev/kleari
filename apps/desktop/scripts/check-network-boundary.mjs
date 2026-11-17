/* global process, console */
// Fails when Kleari could reach the internet other than to check for and download updates
// (docs/network.md). Kleari's promise is that the update check is its only request; this
// keeps every way of making another one out of the code. It changes nothing, and fails closed:
// anything it can't read or doesn't recognise is a failure, not a pass.
// `pnpm check:network`, run in CI's Rust job (part 1 needs Cargo's resolved dependencies);
// its own tests: `node --test scripts/check-network-boundary.test.mjs`.
//
// 1. Rust dependencies, as Cargo resolves them for macOS: the crates that speak HTTP or other
//    internet protocols are reached only through tauri-plugin-updater, and kleari depends
//    directly on no socket crate (tokio's own use of socket2 and mio is fine: Kleari's code
//    may not use `tokio::net`, part 4).
// 2. Tauri plugins and capabilities: the updater is the only plugin, and no capability, in any
//    file or inline in tauri.conf.json, grants a plugin that could fetch, open a socket or run
//    a program. A capability file in a format this check doesn't read is a failure.
// 3. The updater's endpoints are Kleari's own GitHub releases, over HTTPS.
// 4. Rust source: no internet sockets, native networking API or web view showing anything
//    but the app's own pages (`WebviewUrl::External`, `navigate`); every program it runs is one
//    of a fixed few (outside modules compiled only for tests); AppleScript only in the files
//    reviewed for it, running only the shell commands reviewed (exactly), never Cocoa, other
//    scripts or JavaScript for Automation, and never sending a browser anywhere; every web address written in it is
//    Kleari's own GitHub, Apple's plist identifier (never fetched) or a reserved test domain.
// 5. Frontend: no fetch, XHR, WebSocket, EventSource, beacon or HTTP library, no remote file in
//    index.html, and every web address is Kleari's own GitHub.
// 6. The content security policy: every source is the app itself, its IPC or its local asset
//    protocol, and objects, frames, forms and base URLs are blocked by name.

import { spawnSync } from "node:child_process";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative } from "node:path";
import { pathToFileURL } from "node:url";

/** Crates that speak HTTP or another internet protocol: reached only through the updater. */
export const NETWORK_CRATES = [
  "reqwest", "hyper", "h2", "h3", "quinn", "ureq", "isahc", "curl", "curl-sys", "surf", "attohttpc",
  "tungstenite", "tokio-tungstenite", "async-tungstenite", "websocket", "native-tls", "openssl",
  "rustls", "trust-dns-resolver", "hickory-resolver", "dns-lookup", "lettre", "ssh2", "libssh2-sys",
  "tauri-plugin-http", "tauri-plugin-websocket", "tauri-plugin-upload",
];
/** Crates that open sockets: never a direct dependency of kleari. */
export const SOCKET_CRATES = ["socket2", "mio", "net2", "nix", "polling", "async-io", "async-net", "smol", "pnet"];
const UPDATER = "tauri-plugin-updater";
const MAC_TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"];
const KLEARI_RELEASES = "https://github.com/bakare-dev/kleari/releases/";
const KLEARI_GITHUB = "https://github.com/bakare-dev/kleari";

/** Programs Rust may run, all Apple's and all local. */
const PROGRAMS = new Set(["/usr/bin/osascript", "/usr/sbin/system_profiler", "/bin/ps", "/usr/bin/tmutil", "/usr/bin/tccutil"]);
/** The only files that may run AppleScript: each script in them was reviewed (docs/network.md). */
const APPLESCRIPT_FILES = new Set(["src-tauri/src/browser.rs", "src-tauri/src/maintenance.rs"]);
const RUST_NETWORK =
  /\bstd::net\b|\bTcpStream\b|\bTcpListener\b|\bUdpSocket\b|\btokio::net\b|\bNSURLSession\b|\bNSURLConnection\b|\bNSURLDownload\b|\bCFSocket\w*|\bCFStream\w*|\bCFHTTP\w*|\bnw_connection\w*|\bnw_endpoint\w*|\bWKWebView\b|\bWebviewUrl::External\b|\.navigate\s*\(|\bload_url\b|\bsocket2\b|\bmio::net\b|\breqwest\b|\bureq\b|\bhyper\b/g;
/** AppleScript that sends a browser (or anything) to an address, or runs code in a page. */
const APPLESCRIPT_NAVIGATION = /\bopen location\b|\bset\s+(?:the\s+)?URL\b|\bmake new (?:tab|document|window)\b|\bdo JavaScript\b|\bexecute\b[^"\n]*\bjavascript\b|\bnavigate\b|\bGET URL\b/i;
/** The only shell commands AppleScript may run (Maintenance's tasks), exactly as written:
 * anything else, including a command built at run time, fails, so a new one gets reviewed. */
export const ALLOWED_SHELL_COMMANDS = new Set([
  "/usr/sbin/purge",
  "/usr/bin/dscacheutil -flushcache; /usr/bin/killall -HUP mDNSResponder",
  "/usr/bin/tmutil thinlocalsnapshots / 999999999999 4",
  "/usr/bin/mdutil -E /",
  "/usr/bin/find -x /Library/Caches -mindepth 1 -maxdepth 1 ! -name 'com.apple.*' -exec /bin/rm -rf {} +",
  "/usr/bin/find -x /private/var/log -type f -name '*.gz' -mtime +30 -delete; /usr/bin/find -x /private/var/log -type f -name '*.bz2' -mtime +30 -delete; /usr/bin/find -x /Library/Logs/DiagnosticReports -type f -mtime +30 -delete",
  "/bin/rm -rf '/Users/Deleted Users'",
]);
/** AppleScript that reaches beyond its own commands: Cocoa (AppleScriptObjC), other scripts,
 * or JavaScript for Automation (`osascript -l JavaScript`, which can call any framework). */
const APPLESCRIPT_ESCAPE = /\buse framework\b|\bcurrent application's\b|\b(?:run|load|store) script\b|"-l"|\bJavaScript\b(?!["\s]*in\b)/;
/** Web addresses Rust may contain: Kleari's GitHub (update feed, release pages), Apple's plist
 * identifier (a DOCTYPE, never fetched) and reserved test domains (RFC 2606). */
const RUST_HOSTS = /^(github\.com\/bakare-dev\/kleari\/|www\.apple\.com\/DTDs\/|([a-z0-9-]+\.)*example(\.(com|org|net))?([:/]|$)|[a-z]+@host\.example)/;
const WEB_NETWORK =
  /\bfetch\s*\(|\bXMLHttpRequest\b|\bWebSocket\b|\bEventSource\b|\bsendBeacon\b|\bnew\s+Image\s*\(|\bimportScripts\b|@tauri-apps\/plugin-(http|websocket|upload|shell|opener)\b/g;
const HTTP_PACKAGES =
  /^(axios|ky|got|node-fetch|cross-fetch|isomorphic-fetch|undici|superagent|socket\.io-client|ws|@tauri-apps\/plugin-(http|websocket|upload|shell|opener))$/;
const FORBIDDEN_PERMISSIONS = /^(http|websocket|upload|shell|opener|process|fs|deep-link):/;
/** Every source the content security policy may name. */
const CSP_SOURCES = new Set(["'self'", "'none'", "'unsafe-inline'", "asset:", "http://asset.localhost", "data:", "ipc:", "http://ipc.localhost"]);
/** Directives the policy must set to exactly this (form-action has no default-src fallback). */
const CSP_REQUIRED = { "default-src": "'self'", "object-src": "'none'", "frame-src": "'none'", "base-uri": "'none'", "form-action": "'none'" };

/**
 * Rust `text` with comments and the contents of strings and character literals replaced by
 * spaces (newlines kept, so lines and columns stay put): only code is left, for finding
 * attributes, modules and braces. Handles line and nested block comments, plain, byte and raw
 * strings (`r#"…"#`) and character literals, telling `'a'` from a lifetime `'a`.
 */
export function rustCode(text) {
  const out = [...text];
  const blank = (from, to) => {
    for (let at = from; at < to; at += 1) {
      if (out[at] !== "\n") {
        out[at] = " ";
      }
    }
  };
  const identifier = (ch) => ch !== undefined && /[A-Za-z0-9_]/.test(ch);
  let at = 0;
  while (at < text.length) {
    const ch = text[at];
    const next = text[at + 1];
    if (ch === "/" && next === "/") {
      const end = text.indexOf("\n", at);
      const stop = end < 0 ? text.length : end;
      blank(at, stop);
      at = stop;
    } else if (ch === "/" && next === "*") {
      let depth = 1;
      let end = at + 2;
      while (end < text.length && depth > 0) {
        if (text[end] === "/" && text[end + 1] === "*") {
          depth += 1;
          end += 2;
        } else if (text[end] === "*" && text[end + 1] === "/") {
          depth -= 1;
          end += 2;
        } else {
          end += 1;
        }
      }
      blank(at, end);
      at = end;
    } else if ((ch === "r" || (ch === "b" && next === "r")) && !identifier(text[at - 1])) {
      // A raw string: r"…", r#"…"#, br#"…"#; otherwise just an identifier starting with r.
      const start = ch === "b" ? at + 1 : at;
      let hashes = 0;
      while (text[start + 1 + hashes] === "#") {
        hashes += 1;
      }
      if (text[start + 1 + hashes] !== '"') {
        at += 1;
        continue;
      }
      const close = `"${"#".repeat(hashes)}`;
      const end = text.indexOf(close, start + 2 + hashes);
      const stop = end < 0 ? text.length : end + close.length;
      blank(at, stop);
      at = stop;
    } else if (ch === '"') {
      let end = at + 1;
      while (end < text.length && text[end] !== '"') {
        end += text[end] === "\\" ? 2 : 1;
      }
      blank(at, end + 1);
      at = end + 1;
    } else if (ch === "'" && (next === "\\" || text[at + 2] === "'")) {
      // A character literal ('x', '\n', '\u{1F600}'); a lifetime ('a) has no closing quote.
      let end = at + 1;
      while (end < text.length && text[end] !== "'") {
        end += text[end] === "\\" ? 2 : 1;
      }
      blank(at, end + 1);
      at = end + 1;
    } else {
      at += 1;
    }
  }
  return out.join("");
}

/** `text` with each inline `#[cfg(test)] mod … { … }` blanked (line numbers kept): code that
 * is only compiled for tests. The module's extent is found in `rustCode`, so braces in its
 * strings and comments can't stretch it over the code after it. */
function withoutInlineTests(text) {
  const lines = text.split("\n");
  const code = rustCode(text).split("\n");
  for (let index = 0; index < code.length; index += 1) {
    if (!/^\s*#\[cfg\(test\)\]\s*$/.test(code[index]) || !/^\s*(pub(\([^)]*\))?\s+)?mod\s+\w+\s*\{/.test(code[index + 1] ?? "")) {
      continue;
    }
    let depth = 0;
    for (let at = index + 1; at < code.length; at += 1) {
      depth += (code[at].match(/\{/g) ?? []).length - (code[at].match(/\}/g) ?? []).length;
      lines[at] = "";
      if (depth <= 0) {
        index = at;
        break;
      }
    }
  }
  return lines.join("\n");
}

/** Whether the Rust file `name` (under src-tauri/src) is a module its parent declares only for
 * tests: a real `#[cfg(test)]` attribute (in code, not a comment or string) directly before
 * `mod <file>;` in `mod.rs` or `<parent>.rs`. */
function testOnlyModule(srcDir, name, read) {
  const parts = name.replace(/^src-tauri\/src\//, "").replace(/\.rs$/, "").split("/");
  const module = parts.pop() === "mod" ? parts.pop() : name.replace(/^.*\//, "").replace(/\.rs$/, "");
  const parents = parts.length === 0 ? ["lib.rs", "main.rs"] : [`${parts.join("/")}/mod.rs`, `${parts.join("/")}.rs`];
  return parents.some((parent) => {
    let text;
    try {
      text = read(join(srcDir, parent));
    } catch {
      return false;
    }
    return new RegExp(String.raw`^\s*#\[cfg\(test\)\]\s*\n\s*mod\s+${module}\s*;`, "m").test(rustCode(text));
  });
}

/** Every file under `dir` with one of `extensions` (all files for `null`), skipping build output. */
function files(dir, extensions) {
  const found = [];
  for (const entry of readdirSync(dir)) {
    if (entry === "node_modules" || entry === "target" || entry === "dist" || entry.startsWith(".")) {
      continue;
    }
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      found.push(...files(path, extensions));
    } else if (extensions === null || extensions.some((extension) => entry.endsWith(extension))) {
      found.push(path);
    }
  }
  return found;
}

/** Each match of `pattern` (global) in `text`, with its 1-based line. */
function matches(text, pattern) {
  const found = [];
  for (const [index, line] of text.split("\n").entries()) {
    for (const match of line.matchAll(pattern)) {
      found.push({ line: index + 1, text: match[0] });
    }
  }
  return found;
}

/** `cargo metadata` for `target`, as Cargo resolves Kleari's dependencies there. */
export function cargoMetadata(tauriDir, target) {
  const run = spawnSync(
    "cargo",
    [
      "metadata", "--locked", "--format-version", "1", "--filter-platform", target,
      "--manifest-path", join(tauriDir, "Cargo.toml"),
      ...(process.env.CARGO_NET_OFFLINE === "true" ? ["--offline"] : []),
    ],
    { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
  );
  if (run.status !== 0) {
    throw new Error(`cargo metadata for ${target} failed: ${(run.stderr || run.error?.message || "").trim()}`);
  }
  return JSON.parse(run.stdout);
}

/** Part 1, for one target's metadata. */
function checkDependencies(metadata, target, fail) {
  const nameOf = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const nodes = new Map(metadata.resolve.nodes.map((node) => [node.id, node]));
  const root = metadata.packages.find((pkg) => pkg.name === "kleari")?.id;
  if (root === undefined) {
    fail("cargo metadata has no kleari package; update this check.");
    return;
  }
  // Dev-dependencies (tests only) never ship.
  const shipped = (dep) => dep.dep_kinds.some((kind) => kind.kind !== "dev");
  for (const dep of nodes.get(root)?.deps ?? []) {
    if (shipped(dep) && SOCKET_CRATES.includes(nameOf.get(dep.pkg))) {
      fail(`Rust (${target}): kleari depends directly on the socket crate "${nameOf.get(dep.pkg)}".`);
    }
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
      if (shipped(dep)) {
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

/** Part 2's check of one capability: every permission it grants. */
function checkCapability(capability, where, fail) {
  if (typeof capability !== "object" || capability === null || !Array.isArray(capability.permissions)) {
    fail(`${where}: a capability this check can't read (it needs a permissions list).`);
    return;
  }
  for (const permission of capability.permissions) {
    const id = typeof permission === "string" ? permission : permission?.identifier;
    if (typeof id !== "string") {
      fail(`${where}: a permission this check can't read.`);
    } else if (FORBIDDEN_PERMISSIONS.test(id)) {
      fail(`${where}: the permission "${id}" could reach the network or run programs.`);
    }
  }
}

/** Part 6. */
function checkCsp(csp, fail) {
  if (typeof csp !== "string") {
    fail("tauri.conf.json: app.security.csp must be a string that blocks outside sources.");
    return;
  }
  const directives = new Map();
  for (const part of csp.split(";").map((text) => text.trim()).filter(Boolean)) {
    const [directive, ...sources] = part.split(/\s+/);
    if (directives.has(directive)) {
      fail(`tauri.conf.json: the content security policy sets ${directive} twice.`);
    }
    directives.set(directive, sources);
  }
  for (const [directive, value] of Object.entries(CSP_REQUIRED)) {
    const sources = directives.get(directive);
    if (sources === undefined || sources.length !== 1 || sources[0] !== value) {
      fail(`tauri.conf.json: the content security policy must set ${directive} ${value}.`);
    }
  }
  if (!directives.has("connect-src")) {
    fail("tauri.conf.json: the content security policy must set connect-src.");
  }
  for (const [directive, sources] of directives) {
    for (const source of sources) {
      if (!CSP_SOURCES.has(source)) {
        fail(`tauri.conf.json: the content security policy's ${directive} allows ${source}.`);
      }
    }
  }
  if ((directives.get("script-src") ?? directives.get("default-src") ?? []).includes("'unsafe-inline'")) {
    fail("tauri.conf.json: scripts must not allow 'unsafe-inline'.");
  }
}

/**
 * Every way the app in `desktop` (apps/desktop) could reach the internet beyond its update
 * check, as messages; empty when there is none. `metadata(target)` gives `cargo metadata` for
 * a target (tests pass their own).
 */
export function checkNetworkBoundary(desktop, metadata = (target) => cargoMetadata(join(desktop, "src-tauri"), target)) {
  const tauriDir = join(desktop, "src-tauri");
  const problems = [];
  const fail = (message) => problems.push(message);
  const read = (path) => readFileSync(path, "utf8");

  // 1.
  for (const target of MAC_TARGETS) {
    try {
      checkDependencies(metadata(target), target, fail);
    } catch (error) {
      fail(error instanceof Error ? error.message : String(error));
    }
  }

  // 2.
  const cargoToml = read(join(tauriDir, "Cargo.toml"));
  for (const [, plugin] of cargoToml.matchAll(/^\s*(tauri-plugin-[a-z0-9-]+)\s*=/gm)) {
    if (plugin !== UPDATER) {
      fail(`Cargo.toml: the Tauri plugin ${plugin} isn't allowed (only ${UPDATER}); see docs/network.md.`);
    }
  }
  for (const path of files(join(tauriDir, "capabilities"), null)) {
    const where = relative(desktop, path);
    if (!path.endsWith(".json")) {
      fail(`${where}: capability files must be JSON so this check can read them.`);
      continue;
    }
    let parsed;
    try {
      parsed = JSON.parse(read(path));
    } catch {
      fail(`${where}: not valid JSON, so this check can't read it.`);
      continue;
    }
    // A file may hold one capability, or a list of them.
    for (const capability of Array.isArray(parsed) ? parsed : [parsed]) {
      checkCapability(capability, where, fail);
    }
  }
  const conf = JSON.parse(read(join(tauriDir, "tauri.conf.json")));
  for (const capability of conf.app?.security?.capabilities ?? []) {
    // A name refers to a file checked above; an object is a capability written inline.
    if (typeof capability !== "string") {
      checkCapability(capability, "tauri.conf.json (inline capability)", fail);
    }
  }

  // 3.
  for (const endpoint of conf.plugins?.updater?.endpoints ?? []) {
    if (typeof endpoint !== "string" || !endpoint.startsWith(KLEARI_RELEASES)) {
      fail(`tauri.conf.json: the updater endpoint ${endpoint} isn't one of Kleari's GitHub releases.`);
    }
  }

  // 4.
  for (const path of files(join(tauriDir, "src"), [".rs"])) {
    const name = relative(desktop, path);
    const text = read(path);
    const lines = text.split("\n");
    for (const found of matches(text, RUST_NETWORK)) {
      fail(`${name}:${found.line}: "${found.text}" could open a network connection.`);
    }
    // A module its parent compiles only for tests may run other programs (to set up files):
    // it never ships. Decided by that declaration, never by the file's name.
    if (!testOnlyModule(join(tauriDir, "src"), name, read)) {
      for (const found of matches(text, /Command::new\(([^)]*)\)/g)) {
        const argument = found.text.slice("Command::new(".length, -1).trim();
        const constant = new RegExp(String.raw`const ${argument}: &str = "([^"]+)"`).exec(text)?.[1];
        const program = constant ?? /^"([^"]+)"$/.exec(argument)?.[1];
        if (program === undefined || !PROGRAMS.has(program)) {
          fail(`${name}:${found.line}: runs ${argument}, which isn't one of the programs this check allows.`);
        }
      }
    }
    const appleScript = /\bosascript\b|\bNSAppleScript\b|\bOSAScript\b|\bOSAKit\b/.test(text);
    if (appleScript && !APPLESCRIPT_FILES.has(name)) {
      fail(`${name}: runs AppleScript outside the files reviewed for it (${[...APPLESCRIPT_FILES].join(", ")}).`);
    }
    const shipped = withoutInlineTests(text);
    for (const [index, line] of shipped.split("\n").entries()) {
      if (APPLESCRIPT_NAVIGATION.test(line)) {
        fail(`${name}:${index + 1}: AppleScript that sends a browser to an address or runs code in a page.`);
      }
      if (APPLESCRIPT_ESCAPE.test(line)) {
        fail(`${name}:${index + 1}: AppleScript that reaches Cocoa, other scripts or JavaScript for Automation.`);
      }
      for (const shell of line.matchAll(/do shell script\b(?:\s+"([^"]*)")?/g)) {
        if (shell[1] === undefined || !ALLOWED_SHELL_COMMANDS.has(shell[1])) {
          fail(`${name}:${index + 1}: AppleScript runs a shell command this check hasn't reviewed: ${shell[0]}`);
        }
      }
    }
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

  // 5.
  for (const path of files(join(desktop, "src"), [".ts", ".tsx", ".js", ".jsx", ".mjs", ".css", ".html"])) {
    const name = relative(desktop, path);
    const text = read(path);
    for (const found of matches(text, WEB_NETWORK)) {
      fail(`${name}:${found.line}: "${found.text}" could make a network request.`);
    }
    for (const found of matches(text, /(?:https?|wss?):\/\/[^\s"'`)\\]+/g)) {
      if (!found.text.startsWith(KLEARI_GITHUB)) {
        fail(`${name}:${found.line}: ${found.text} isn't Kleari's own GitHub.`);
      }
    }
  }
  for (const found of matches(read(join(desktop, "index.html")), /(src|href)\s*=\s*["']?(?:https?:)?\/\/[^"'\s>]*/g)) {
    fail(`index.html:${found.line}: loads ${found.text} from outside the app.`);
  }
  const pkg = JSON.parse(read(join(desktop, "package.json")));
  for (const dependency of Object.keys(pkg.dependencies ?? {})) {
    if (HTTP_PACKAGES.test(dependency)) {
      fail(`package.json: the dependency ${dependency} makes network requests.`);
    }
  }

  // 6.
  checkCsp(conf.app?.security?.csp, fail);
  return problems;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const problems = checkNetworkBoundary(join(import.meta.dirname, ".."));
  if (problems.length > 0) {
    console.error(`Kleari could reach the network beyond its update check (docs/network.md):\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
  console.log("Network boundary: only the update check can reach the internet.");
}
