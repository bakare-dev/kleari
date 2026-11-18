// Tests for the Dashboard's search (src/features/dashboard/search.ts), run by Node with its
// TypeScript type stripping: `pnpm test:ui`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { searchEverything } from "../src/features/dashboard/search.ts";

const apps = {
  id: "apps",
  label: "Apps",
  icon: "apps",
  tone: "blue",
  target: { route: "apps", detail: "apps", list: "apps" },
  items: [
    { id: "a1", title: "Visual Studio Code", sub: "1 GB", text: "/Applications com.microsoft.VSCode" },
    { id: "a2", title: "Xcode", sub: "12 GB", text: "/Applications com.apple.dt.Xcode" },
    { id: "a3", title: "Code Helper Leftover", sub: "2 MB", target: { route: "apps", detail: "leftovers", list: "apps.leftovers" } },
  ],
};
const developer = { id: "developer", label: "Developer", icon: "code", tone: "indigo", target: { route: "developer" }, items: null, pending: "Scan in Developer to include it." };
const pages = { id: "pages", label: "Pages", icon: "dashboard", tone: "gray", target: { route: "dashboard" }, items: [{ id: "p1", title: "Storage", sub: "Page", text: "disk space" }] };

test("an empty or blank query finds nothing and lists nothing as pending", () => {
  for (const query of ["", "   "]) {
    const result = searchEverything(query, [apps, developer, pages]);
    assert.deepEqual(result.groups, []);
    assert.deepEqual(result.pending, []);
    assert.equal(result.total, 0);
  }
});

test("matches are grouped by module in the sources' order, case-insensitively, title and text", () => {
  const result = searchEverything("CODE", [pages, apps]);
  assert.deepEqual(result.groups.map((group) => group.source.id), ["apps"]);
  assert.equal(result.groups[0].total, 3);
  const disk = searchEverything("disk", [pages, apps]);
  assert.deepEqual(disk.groups.map((group) => group.source.id), ["pages"]);
});

test("titles that start with the query come first, then titles containing it, then other text", () => {
  const result = searchEverything("code", [apps]);
  assert.deepEqual(result.groups[0].matches.map((item) => item.id), ["a3", "a1", "a2"]);
});

test("each group shows at most perGroup matches but counts all of them", () => {
  const result = searchEverything("code", [apps], 2);
  assert.equal(result.groups[0].matches.length, 2);
  assert.equal(result.groups[0].total, 3);
  assert.equal(result.total, 3);
});

test("Show all opens the best match's tab, or the module's own", () => {
  assert.deepEqual(searchEverything("code", [apps]).groups[0].showAll, { route: "apps", detail: "leftovers", list: "apps.leftovers" });
  assert.deepEqual(searchEverything("xcode", [apps]).groups[0].showAll, apps.target);
});

test("a module with nothing read yet is pending, never a group; one without a hint is left out", () => {
  const silent = { ...developer, id: "quiet", pending: undefined };
  const result = searchEverything("code", [apps, developer, silent]);
  assert.deepEqual(result.pending.map((source) => source.id), ["developer"]);
  assert.deepEqual(result.groups.map((group) => group.source.id), ["apps"]);
});

test("the query is trimmed, so surrounding spaces still match", () => {
  assert.equal(searchEverything("  xcode ", [apps]).groups[0].total, 1);
});
