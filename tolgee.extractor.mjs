/**
 * @typedef {import('@tolgee/cli/extractor').ExtractionResult} ExtractionResult
 */

import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";

function normalizePath(fileName) {
  return fileName.replace(/\\/g, "/");
}

function resolveNamespace(fileName) {
  const relativePath = normalizePath(path.relative(process.cwd(), fileName));
  const sourceMarker = "/src/";
  const markerIndex = relativePath.lastIndexOf(sourceMarker);

  if (markerIndex <= 0) {
    return undefined;
  }

  return relativePath.slice(0, markerIndex);
}

const pluralCategories = new Set([
  "zero",
  "one",
  "two",
  "few",
  "many",
  "other",
]);

function pluralKeys(namespace, keyName) {
  if (!namespace) return [`${keyName}.one`, `${keyName}.other`];
  // JSON_C uses flat dotted keys, which embedded-i18n resolves exactly
  // like nested { "key": { "one": ..., "other": ... } } catalogs.
  const directory = path.join(process.cwd(), namespace, "locales");
  const keys = new Set();
  try {
    for (const filename of readdirSync(directory)) {
      if (!filename.endsWith(".json")) continue;
      const catalog = JSON.parse(readFileSync(path.join(directory, filename), "utf8"));
      for (const [key, value] of Object.entries(catalog)) {
        if (key === keyName && value && typeof value === "object") {
          for (const category of Object.keys(value)) {
            if (pluralCategories.has(category)) keys.add(`${keyName}.${category}`);
          }
        }
        if (
          key.startsWith(keyName + ".") &&
          pluralCategories.has(key.slice(keyName.length + 1))
        ) {
          keys.add(key);
        }
      }
    }
  } catch {
    // Extraction can run before catalogs are populated. The strict Rust
    // localization build remains responsible for validating their contents.
  }
  return keys.size ? [...keys].sort() : [`${keyName}.one`, `${keyName}.other`];
}

/**
 * Extracts keys from Rust translation macros, including plural forms.
 *
 * @param {string} code
 * @param {string} fileName
 * @returns {ExtractionResult}
 */
export default function extractRustTranslations(code, fileName) {
  if (!fileName.endsWith(".rs")) {
    return { keys: [] };
  }

  const namespace = resolveNamespace(fileName);
  const foundKeys = [];
  const addKey = (keyName, index) => {
    foundKeys.push({
      keyName,
      namespace,
      line: code.slice(0, index).split("\n").length,
      file: fileName,
    });
  };

  // Plain lookups and plural lookup macros have the key as first argument.
  const translationCall =
    /\b(translate|translation|translate_plural)!\s*\(\s*(?:c)?"((?:\\.|[^"\\])+)"/g;
  let match;
  while ((match = translationCall.exec(code)) !== null) {
    const keys =
      match[1] === "translate_plural"
        ? pluralKeys(namespace, match[2])
        : [match[2]];
    for (const key of keys) addKey(key, match.index);
  }

  // The write macro takes the writer before the translation key.
  const pluralWriteCall =
    /\btranslate_plural_write!\s*\(\s*[^,]+,\s*"((?:\\.|[^"\\])+)"/g;
  while ((match = pluralWriteCall.exec(code)) !== null) {
    for (const key of pluralKeys(namespace, match[1])) addKey(key, match.index);
  }

  return {
    keys: foundKeys,
  };
}
