// Tests for the list filter's rules (src/v2/ui/listFilterLogic.ts), run by Node with its
// TypeScript type stripping: `pnpm test:ui`.
import assert from "node:assert/strict";
import { test } from "node:test";
import { ALL_TYPES, applyFilter } from "../src/v2/ui/listFilterLogic.ts";

const spec = {
  bytes: (item) => item.bytes,
  name: (item) => item.name,
  text: (item) => item.location,
  date: (item) => item.modifiedMs,
  type: (item) => item.kind,
};
const state = (patch) => ({ sort: "largest", type: ALL_TYPES, query: "", ...patch });

// Two groups as Files shows them: one with an image, one without.
const photos = [
  { name: "beach.jpg", kind: "image", bytes: 30, location: "~/Downloads", modifiedMs: 3 },
  { name: "notes.pdf", kind: "document", bytes: 20, location: "~/Downloads", modifiedMs: 1 },
];
const work = [
  { name: "report.docx", kind: "document", bytes: 50, location: "~/Downloads", modifiedMs: 2 },
  { name: "build.zip", kind: "archive", bytes: 10, location: "~/Downloads", modifiedMs: null },
];
const everything = [...photos, ...work];

test("a group with none of the chosen type shows nothing when filtered against the whole list", () => {
  const images = state({ type: "image" });
  assert.deepEqual(applyFilter(photos, images, spec, everything).map((item) => item.name), ["beach.jpg"]);
  assert.deepEqual(applyFilter(work, images, spec, everything), []);
});

test("on its own a list falls back to everything when no item has the remembered type", () => {
  assert.equal(applyFilter(work, state({ type: "image" }), spec).length, 2);
  // And against a whole list that lacks it too.
  assert.equal(applyFilter(work, state({ type: "video" }), spec, everything).length, 2);
});

test("search matches the name or the location, ignoring case and spaces around it", () => {
  assert.deepEqual(applyFilter(everything, state({ query: "  REPORT " }), spec).map((item) => item.name), ["report.docx"]);
  assert.equal(applyFilter(everything, state({ query: "downloads" }), spec).length, 4);
});

test("sorts by size, date (undated last) and name", () => {
  const names = (sort) => applyFilter(everything, state({ sort }), spec).map((item) => item.name);
  assert.deepEqual(names("largest"), ["report.docx", "beach.jpg", "notes.pdf", "build.zip"]);
  assert.deepEqual(names("smallest"), ["build.zip", "notes.pdf", "beach.jpg", "report.docx"]);
  assert.deepEqual(names("newest"), ["beach.jpg", "report.docx", "notes.pdf", "build.zip"]);
  assert.deepEqual(names("oldest"), ["notes.pdf", "report.docx", "beach.jpg", "build.zip"]);
  assert.deepEqual(names("name"), ["beach.jpg", "build.zip", "notes.pdf", "report.docx"]);
});

test("a list without dates sorts by size when a date sort is remembered", () => {
  const undated = { ...spec, date: undefined };
  assert.deepEqual(applyFilter(everything, state({ sort: "newest" }), undated).map((item) => item.name), ["report.docx", "beach.jpg", "notes.pdf", "build.zip"]);
});
