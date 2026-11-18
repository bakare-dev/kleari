// Tests for the licence check in third-party-notices.mjs. Run: `pnpm test:notices`.

import { strict as assert } from "node:assert";
import { test } from "node:test";
import { compatible } from "./third-party-notices.mjs";

test("licences that combine with GPL-3.0-or-later are accepted", () => {
  for (const expression of ["MIT", "Apache-2.0", "MIT OR Apache-2.0", "MIT/Apache-2.0", "Zlib OR Apache-2.0 OR MIT", "Unicode-3.0", "MPL-2.0", "(MIT OR Apache-2.0) AND Unicode-3.0", "Apache-2.0 WITH LLVM-exception", "LGPL-2.1+"]) {
    assert.equal(compatible(expression), true, expression);
  }
});

test("a choice is enough when one side is compatible", () => {
  assert.equal(compatible("SSPL-1.0 OR MIT"), true);
});

test("licences that can't be combined, or an AND with one, are refused", () => {
  for (const expression of ["SSPL-1.0", "BUSL-1.1", "GPL-2.0-only", "CC-BY-NC-4.0", "MIT AND SSPL-1.0", "Proprietary", "Apache-2.0 WITH Commons-Clause"]) {
    assert.equal(compatible(expression), false, expression);
  }
});

test("an expression that can't be read is refused", () => {
  for (const expression of ["", "MIT OR", "(MIT", "MIT) OR Apache-2.0", "AND MIT"]) {
    assert.equal(compatible(expression), null, expression);
  }
});
