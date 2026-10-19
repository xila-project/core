import assert from "node:assert/strict";
import path from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { ALL_LOCALES, selectScope } from "./localization_scope.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
const packages = [
  { name: "graphics", manifest_path: path.join(root, "modules/graphics/Cargo.toml"), dependencies: ["shared"], localization: true },
  { name: "shared", manifest_path: path.join(root, "modules/shared/Cargo.toml"), dependencies: [], localization: true },
  { name: "calculator", manifest_path: path.join(root, "executables/calculator/Cargo.toml"), dependencies: ["graphics"], localization: true },
  { name: "clock", manifest_path: path.join(root, "executables/clock/Cargo.toml"), dependencies: ["shared"], localization: true },
  { name: "fonts_generator", manifest_path: path.join(root, "modules/graphics/fonts_generator/Cargo.toml"), dependencies: ["graphics"], localization: false },
  { name: "debug_protocol", manifest_path: path.join(root, "modules/debug_protocol/Cargo.toml"), dependencies: [], localization: false },
];

test("a catalog change checks only its locale and dependent guest packages", () => {
  const scope = selectScope(["modules/graphics/locales/es.json"], packages, root);

  assert.equal(scope.full, false);
  assert.deepEqual(scope.locales, ["es"]);
  assert.deepEqual(scope.hostPackages, ["graphics"]);
  assert.deepEqual(scope.guestPackages, ["calculator"]);
  assert.deepEqual(scope.fontLocales, ["es"]);
});

test("a Rust change checks all locales for the changed localization package", () => {
  const scope = selectScope(["modules/graphics/src/screen.rs"], packages, root);

  assert.equal(scope.full, false);
  assert.deepEqual(scope.locales, ALL_LOCALES);
  assert.deepEqual(scope.hostPackages, ["graphics"]);
  assert.deepEqual(scope.guestPackages, ["calculator"]);
  assert.deepEqual(scope.fontLocales, []);
});

test("a non-Latin catalog change revalidates its font subset", () => {
  const scope = selectScope(["executables/calculator/locales/it.json"], packages, root);

  assert.equal(scope.full, false);
  assert.deepEqual(scope.locales, ["it"]);
  assert.deepEqual(scope.hostPackages, ["calculator"]);
  assert.deepEqual(scope.fontLocales, ["it"]);
});

test("a Rust change in a package without localization skips locale compilation", () => {
  const scope = selectScope(["modules/debug_protocol/src/lib.rs"], packages, root);

  assert.equal(scope.full, false);
  assert.deepEqual(scope.locales, []);
  assert.deepEqual(scope.hostPackages, []);
  assert.deepEqual(scope.guestPackages, []);
});

test("workspace configuration alone uses the full check", () => {
  const scope = selectScope(["Cargo.lock"], packages, root);

  assert.equal(scope.full, true);
  assert.deepEqual(scope.locales, ALL_LOCALES);
});

test("workspace configuration with a changed package keeps a scoped check", () => {
  const scope = selectScope(
    ["Cargo.lock", "Cargo.toml", "modules/graphics/locales/es.json"],
    packages,
    root,
  );

  assert.equal(scope.full, false);
  assert.deepEqual(scope.locales, ["es"]);
  assert.deepEqual(scope.hostPackages, ["graphics"]);
});

test("an unmapped translation input falls back to the full check", () => {
  const scope = selectScope(["translations/messages.ftl"], packages, root);

  assert.equal(scope.full, true);
  assert.deepEqual(scope.locales, ALL_LOCALES);
});
