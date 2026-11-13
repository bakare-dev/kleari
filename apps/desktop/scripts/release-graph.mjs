// Follows the relative imports of a source file and returns every reachable path inside the
// v2 sample-data folder, with the chain that reaches it. The bundle marker alone can't prove
// a release build is free of sample data, because not every sample module contains the
// marker; this checks the source graph instead.

import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";

const desktop = resolve(import.meta.dirname, "..");
export const sampleFolder = join(desktop, "src", "v2", "sample");

const EXTENSIONS = ["", ".ts", ".tsx", ".js", ".mjs", "/index.ts", "/index.tsx"];
// Static imports and re-exports (`import x from`, `import "x"`, `export … from`) and dynamic `import("x")`.
const STATIC = /(?:^|[\s;])(?:import|export)\s+(?:[^'";]*?\sfrom\s+)?["']([^"']+)["']/g;
const DYNAMIC = /import\(\s*["']([^"']+)["']\s*\)/g;

function resolveImport(from, specifier) {
  if (!specifier.startsWith(".")) {
    return null;
  }
  const base = resolve(dirname(from), specifier);
  for (const extension of EXTENSIONS) {
    const candidate = base + extension;
    if (existsSync(candidate) && statSync(candidate).isFile()) {
      return candidate;
    }
  }
  throw new Error(`Cannot resolve ${specifier} from ${relative(desktop, from)}`);
}

function importsOf(file, excluded) {
  const source = readFileSync(file, "utf8");
  const specifiers = [...source.matchAll(STATIC), ...source.matchAll(DYNAMIC)].map((match) => match[1]);
  return specifiers
    .filter((specifier) => !excluded.includes(specifier))
    .map((specifier) => resolveImport(file, specifier))
    .filter((path) => path !== null);
}

const isSample = (path) => path === sampleFolder || path.startsWith(`${sampleFolder}/`);

/**
 * Sample-data files reachable from `entry`, following static and dynamic imports.
 * `excludeAtEntry` names import specifiers to skip in the entry file only, exactly as written
 * there (main.tsx's development-only `./v2/V2App`); every other import is followed.
 */
export function sampleImportsFrom(entry, { excludeAtEntry = [] } = {}) {
  const found = [];
  const seen = new Set();
  const queue = [{ file: resolve(entry), chain: [resolve(entry)], excluded: excludeAtEntry }];
  while (queue.length > 0) {
    const { file, chain, excluded } = queue.shift();
    if (seen.has(file)) {
      continue;
    }
    seen.add(file);
    if (isSample(file)) {
      found.push(chain.map((path) => relative(desktop, path)).join(" → "));
      continue;
    }
    if (!/\.(tsx?|m?js)$/.test(file)) {
      // Stylesheets and JSON carry no imports to follow.
      continue;
    }
    for (const next of importsOf(file, excluded)) {
      queue.push({ file: next, chain: [...chain, next], excluded: [] });
    }
  }
  return found;
}
