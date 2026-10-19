import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { test } from "node:test";
import { checkCatalog, formatSignature, pseudoLocalize } from "./check-catalogs.mjs";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const catalog = (name) => JSON.parse(readFileSync(path.join(ROOT, name), "utf8"));

test("preserves implicit, explicit, formatted and named Rust format slots", () => {
  const value = "At {{home}}, {name} used {} / {:.2} of {1:?}";
  assert.deepEqual(formatSignature(value), ["{name}", "{}", "{:.2}", "{1:?}"]);
  assert.deepEqual(formatSignature(pseudoLocalize(value)), formatSignature(value));
});

test("rejects malformed placeholder braces", () => {
  for (const value of ["Left {name", "Right }", "Bad {one{two}}"]) {
    assert.throws(() => formatSignature(value));
  }
});

test("detects missing, extra and modified placeholders", () => {
  const source = {
    "Completed {} of {}": "Completed {} of {}",
    "Error: {reason:?}": "Error: {reason:?}",
  };
  assert.deepEqual(
    checkCatalog(source, {
      "Completed {} of {}": "{} sur {} terminés",
      "Error: {reason:?}": "Erreur : {reason:?}",
    }, "fr"),
    [],
  );
  const issues = checkCatalog(source, {
    "Completed {} of {}": "Terminé : {}",
    "Extra": "Extra",
  }, "fr");
  assert.equal(issues.length, 3);
  assert.ok(issues.some((message) => message.includes("placeholder mismatch")));
  assert.ok(issues.some((message) => message.includes("missing key")));
  assert.ok(issues.some((message) => message.includes("unexpected key")));
});

test("pseudo-localization expands representative UI/CLI labels without touching format slots", () => {
  const ui = catalog("executables/settings/locales/en.json");
  const cli = catalog("executables/shell/command_line/locales/en.json");
  const terminal = catalog("executables/terminal/locales/en.json");
  const samples = [
    ui.General,
    ui["Primary color"],
    cli["Command not found"],
    cli["Error: {}"],
    terminal["Enter your command ..."],
  ];
  for (const sample of samples) {
    const expanded = pseudoLocalize(sample);
    assert.ok(expanded.startsWith("⟦") && expanded.endsWith("⟧"));
    assert.ok(expanded.length >= sample.length + 4, sample);
    assert.deepEqual(formatSignature(expanded), formatSignature(sample));
  }
});
