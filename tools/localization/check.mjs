#!/usr/bin/env node
/**
 * Static localization consistency checks. No runtime dependencies and no
 * remote Tolgee access. Optionally compare against a pre-migration Git ref:
 *   node tools/localization/check.mjs --baseline <commit>
 */
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { flattenCatalog } from "./catalog.mjs";
import { execFileSync } from "node:child_process";
import extract from "../../tolgee.extractor.mjs";

const ROOT = process.cwd();
const APPLICATIONS_ROOT = process.env.XILA_APPLICATIONS_ROOT || ROOT;
const mappingRoot = path.join(ROOT, "tools/localization/mappings");
const args = process.argv.slice(2);
const baselineIndex = args.indexOf("--baseline");
const baseline = baselineIndex >= 0 ? args[baselineIndex + 1] : null;
if (baselineIndex >= 0 && !baseline) throw Error("--baseline requires a Git ref");
const failures = [];
const CATEGORY = /\.(zero|one|two|few|many|other)$/;
const KEY = /^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)+$/;

function visit(dir, suffix, output = []) {
  if (!existsSync(dir)) return output;
  for (const item of readdirSync(dir, { withFileTypes: true })) {
    const file = path.join(dir, item.name);
    if (item.isDirectory()) visit(file, suffix, output);
    else if (item.isFile() && item.name.endsWith(suffix)) output.push(file);
  }
  return output;
}

function catalog(pathName) {
  const json = JSON.parse(readFileSync(pathName, "utf8"));
  if (!json || Array.isArray(json) || typeof json !== "object") {
    throw Error(pathName + ": expected a JSON object");
  }
  const flat = flattenCatalog(json);
  for (const [key, value] of Object.entries(flat)) {
    if (typeof value !== "string") failures.push(pathName + ": nonstring value " + key);
    if (!KEY.test(key)) failures.push(pathName + ": nonsemantic key " + key);
    const segments = key.split(".");
    for (let n = 1; n < segments.length; n++) {
      const prefix = segments.slice(0, n).join(".");
      if (Object.hasOwn(flat, prefix)) failures.push(pathName + ": dotted collision " + prefix);
    }
  }
  return flat;
}

function baselineCatalog(file) {
  const relative = path.relative(ROOT, file).replace(/\\/g, "/");
  try {
    return JSON.parse(execFileSync("git", ["show", baseline + ":" + relative], {
      cwd: ROOT, encoding: "utf8",
    }));
  } catch {
    failures.push("Unable to read baseline catalog: " + relative);
    return {};
  }
}

let namespaceCount = 0;
for (const file of readdirSync(mappingRoot).filter(name => name.endsWith(".json")).sort()) {
  const mapping = JSON.parse(readFileSync(path.join(mappingRoot, file), "utf8"));
  const namespace = mapping.namespace;
  const prefix = path.join(ROOT, namespace);
  const locales = visit(path.join(prefix, "locales"), ".json");
  const values = new Map(locales.map(file => [file, catalog(file)]));
  const oldToNew = mapping.old_to_new;
  namespaceCount++;
  const destinations = Object.values(oldToNew);
  if (new Set(destinations).size !== destinations.length) {
    failures.push(namespace + ": duplicate semantic destinations in migration mapping");
  }
  const allKeys = new Set([...values.values()].flatMap(data => Object.keys(data)));
  for (const [oldKey, newKey] of Object.entries(oldToNew)) {
    if (!KEY.test(newKey)) failures.push(namespace + ": invalid mapped key " + newKey);
    if (!allKeys.has(newKey) && ![...allKeys].some(k => k.startsWith(newKey + "."))) {
      failures.push(namespace + ": missing mapped key " + oldKey + " -> " + newKey);
    }
  }
  const used = new Set();
  for (const source of visit(path.join(prefix, "src"), ".rs")) {
    for (const key of extract(readFileSync(source, "utf8"), source).keys) {
      used.add(key.keyName);
      if (!KEY.test(key.keyName)) {
        failures.push(namespace + ": nonsemantic source key " + key.keyName + " referenced by " + path.relative(ROOT, source) + ":" + key.line);
      }
      if (!allKeys.has(key.keyName)) {
        failures.push(namespace + ": missing key " + key.keyName + " referenced by " + path.relative(ROOT, source));
      }
    }
  }
  const english = [...values.entries()].find(([file]) => file.endsWith("/en.json"))?.[1] || {};
  for (const [file, data] of values) {
    for (const key of Object.keys(english)) {
      // CLDR plural coverage is locale-specific; compile-time strict checks
      // validate the exact categories required by each locale.
      if (!CATEGORY.test(key) && !Object.hasOwn(data, key)) {
        failures.push(file + ": missing nonplural English key " + key);
      }
    }
    if (!baseline) continue;
    const before = baselineCatalog(file);
    for (const [oldKey, oldValue] of Object.entries(before)) {
      const plural = oldKey.match(CATEGORY);
      const root = plural ? oldKey.slice(0, -plural[0].length) : oldKey;
      const newKey = oldToNew[root] + (plural ? plural[0] : "");
      if (!oldToNew[root] || data[newKey] !== oldValue) {
        failures.push(file + ": translation changed or missing: " + oldKey);
      }
      for (const extra of mapping.split_review?.[root] || []) {
        const destination = extra + (plural ? plural[0] : "");
        if (data[destination] !== oldValue) {
          failures.push(file + ": split translation mismatch: " + destination);
        }
      }
    }
  }
  const unused = [...allKeys].filter(k => !used.has(k));
  console.log(namespace + ": " + allKeys.size + " keys across " + locales.length +
    " locales; " + used.size + " extracted references; " + unused.length + " unused catalog keys");
  if (unused.length) console.log("  Review unused: " + unused.join(", "));
}

// Guest UI tests read JSON catalogs through a helper instead of translate! macros.
// Check those assertions too, so source-text keys do not silently escape the
// normal Rust macro extraction pass.
const guestUiTests = [
  {
    application: "clock",
    file: "executables/wasm/tests/clock.rs",
    calls: /\btext\(\s*"([^"]+)"\s*\)/g,
  },
  {
    application: "pixel_painter",
    file: "executables/wasm/tests/pixel_painter.rs",
    calls: /\bstatus\(\s*manager\s*,\s*"([^"]+)"/g,
  },
  {
    application: "text_editor",
    file: "executables/wasm/tests/text_editor.rs",
    calls: /\blocalization::text\(\s*"text_editor"\s*,\s*"([^"]+)"/g,
  },
];
for (const { application, file, calls } of guestUiTests) {
  const sourcePath = path.join(ROOT, file);
  const english = catalog(path.join(APPLICATIONS_ROOT, "executables", application, "locales", "en.json"));
  const source = readFileSync(sourcePath, "utf8");
  let count = 0;
  for (const match of source.matchAll(calls)) {
    const key = match[1];
    count++;
    if (!KEY.test(key)) failures.push(file + ": nonsemantic guest UI test key " + JSON.stringify(key));
    if (!Object.hasOwn(english, key)) failures.push(file + ": missing guest UI test key " + JSON.stringify(key));
  }
  if (!count) failures.push(file + ": no guest UI translation assertions found");
  else console.log(file + ": validated " + count + " guest UI translation references");
}

console.log("Checked " + namespaceCount + " namespaces.");
if (failures.length) {
  console.error("Localization validation failed:\n" + failures.join("\n"));
  process.exitCode = 1;
}
