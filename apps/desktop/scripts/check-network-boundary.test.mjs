// Tests for check-network-boundary.mjs: a minimal app that passes, then one way at a time of
// reaching the internet, each of which must fail. Run: `node --test scripts/`.

import { strict as assert } from "node:assert";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, test } from "node:test";
import { checkNetworkBoundary } from "./check-network-boundary.mjs";

const CSP =
  "default-src 'self'; connect-src 'self' ipc: http://ipc.localhost; img-src 'self' asset: http://asset.localhost data:; style-src 'self' 'unsafe-inline'; script-src 'self'; object-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'";

/** A passing app's files, by path under apps/desktop. */
function passingApp() {
  return {
    "package.json": JSON.stringify({ dependencies: { react: "^19.1.0", "@tauri-apps/api": "^2.8.0" } }),
    "index.html": '<!doctype html><script type="module" src="/src/main.tsx"></script>',
    "src/main.tsx": 'const notes = "https://github.com/bakare-dev/kleari/releases";\n',
    "src-tauri/Cargo.toml": '[dependencies]\ntauri = "2"\ntauri-plugin-updater = "2"\n',
    "src-tauri/tauri.conf.json": JSON.stringify({
      app: { security: { csp: CSP } },
      plugins: { updater: { endpoints: ["https://github.com/bakare-dev/kleari/releases/latest/download/latest.json"] } },
    }),
    "src-tauri/capabilities/default.json": JSON.stringify({ identifier: "default", permissions: ["core:default", "allow-get-mouse"] }),
    "src-tauri/src/lib.rs": 'const PS: &str = "/bin/ps";\nfn run() { let _ = std::process::Command::new(PS); }\n',
    "src-tauri/src/browser.rs": 'const OSASCRIPT: &str = "/usr/bin/osascript";\nconst LIST: &str = "set addresses to URL of every tab of w";\n',
    "src-tauri/src/maintenance.rs":
      'const PURGE: &str = r#"do shell script "/usr/sbin/purge" with administrator privileges"#;\n' +
      '#[cfg(test)]\nmod tests {\n    fn shape(s: &str) -> bool {\n        s.starts_with("do shell script")\n    }\n}\n',
    "src-tauri/src/quarantine/mod.rs": "#[cfg(test)]\nmod tests;\n",
    "src-tauri/src/quarantine/tests.rs": 'fn f() { Command::new("/usr/bin/chflags"); }\n',
  };
}

/** `cargo metadata` for kleari depending on `deps` (name → its own deps). */
function metadata(direct, graph = {}) {
  const names = new Set(["kleari", ...direct, ...Object.keys(graph), ...Object.values(graph).flat()]);
  const id = (name) => `${name} 1.0.0`;
  const node = (name, deps) => ({ id: id(name), deps: deps.map((dep) => ({ pkg: id(dep), dep_kinds: [{ kind: null }] })) });
  return {
    packages: [...names].map((name) => ({ id: id(name), name })),
    resolve: { nodes: [node("kleari", direct), ...Object.entries(graph).map(([name, deps]) => node(name, deps))] },
  };
}
const updaterOnly = () => metadata(["tauri", "tauri-plugin-updater"], { "tauri-plugin-updater": ["reqwest"], reqwest: ["hyper", "rustls"], tauri: ["tokio"], tokio: ["socket2", "mio"] });

let root;
afterEach(() => rmSync(root, { recursive: true, force: true }));

/** The checker's problems for the passing app with `changes` applied (`null` removes a file). */
function problems(changes = {}, deps = updaterOnly) {
  root = mkdtempSync(join(tmpdir(), "kleari-network-"));
  for (const [path, text] of Object.entries({ ...passingApp(), ...changes })) {
    if (text === null) {
      continue;
    }
    mkdirSync(dirname(join(root, path)), { recursive: true });
    writeFileSync(join(root, path), text);
  }
  return checkNetworkBoundary(root, deps);
}

const fails = (changes, pattern, deps) => {
  const found = problems(changes, deps);
  assert.ok(found.some((message) => pattern.test(message)), `expected ${pattern}, got:\n${found.join("\n")}`);
};

test("the passing app passes: networking only through the updater, tokio's own sockets allowed", () => {
  assert.deepEqual(problems(), []);
});

test("a networking crate reached outside the updater fails", () => {
  fails({}, /reaches the networking crate "reqwest"/, () => metadata(["tauri", "tauri-plugin-updater", "reqwest"], { "tauri-plugin-updater": ["reqwest"] }));
  fails({}, /reaches the networking crate "ureq"/, () => metadata(["tauri", "some-sdk"], { "some-sdk": ["ureq"] }));
});

test("a direct dependency on a socket crate fails", () => {
  fails({}, /depends directly on the socket crate "socket2"/, () => metadata(["tauri", "socket2"]));
});

test("a dev-dependency never ships, so it may use networking", () => {
  const deps = () => {
    const meta = metadata(["tauri"]);
    meta.packages.push({ id: "ureq 1.0.0", name: "ureq" });
    meta.resolve.nodes[0].deps.push({ pkg: "ureq 1.0.0", dep_kinds: [{ kind: "dev" }] });
    return meta;
  };
  assert.deepEqual(problems({}, deps), []);
});

test("cargo metadata failing fails the check", () => {
  fails({}, /cargo metadata failed/, () => {
    throw new Error("cargo metadata failed: offline");
  });
});

test("another Tauri plugin fails", () => {
  fails({ "src-tauri/Cargo.toml": 'tauri = "2"\ntauri-plugin-updater = "2"\ntauri-plugin-http = "2"\n' }, /plugin tauri-plugin-http isn't allowed/);
});

test("a network or shell permission fails, in a JSON file, a list of capabilities, or inline", () => {
  fails({ "src-tauri/capabilities/default.json": JSON.stringify({ permissions: ["shell:allow-open"] }) }, /"shell:allow-open"/);
  fails({ "src-tauri/capabilities/more.json": JSON.stringify([{ permissions: [{ identifier: "http:default", allow: [] }] }]) }, /"http:default"/);
  const inline = JSON.stringify({
    app: { security: { csp: CSP, capabilities: ["default", { identifier: "x", permissions: ["opener:default"] }] } },
    plugins: { updater: { endpoints: [] } },
  });
  fails({ "src-tauri/tauri.conf.json": inline }, /inline capability.*"opener:default"/);
});

test("a capability file the check can't read fails closed", () => {
  fails({ "src-tauri/capabilities/extra.toml": 'identifier = "x"\npermissions = ["shell:default"]\n' }, /must be JSON/);
  fails({ "src-tauri/capabilities/broken.json": "{ not json" }, /not valid JSON/);
  fails({ "src-tauri/capabilities/odd.json": JSON.stringify({ identifier: "x" }) }, /can't read/);
});

test("an updater endpoint outside Kleari's releases fails", () => {
  const conf = JSON.stringify({ app: { security: { csp: CSP } }, plugins: { updater: { endpoints: ["https://evil.example/latest.json"] } } });
  fails({ "src-tauri/tauri.conf.json": conf }, /updater endpoint https:\/\/evil\.example/);
});

test("Rust sockets, networking APIs and external web views fail", () => {
  for (const code of ["use std::net::TcpStream;", "let s = tokio::net::TcpStream::connect(a);", "let s = socket2::Socket::new();", "NSURLSession::sharedSession()", "WebviewUrl::External(url)", "window.navigate(url)"]) {
    fails({ "src-tauri/src/extra.rs": `fn f() { ${code} }\n` }, /could open a network connection/);
  }
});

test("an unlisted program fails, except in a module compiled only for tests", () => {
  fails({ "src-tauri/src/extra.rs": 'fn f() { Command::new("/usr/bin/curl"); }\n' }, /runs "\/usr\/bin\/curl"/);
  // The passing app's quarantine/tests.rs runs chflags: its parent declares it `#[cfg(test)]`.
  assert.deepEqual(problems(), []);
  // Named tests.rs but compiled into the app: no exemption.
  fails({ "src-tauri/src/quarantine/mod.rs": "mod tests;\n" }, /quarantine\/tests\.rs:1: runs "\/usr\/bin\/chflags"/);
  fails({ "src-tauri/src/sync/tests.rs": 'fn f() { Command::new("/usr/bin/curl"); }\n', "src-tauri/src/sync/mod.rs": "pub mod tests;\n" }, /runs "\/usr\/bin\/curl"/);
  // The attribute commented out, or only inside a string: compiled into the app.
  for (const parent of ["// #[cfg(test)]\nmod tests;\n", "/* #[cfg(test)] */\nmod tests;\n", 'const S: &str = "\n#[cfg(test)]\nmod tests;";\nmod tests;\n']) {
    fails({ "src-tauri/src/quarantine/mod.rs": parent }, /quarantine\/tests\.rs:1: runs "\/usr\/bin\/chflags"/);
  }
});

test("braces in a test module's strings and comments can't hide the shipped code after it", () => {
  const maintenance =
    '#[cfg(test)]\nmod tests {\n    const OPEN: &str = "{";\n    const RAW: &str = r#"{{ "#;\n    const C: char = \'{\';\n    // {\n    /* { */\n    fn f<\'a>(s: &\'a str) -> &\'a str { s }\n}\n' +
    'const S: &str = r#"do shell script "/usr/bin/curl -s x""#;\n';
  fails({ "src-tauri/src/maintenance.rs": maintenance }, /maintenance\.rs:10: AppleScript runs a shell command this check hasn't reviewed/);
});

test("emoji and raw C strings in a test module can't hide the shipped code after it either", () => {
  const shipped = 'const S: &str = r#"do shell script "/usr/bin/curl -s x""#;\n';
  // An emoji (\u{1F600}: one character, two UTF-16 units) in a comment, a string and a char,
  // near the closing brace: written as an escape so the file itself stays plain text.
  const emoji = '#[cfg(test)]\nmod tests {\n    // \u{1F600} {\n    const E: &str = "\u{1F600} { \u{1F600}";\n    const C: char = \'\u{1F600}\';\n}\n' + shipped;
  fails({ "src-tauri/src/maintenance.rs": emoji }, /maintenance\.rs:7: AppleScript runs a shell command this check hasn't reviewed/);
  // A raw C string with a quote and a brace inside, and a C string.
  const raw = '#[cfg(test)]\nmod tests {\n    const R: &CStr = cr#"say "{" here"#;\n    const C: &CStr = c"{";\n}\n' + shipped;
  fails({ "src-tauri/src/maintenance.rs": raw }, /maintenance\.rs:6: AppleScript runs a shell command this check hasn't reviewed/);
});

test("AppleScript outside the reviewed files fails", () => {
  fails({ "src-tauri/src/extra.rs": 'const OSASCRIPT: &str = "/usr/bin/osascript";\n' }, /AppleScript outside the files reviewed/);
});

test("AppleScript that sends a browser somewhere fails, even with the address given at run time", () => {
  for (const script of [
    'tell application "Safari" to open location (item 1 of argv)',
    "set URL of front document to item 1 of argv",
    "make new tab at end of tabs of window 1",
    'do JavaScript "location.href = x" in document 1',
    'execute front window\'s active tab javascript "fetch(x)"',
  ]) {
    fails({ "src-tauri/src/browser.rs": `const S: &str = r#"${script}"#;\n` }, /sends a browser to an address/);
  }
});

test("AppleScript may run only the reviewed shell commands, exactly", () => {
  const maintenance = (script) => ({ "src-tauri/src/maintenance.rs": `const S: &str = r#"${script}"#;\n` });
  fails(maintenance('do shell script "/usr/bin/curl -s x"'), /shell command this check hasn't reviewed/);
  // Python opening a socket to an address put together at run time.
  fails(maintenance(`do shell script "/usr/bin/python3 -c 'import socket; socket.create_connection((h, 443))'"`), /hasn't reviewed/);
  // A command built at run time, or a reviewed one with something added.
  fails(maintenance("do shell script command"), /hasn't reviewed: do shell script$/);
  fails(maintenance('do shell script "/usr/sbin/purge; /usr/bin/nc x 80"'), /hasn't reviewed/);
  // The reviewed one passes, and a test module's mention of the command's shape is ignored.
  assert.deepEqual(problems(maintenance('do shell script "/usr/sbin/purge" with administrator privileges')), []);
});

test("AppleScript that reaches Cocoa, other scripts or JavaScript for Automation fails", () => {
  for (const script of ['use framework "Foundation"', "set u to current application's NSURL's URLWithString:x", "run script x", 'load script file "x.scpt"']) {
    fails({ "src-tauri/src/browser.rs": `const S: &str = r#"${script}"#;\n` }, /reaches Cocoa, other scripts or JavaScript/);
  }
  fails({ "src-tauri/src/browser.rs": 'fn f() { Command::new(OSASCRIPT).args(["-l", "JavaScript", "-e", s]); }\nconst OSASCRIPT: &str = "/usr/bin/osascript";\n' }, /JavaScript for Automation/);
});

test("a web address in Rust that isn't allowed fails, unless its line marks it as never requested", () => {
  fails({ "src-tauri/src/extra.rs": 'const U: &str = "https://telemetry.example.io/x";\n' }, /isn't an address this check allows/);
  assert.deepEqual(problems({ "src-tauri/src/extra.rs": 'const U: &str = "https://github.com/someone/x"; // never requested: must be refused\n' }), []);
  assert.deepEqual(problems({ "src-tauri/src/extra.rs": 'const U: &str = "https://mail.example.com/u/0";\n' }), []);
});

test("frontend requests, addresses and HTTP libraries fail", () => {
  for (const code of ['fetch("/x")', "new XMLHttpRequest()", 'new WebSocket("ws://x")', 'navigator.sendBeacon("/x")', 'import { fetch } from "@tauri-apps/plugin-http";']) {
    fails({ "src/extra.ts": `${code}\n` }, /could make a network request/);
  }
  fails({ "src/extra.css": ".a { background: url(https://cdn.example/x.png) }\n" }, /isn't Kleari's own GitHub/);
  fails({ "index.html": '<link rel="stylesheet" href="https://fonts.example/x.css">' }, /loads href/);
  fails({ "package.json": JSON.stringify({ dependencies: { axios: "^1" } }) }, /axios makes network requests/);
});

test("a content security policy that allows an outside source or drops a blocking directive fails", () => {
  const conf = (csp) => JSON.stringify({ app: { security: { csp } }, plugins: { updater: { endpoints: [] } } });
  fails({ "src-tauri/tauri.conf.json": conf(CSP.replace("connect-src 'self'", "connect-src 'self' https://evil.example")) }, /connect-src allows https:\/\/evil\.example/);
  fails({ "src-tauri/tauri.conf.json": conf(CSP.replace("; form-action 'none'", "")) }, /must set form-action 'none'/);
  fails({ "src-tauri/tauri.conf.json": conf(CSP.replace("object-src 'none'", "object-src 'self'")) }, /must set object-src 'none'/);
  fails({ "src-tauri/tauri.conf.json": conf(CSP.replace("; connect-src 'self' ipc: http://ipc.localhost", "")) }, /must set connect-src/);
  fails({ "src-tauri/tauri.conf.json": conf(CSP.replace("script-src 'self'", "script-src 'self' 'unsafe-inline'")) }, /'unsafe-inline'/);
  fails({ "src-tauri/tauri.conf.json": conf(undefined) }, /must be a string/);
});
