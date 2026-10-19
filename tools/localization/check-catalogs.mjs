#!/usr/bin/env node
// Catalog-only validation. Nothing here is linked into Xila or Wasm builds.
import { readFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { globSync } from "glob";

const ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");

export function splitFormat(template) {
  if (typeof template !== "string") {
    throw new TypeError("A translation must be a string");
  }
  const parts = [];
  const slots = [];
  let literal = "";
  const flush = () => {
    if (literal) parts.push({ kind: "literal", value: literal });
    literal = "";
  };

  for (let index = 0; index < template.length; index++) {
    const character = template[index];
    if ((character === "{" || character === "}") && template[index + 1] === character) {
      // Rust formatting escape sequences are text, not parameters.
      literal += character + character;
      index++;
    } else if (character === "{") {
      const closing = template.indexOf("}", index + 1);
      if (closing < 0) throw new Error("Unclosed format argument");
      const argument = template.slice(index + 1, closing);
      if (argument.includes("{") || !/^(?:[a-zA-Z_][a-zA-Z_0-9]*|[0-9]+)?(?:![a-z])?(?::[^{}]*)?$/.test(argument)) {
        throw new Error("Invalid format argument {" + argument + "}");
      }
      flush();
      const value = template.slice(index, closing + 1);
      slots.push(value);
      parts.push({ kind: "slot", value });
      index = closing;
    } else if (character === "}") {
      throw new Error("Unmatched closing brace");
    } else {
      literal += character;
    }
  }
  flush();
  return { parts, slots };
}

export function formatSignature(template) {
  return splitFormat(template).slots;
}

const accent = Object.freeze({
  a: "á", b: "ƀ", c: "ç", d: "ď", e: "é", f: "ƒ", g: "ğ",
  h: "ĥ", i: "í", j: "ĵ", k: "ķ", l: "ĺ", m: "ḿ", n: "ñ",
  o: "ó", p: "ṕ", q: "ɋ", r: "ŕ", s: "š", t: "ť", u: "ú",
  v: "ṽ", w: "ŵ", x: "ẋ", y: "ý", z: "ž",
});

export function pseudoLocalize(template) {
  const { parts } = splitFormat(template);
  const contents = parts.map(({ kind, value }) => {
    if (kind === "slot") return value;
    return value.replace(/[A-Za-z]+/g, (word) => {
      const mapped = [...word].map((c) => {
        const result = accent[c.toLowerCase()];
        return c === c.toUpperCase() ? result.toUpperCase() : result;
      }).join("");
      return mapped + "~".repeat(Math.max(1, Math.ceil(word.length * 0.3)));
    });
  }).join("");
  return "⟦" + contents + "⟧";
}

export function checkCatalog(source, candidate, locale) {
  const errors = [];
  for (const [key, english] of Object.entries(source)) {
    if (!Object.hasOwn(candidate, key)) {
      errors.push(locale + ": missing key " + JSON.stringify(key));
      continue;
    }
    try {
      const expected = JSON.stringify(formatSignature(key));
      for (const [label, value] of [["English source", english], [locale, candidate[key]]]) {
        const found = JSON.stringify(formatSignature(value));
        if (expected !== found) {
          errors.push(locale + ": " + label + " placeholder mismatch at " +
            JSON.stringify(key) + " (expected " + expected + ", got " + found + ")");
        }
      }
    } catch (error) {
      errors.push(locale + ": " + JSON.stringify(key) + ": " + error.message);
    }
  }
  for (const key of Object.keys(candidate)) {
    if (!Object.hasOwn(source, key)) {
      errors.push(locale + ": unexpected key " + JSON.stringify(key));
    }
  }
  return errors;
}

function main(args) {
  let pseudoDirectory = null;
  if (args.length === 2 && args[0] === "--pseudo") {
    pseudoDirectory = path.resolve(args[1]);
  } else if (args.length !== 0) {
    throw new Error("Usage: node tools/localization/check-catalogs.mjs [--pseudo OUTPUT_DIR]");
  }

  const sources = globSync("**/locales/en.json", {
    cwd: ROOT,
    ignore: ["node_modules/**", "**/node_modules/**", "target/**"],
  }).sort();
  if (sources.length === 0) throw new Error("No English catalogs found");

  const errors = [];
  let checked = 0;
  for (const relative of sources) {
    const namespace = relative.slice(0, -("en.json".length));
    const source = JSON.parse(readFileSync(path.join(ROOT, relative), "utf8"));
    const locales = globSync("*.json", { cwd: path.join(ROOT, namespace) }).sort();
    for (const localeFile of locales) {
      const locale = path.basename(localeFile, ".json");
      const catalog = JSON.parse(readFileSync(path.join(ROOT, namespace, localeFile), "utf8"));
      errors.push(...checkCatalog(source, catalog, namespace + locale));
      checked++;
    }
    if (pseudoDirectory) {
      const generated = Object.fromEntries(
        Object.keys(source).map((key) => [key, pseudoLocalize(source[key])]),
      );
      const destination = path.join(pseudoDirectory, namespace, "pseudo.json");
      mkdirSync(path.dirname(destination), { recursive: true });
      writeFileSync(destination, JSON.stringify(generated, null, 2) + "\n");
    }
  }

  if (errors.length) {
    for (const error of errors) console.error(error);
    console.error("Localization validation failed: " + errors.length + " problem(s)");
    process.exitCode = 1;
  } else {
    console.log("Validated " + checked + " catalogs in " + sources.length + " namespaces.");
    if (pseudoDirectory) {
      console.log("Generated pseudo-localization catalogs under " + pseudoDirectory);
    }
  }
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main(process.argv.slice(2));
}
