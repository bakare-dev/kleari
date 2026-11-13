/* global process, console */
// Fails when the release bundle could contain v2 sample data. main.tsx loads the sample
// interface (V2App) only in development (import.meta.env.DEV) and ReleaseApp otherwise, so
// Vite must drop sample data from `pnpm build`. Two checks prove it:
// 1. every import reachable from main.tsx, static or dynamic, reaches no file in
//    src/v2/sample/ (whether or not it carries the marker); only main.tsx's DEV-gated
//    ./v2/V2App import is skipped, and the gating itself is checked;
// 2. no built JavaScript file contains the sample-data marker.
// Run after `pnpm build`: `pnpm check:release-bundle`.

import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { sampleImportsFrom } from "./release-graph.mjs";

const MARKER = "kleari-sample-data";
const desktop = join(import.meta.dirname, "..");
const assets = join(desktop, "dist", "assets");
const main = join(desktop, "src", "main.tsx");

// main.tsx must load V2App only behind import.meta.env.DEV and ReleaseApp otherwise; that one
// development import is the only one the graph walk skips.
const DEV_ONLY = "./v2/V2App";
const mainSource = readFileSync(main, "utf8");
const gated = new RegExp(
  String.raw`import\.meta\.env\.DEV\s*\?\s*import\(\s*["']\./v2/V2App["']\s*\)[\s\S]*?:\s*import\(\s*["']\./v2/ReleaseApp["']\s*\)`,
);
if (!gated.test(mainSource)) {
  console.error(`main.tsx no longer loads ${DEV_ONLY} only in development and ./v2/ReleaseApp otherwise; update this check.`);
  process.exit(1);
}

const reachable = sampleImportsFrom(main, { excludeAtEntry: [DEV_ONLY] });
if (reachable.length > 0) {
  console.error(`The release interface imports sample data:\n  ${reachable.join("\n  ")}`);
  process.exit(1);
}

let files;
try {
  files = readdirSync(assets).filter((name) => name.endsWith(".js"));
} catch (error) {
  console.error(`No release bundle at ${assets}. Run pnpm build first.`);
  throw error;
}

if (files.length === 0) {
  console.error(`No JavaScript files in ${assets}. Run pnpm build first.`);
  process.exit(1);
}

const leaking = files.filter((name) => readFileSync(join(assets, name), "utf8").includes(MARKER));

if (leaking.length > 0) {
  console.error(`Sample data found in the release bundle: ${leaking.join(", ")}`);
  process.exit(1);
}

console.log(`Release bundle is free of sample data (source graph checked; ${files.length} JavaScript files checked).`);
