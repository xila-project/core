import assert from "node:assert/strict";
import { test } from "node:test";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { flattenCatalog } from "./catalog.mjs";

const ROOT = fileURLToPath(new URL("../../", import.meta.url));
const KEY = /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/;

function findCatalogs(directory, results = []) {
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    if (!entry.isDirectory() || ["target", "node_modules", ".git"].includes(entry.name)) continue;
    const child = path.join(directory, entry.name);
    if (entry.name === "locales") {
      for (const file of readdirSync(child).filter(name => name.endsWith(".json"))) {
        results.push(path.join(child, file));
      }
    } else {
      findCatalogs(child, results);
    }
  }
  return results;
}

function inflate(flat) {
  const nested = {};
  for (const [key, value] of Object.entries(flat)) {
    const segments = key.split(".");
    let node = nested;
    for (const segment of segments.slice(0, -1)) {
      node = node[segment] ??= {};
    }
    node[segments.at(-1)] = value;
  }
  return nested;
}

test("flat, nested and mixed catalogs resolve to the same dotted keys", () => {
  const mixed = {
    menu: { actions: { save: "Save" }, "actions.cancel": "Cancel" },
    "menu.title": "Menu",
    items: { one: "One item", other: "{count} items" },
  };
  assert.deepEqual({ ...flattenCatalog(mixed) }, {
    "menu.actions.save": "Save",
    "menu.actions.cancel": "Cancel",
    "menu.title": "Menu",
    "items.one": "One item",
    "items.other": "{count} items",
  });
});

test("rejects ambiguous keys which Tolgee could silently merge", () => {
  assert.throws(() => flattenCatalog({ a: { b: "first" }, "a.b": "second" }), /duplicate translation key/);
  assert.throws(() => flattenCatalog({ a: "parent", "a.b": "child" }), /translation path collision/);
  assert.throws(() => flattenCatalog([]), /expected a translation JSON object/);
});

test("all Xila locale files are nested and round-trip without changing messages", () => {
  const catalogs = ["libraries", "subsystems", "drivers"]
    .flatMap(directory => findCatalogs(path.join(ROOT, directory)));
  assert.ok(catalogs.length >= 88, "expected at least 11 Core namespaces with 8 locales each");
  const unconverted = new Set();
  for (const file of catalogs) {
    const nested = JSON.parse(readFileSync(file, "utf8"));
    const dotted = flattenCatalog(nested);
    if (Object.keys(dotted).length === 0) {
      // Bootsplash intentionally has eight empty catalogs and no translations.
      assert.match(file, /[/\\]libraries[/\\]bootsplash[/\\]locales[/\\]/);
      continue;
    }
    for (const [key, value] of Object.entries(dotted)) {
      assert.match(key, KEY, file + ": nonsemantic translation key");
      assert.equal(typeof value, "string", file + ": non-string translation: " + key);
    }
    function visit(node) {
      for (const [key, value] of Object.entries(node)) {
        if (key.includes(".")) unconverted.add(path.relative(ROOT, file));
        if (value && typeof value === "object" && !Array.isArray(value)) visit(value);
      }
    }
    visit(nested);
    if (!unconverted.has(path.relative(ROOT, file))) {
      assert.deepEqual(inflate(dotted), nested, file + ": catalog not lossless");
    }
  }
  assert.deepEqual([...unconverted].sort(), [], "Remaining flat catalogs");
});
