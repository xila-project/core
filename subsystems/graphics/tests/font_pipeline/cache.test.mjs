import assert from "node:assert/strict";
import {
  mkdtempSync,
  writeFileSync,
  readFileSync,
  mkdirSync,
  rmSync,
  statSync,
  existsSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { digest, ensureOutput, converterInputs } from "../../build/cache.mjs";

function directory(t) {
  const path = mkdtempSync(join(tmpdir(), "xila-font-cache-"));
  t.after(() => rmSync(path, { recursive: true, force: true }));
  return path;
}

test("cache hits preserve output and completion metadata without running generation", (t) => {
  const path = join(directory(t), "font.c");
  let calls = 0;
  const generate = (output) => {
    calls++;
    writeFileSync(output, "complete source");
  };
  assert.equal(ensureOutput(path, "a", generate), true);
  const before = [
    statSync(path, { bigint: true }).mtimeNs,
    statSync(`${path}.json`, { bigint: true }).mtimeNs,
  ];
  assert.equal(ensureOutput(path, "a", generate), false);
  assert.equal(calls, 1);
  assert.deepEqual(
    [
      statSync(path, { bigint: true }).mtimeNs,
      statSync(`${path}.json`, { bigint: true }).mtimeNs,
    ],
    before,
  );
  assert.equal(ensureOutput(path, "b", generate), true);
  assert.equal(calls, 2);
});

test("missing, corrupt and incomplete outputs are never valid cache entries", (t) => {
  const path = join(directory(t), "font.c");
  const generate = (output) => writeFileSync(output, "complete");
  for (const damage of [
    () => rmSync(path),
    () => writeFileSync(path, ""),
    () => writeFileSync(path, "partial"),
    () => rmSync(`${path}.json`),
    () => writeFileSync(`${path}.json`, "{broken"),
    () => writeFileSync(`${path}.json`, "null"),
  ]) {
    ensureOutput(path, "a", generate);
    damage();
    assert.equal(ensureOutput(path, "a", generate), true);
    assert.equal(readFileSync(path, "utf8"), "complete");
  }
});

test("failed or empty generation cannot publish a completion marker", (t) => {
  const path = join(directory(t), "font.c");
  assert.throws(
    () =>
      ensureOutput(path, "a", (output) => {
        writeFileSync(output, "partial");
        throw new Error("converter failed");
      }),
    /converter failed/,
  );
  assert.equal(existsSync(`${path}.json`), false);
  assert.throws(
    () => ensureOutput(path, "a", (output) => writeFileSync(output, "")),
    /empty/,
  );
  assert.equal(existsSync(`${path}.json`), false);
  assert.equal(
    ensureOutput(path, "a", (output) => writeFileSync(output, "complete")),
    true,
  );
});

test("digest fields have unambiguous boundaries", () => {
  assert.notEqual(digest(["ab", "c"]), digest(["a", "bc"]));
});

test("converter identity includes nested and externally resolved dependency contents", (t) => {
  const root = directory(t);
  function pkg(relative, name, dependencies = {}) {
    const path = join(root, relative);
    mkdirSync(path, { recursive: true });
    writeFileSync(
      join(path, "package.json"),
      JSON.stringify({
        name,
        version: "1.0.0",
        main: "index.js",
        dependencies,
      }),
    );
    writeFileSync(join(path, "index.js"), "module.exports = 1;");
    return path;
  }
  const converter = pkg("node_modules/converter", "converter", {
    nested: "1",
    shared: "1",
  });
  const nested = pkg("node_modules/converter/node_modules/nested", "nested");
  const shared = pkg("node_modules/shared", "shared");
  const first = converterInputs(converter);
  assert.equal(converterInputs(converter).identity, first.identity);
  writeFileSync(join(nested, "index.js"), "module.exports = 2;");
  const second = converterInputs(converter);
  assert.notEqual(second.identity, first.identity);
  writeFileSync(join(shared, "index.js"), "module.exports = 3;");
  assert.notEqual(converterInputs(converter).identity, second.identity);
  assert.ok(first.paths.includes(join(shared, "index.js")));
});
