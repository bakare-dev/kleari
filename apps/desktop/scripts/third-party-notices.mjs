/* global process, console */
// Writes THIRD_PARTY_NOTICES.md at the repository root: every Rust crate and npm package that
// ships inside Kleari, with its licence and the licence and notice files it comes with. It
// also checks each licence can be combined with Kleari's (GPL-3.0-or-later).
//
// `pnpm notices` rewrites the file; `pnpm check:notices` (CI's Rust job) fails if it is out of
// date or a licence isn't one this check accepts. Needs Cargo with dependencies fetched (as
// `cargo metadata` resolves them for macOS) and `pnpm install` (for node_modules).

import { createHash } from "node:crypto";
import { existsSync, readdirSync, readFileSync, realpathSync, writeFileSync } from "node:fs";
import { dirname, join, sep } from "node:path";
import { pathToFileURL } from "node:url";
import { cargoMetadata } from "./check-network-boundary.mjs";

const MAC_TARGETS = ["aarch64-apple-darwin", "x86_64-apple-darwin"];

/** Licences that can be combined with GPL-3.0-or-later in one program (SPDX identifiers). */
export const COMPATIBLE = new Set([
  "MIT", "MIT-0", "Apache-2.0", "BSD-2-Clause", "BSD-3-Clause", "ISC", "Zlib", "0BSD", "BSL-1.0",
  "Unicode-3.0", "Unicode-DFS-2016", "CC0-1.0", "Unlicense", "MPL-2.0", "CDLA-Permissive-2.0",
  "LGPL-2.1-or-later", "LGPL-3.0", "LGPL-3.0-only", "LGPL-3.0-or-later",
  "GPL-3.0", "GPL-3.0-only", "GPL-3.0-or-later", "GPL-2.0-or-later",
]);
/** `<licence> WITH <exception>` pairs that stay compatible. */
const COMPATIBLE_EXCEPTIONS = new Set(["LLVM-exception"]);

/**
 * Whether an SPDX licence expression can be satisfied with compatible licences: `OR` needs one
 * side, `AND` both, `WITH` a known exception; `/` (an old Cargo spelling of OR) counts as OR.
 * `null` for an expression this can't read, which is treated as not compatible.
 */
export function compatible(expression) {
  const tokens = expression.replaceAll("(", " ( ").replaceAll(")", " ) ").replaceAll("/", " OR ").split(/\s+/).filter(Boolean);
  let at = 0;
  const peek = () => tokens[at];
  function primary() {
    const token = tokens[at++];
    if (token === "(") {
      const inner = or();
      if (tokens[at++] !== ")") {
        throw new Error("unbalanced");
      }
      return inner;
    }
    if (token === undefined || token === ")" || /^(AND|OR|WITH)$/.test(token)) {
      throw new Error("unexpected");
    }
    const id = token.replace(/\+$/, "-or-later");
    if (peek() === "WITH") {
      at += 1;
      const exception = tokens[at++];
      return COMPATIBLE.has(id) && COMPATIBLE_EXCEPTIONS.has(exception ?? "");
    }
    return COMPATIBLE.has(id) || COMPATIBLE.has(token);
  }
  function and() {
    let value = primary();
    while (peek() === "AND") {
      at += 1;
      value = primary() && value;
    }
    return value;
  }
  function or() {
    let value = and();
    while (peek() === "OR") {
      at += 1;
      value = and() || value;
    }
    return value;
  }
  try {
    const value = or();
    return at === tokens.length ? value : null;
  } catch {
    return null;
  }
}

/** The licence and notice files at the top of a package's folder, by name. */
function licenceFiles(dir) {
  if (!existsSync(dir)) {
    return [];
  }
  return readdirSync(dir)
    .filter((name) => /^(licen[cs]e|copying|notice|copyright)([-._].*)?$/i.test(name))
    .sort()
    .map((name) => ({ name, text: readFileSync(join(dir, name), "utf8").replace(/\r\n/g, "\n").trim() }));
}

/** Rust crates kleari ships on macOS (normal and build dependencies, not dev ones). */
function rustPackages(metadata) {
  const shipped = new Map();
  for (const target of MAC_TARGETS) {
    const meta = metadata(target);
    const byId = new Map(meta.packages.map((pkg) => [pkg.id, pkg]));
    const nodes = new Map(meta.resolve.nodes.map((node) => [node.id, node]));
    const root = meta.packages.find((pkg) => pkg.name === "kleari")?.id;
    const queue = [root];
    const seen = new Set();
    while (queue.length > 0) {
      const id = queue.pop();
      if (id === undefined || seen.has(id)) {
        continue;
      }
      seen.add(id);
      if (id !== root) {
        const pkg = byId.get(id);
        shipped.set(`${pkg.name}@${pkg.version}`, {
          ecosystem: "Rust",
          name: pkg.name,
          version: pkg.version,
          license: pkg.license ?? null,
          repository: pkg.repository ?? null,
          files: licenceFiles(dirname(pkg.manifest_path)),
          fetched: existsSync(pkg.manifest_path),
        });
      }
      for (const dep of nodes.get(id)?.deps ?? []) {
        if (dep.dep_kinds.some((kind) => kind.kind !== "dev")) {
          queue.push(dep.pkg);
        }
      }
    }
  }
  return [...shipped.values()];
}

/** Where `name` resolves from a package in `dir`, as Node and pnpm lay node_modules out. */
function resolvePackage(dir, name) {
  for (let at = dir; ; at = dirname(at)) {
    for (const candidate of [join(at, "node_modules", name), at.endsWith(`${sep}node_modules`) ? join(at, name) : null]) {
      if (candidate && existsSync(join(candidate, "package.json"))) {
        return realpathSync(candidate);
      }
    }
    if (dirname(at) === at) {
      return null;
    }
  }
}

/** npm packages the interface ships: the desktop app's dependencies and theirs. */
function npmPackages(desktop) {
  const shipped = new Map();
  const own = JSON.parse(readFileSync(join(desktop, "package.json"), "utf8"));
  const queue = Object.keys(own.dependencies ?? {}).map((name) => ({ name, from: desktop }));
  const missing = [];
  while (queue.length > 0) {
    const { name, from } = queue.pop();
    const dir = resolvePackage(from, name);
    if (dir === null) {
      missing.push(name);
      continue;
    }
    const pkg = JSON.parse(readFileSync(join(dir, "package.json"), "utf8"));
    const key = `${pkg.name}@${pkg.version}`;
    if (shipped.has(key)) {
      continue;
    }
    const license = typeof pkg.license === "string" ? pkg.license : (pkg.license?.type ?? null);
    const repository = typeof pkg.repository === "string" ? pkg.repository : (pkg.repository?.url ?? null);
    shipped.set(key, { ecosystem: "npm", name: pkg.name, version: pkg.version, license, repository, files: licenceFiles(dir) });
    for (const dep of Object.keys({ ...pkg.dependencies, ...pkg.optionalDependencies })) {
      queue.push({ name: dep, from: dir });
    }
  }
  return { packages: [...shipped.values()], missing };
}

/** The notices file's text, and any problems (incompatible or unreadable licences). */
export function buildNotices(desktop, metadata) {
  const problems = [];
  const rust = rustPackages(metadata);
  const npm = npmPackages(desktop);
  for (const name of npm.missing) {
    problems.push(`npm: ${name} isn't installed (run pnpm install).`);
  }
  const all = [...rust, ...npm.packages].sort((a, b) =>
    a.ecosystem === b.ecosystem ? a.name.localeCompare(b.name) || a.version.localeCompare(b.version) : a.ecosystem.localeCompare(b.ecosystem),
  );
  for (const pkg of all) {
    if (pkg.fetched === false) {
      problems.push(`${pkg.ecosystem} ${pkg.name} ${pkg.version}: its source isn't downloaded (run cargo fetch --locked).`);
    } else if (pkg.license === null) {
      problems.push(`${pkg.ecosystem} ${pkg.name} ${pkg.version}: no licence stated.`);
    } else if (compatible(pkg.license) !== true) {
      problems.push(`${pkg.ecosystem} ${pkg.name} ${pkg.version}: "${pkg.license}" isn't a licence this check accepts with GPL-3.0-or-later.`);
    }
  }
  // Identical licence texts are written once, and each package names the ones it ships with.
  const texts = new Map();
  const refs = (pkg) =>
    pkg.files.map((file) => {
      const hash = createHash("sha256").update(file.text).digest("hex").slice(0, 12);
      if (!texts.has(hash)) {
        texts.set(hash, { text: file.text, users: [] });
      }
      texts.get(hash).users.push(`${pkg.name} ${pkg.version}`);
      return hash;
    });
  const rows = all.map((pkg) => {
    const ids = refs(pkg);
    const repo = pkg.repository ? pkg.repository.replace(/^git\+/, "").replace(/\.git$/, "") : "";
    const licence = ids.length > 0 ? ids.map((id) => `[${id}](#${id})`).join(", ") : "(the licence's standard text)";
    return `| ${pkg.ecosystem} | ${pkg.name} | ${pkg.version} | ${pkg.license ?? "none stated"} | ${licence} | ${repo} |`;
  });
  const sections = [...texts.entries()]
    .sort(([a], [b]) => a.localeCompare(b))
    .map(([hash, { text, users }]) => `### ${hash}\n\nShipped with: ${[...new Set(users)].join(", ")}.\n\n\`\`\`text\n${text.replaceAll("```", "'''")}\n\`\`\``);
  const text = [
    "# Third-party notices",
    "",
    "Kleari is free software under the GNU General Public License v3.0 or later (see [LICENSE](LICENSE)),",
    "© 2026 the Kleari contributors. It includes the open-source software listed below, each under its",
    "own licence. This file is generated from the packages that ship in the app (`pnpm notices`);",
    "CI fails if it is out of date or a licence can't be combined with Kleari's.",
    "",
    `${rust.length} Rust crates and ${npm.packages.length} npm packages.`,
    "",
    "| Ecosystem | Package | Version | Licence | Licence files | Source |",
    "|---|---|---|---|---|---|",
    ...rows,
    "",
    "## Licence and notice files",
    "",
    "Each text below is reproduced as the package ships it; packages with no such file are under",
    "the standard text of the licence named above.",
    "",
    sections.join("\n\n"),
    "",
  ].join("\n");
  return { text, problems };
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  const desktop = join(import.meta.dirname, "..");
  const out = join(desktop, "..", "..", "THIRD_PARTY_NOTICES.md");
  const { text, problems } = buildNotices(desktop, (target) => cargoMetadata(join(desktop, "src-tauri"), target));
  if (problems.length > 0) {
    console.error(`Third-party licences need attention:\n  ${problems.join("\n  ")}`);
    process.exit(1);
  }
  if (process.argv.includes("--check")) {
    const current = existsSync(out) ? readFileSync(out, "utf8") : "";
    if (current !== text) {
      console.error("THIRD_PARTY_NOTICES.md is out of date: run `pnpm notices` and commit it.");
      process.exit(1);
    }
    console.log("THIRD_PARTY_NOTICES.md is up to date, and every licence is compatible.");
  } else {
    writeFileSync(out, text);
    console.log(`Wrote THIRD_PARTY_NOTICES.md.`);
  }
}
