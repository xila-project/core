/**
 * Syntax-aware, dependency-free extractor for embedded-i18n Rust macros.
 * Only static Rust string literals are recognized (the proc macros require them).
 * Values remain in per-crate JSON_C namespaces.
 */
import { readFileSync, readdirSync } from "node:fs";
import path from "node:path";

const PLURAL = new Set(["zero", "one", "two", "few", "many", "other"]);
const MACROS = new Set([
  "translate", "translation", "translate_args", "translate_write",
  "translate_plural", "translate_plural_write", "translate_select",
  "translate_select_write",
]);
const WRITE = new Set(["translate_write", "translate_plural_write", "translate_select_write"]);

function namespaceFor(fileName) {
  const relative = path.relative(process.cwd(), path.resolve(fileName)).replace(/\\/g, "/");
  const index = relative.lastIndexOf("/src/");
  return index > 0 ? relative.slice(0, index) : undefined;
}

function readLiteral(code, at) {
  let pos = at;
  if (code[pos] === "c" || code[pos] === "b") pos++;
  if (code[pos] === "r") {
    pos++;
    let hashes = 0;
    while (code[pos] === "#") { hashes++; pos++; }
    if (code[pos] !== '"') return null;
    const end = code.indexOf('"' + "#".repeat(hashes), pos + 1);
    if (end < 0) return null;
    return { value: code.slice(pos + 1, end), end: end + hashes + 1 };
  }
  if (code[pos] !== '"') return null;
  pos++;
  let value = "";
  while (pos < code.length) {
    const ch = code[pos++];
    if (ch === '"') return { value, end: pos };
    if (ch !== "\\") { value += ch; continue; }
    const esc = code[pos++];
    const escapes = { n: "\n", r: "\r", t: "\t", "0": "\0", '"': '"', "\\": "\\" };
    if (esc in escapes) value += escapes[esc];
    else if (esc === "u" && code[pos] === "{") {
      const last = code.indexOf("}", pos + 1);
      if (last < 0) return null;
      value += String.fromCodePoint(parseInt(code.slice(pos + 1, last).replace(/_/g, ""), 16));
      pos = last + 1;
    } else value += esc;
  }
  return null;
}

function lex(code) {
  const tokens = [];
  let i = 0;
  while (i < code.length) {
    if (/\s/.test(code[i])) { i++; continue; }
    if (code.startsWith("//", i)) {
      const end = code.indexOf("\n", i);
      i = end < 0 ? code.length : end + 1;
      continue;
    }
    if (code.startsWith("/*", i)) {
      i += 2;
      let depth = 1;
      while (i < code.length && depth) {
        if (code.startsWith("/*", i)) { depth++; i += 2; }
        else if (code.startsWith("*/", i)) { depth--; i += 2; }
        else i++;
      }
      continue;
    }
    const start = i;
    // Rust string and C-string literals, including raw literal syntax.
    const prefix = /^(?:cr|br|r|c|b)?(?:#+)?"/.exec(code.slice(i));
    if (prefix) {
      const value = readLiteral(code, i);
      if (value) {
        tokens.push({ type: "string", value: value.value, at: start });
        i = value.end;
        continue;
      }
    }
    if (/[A-Za-z_]/.test(code[i])) {
      i++;
      while (i < code.length && /[A-Za-z0-9_]/.test(code[i])) i++;
      tokens.push({ type: "id", value: code.slice(start, i), at: start });
      continue;
    }
    // Do not attempt to interpret lifetimes; the punctuation is harmless.
    tokens.push({ type: "punct", value: code[i++], at: start });
  }
  return tokens;
}

function splitArgs(tokens, openAt) {
  const args = [];
  const stack = ["("];
  let start = openAt + 1;
  const opening = { "(": ")", "[": "]", "{": "}" };
  for (let i = start; i < tokens.length; i++) {
    const value = tokens[i].value;
    if (value in opening && tokens[i].type === "punct") {
      stack.push(value);
    } else if (tokens[i].type === "punct" && [")", "]", "}"].includes(value)) {
      const opener = stack.pop();
      if (opening[opener] !== value) return null;
      if (!stack.length) {
        args.push(tokens.slice(start, i));
        return args;
      }
    } else if (value === "," && stack.length === 1 && tokens[i].type === "punct") {
      args.push(tokens.slice(start, i));
      start = i + 1;
    }
  }
  return null;
}

function formsInCatalogs(namespace, key, plural) {
  if (!namespace) return [];
  const directory = path.join(process.cwd(), namespace, "locales");
  const found = new Set();
  try {
    for (const file of readdirSync(directory)) {
      if (!file.endsWith(".json")) continue;
      const catalog = JSON.parse(readFileSync(path.join(directory, file), "utf8"));
      for (const [candidate, value] of Object.entries(catalog)) {
        if (candidate === key && value && typeof value === "object") {
          for (const suffix of Object.keys(value)) {
            if (!plural || PLURAL.has(suffix)) found.add(key + "." + suffix);
          }
        }
        if (!candidate.startsWith(key + ".")) continue;
        const suffix = candidate.slice(key.length + 1);
        if (!suffix || suffix.includes(".")) continue;
        if (!plural || PLURAL.has(suffix)) found.add(candidate);
      }
    }
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
  // Unknown plural/select forms must never be fabricated.
  return [...found].sort();
}

export default function extractRustTranslations(code, fileName) {
  if (!fileName.endsWith(".rs")) return { keys: [] };
  const namespace = namespaceFor(fileName);
  const tokens = lex(code);
  const keys = [];
  for (let i = 0; i < tokens.length - 2; i++) {
    const macro = tokens[i].value;
    if (tokens[i].type !== "id" || !MACROS.has(macro)) continue;
    if (tokens[i + 1].value !== "!" || tokens[i + 2].value !== "(") continue;
    const args = splitArgs(tokens, i + 2);
    if (!args) continue;
    let context = "";
    if (args[0]?.[0]?.value === "context" && args[0]?.[1]?.value === "="
        && args[0]?.[2]?.type === "string") {
      context = args.shift()[2].value;
    }
    const literal = args[WRITE.has(macro) ? 1 : 0];
    if (literal?.length !== 1 || literal[0].type !== "string") continue;
    const name = (context ? context + "." : "") + literal[0].value;
    const derived = macro.includes("plural")
      ? formsInCatalogs(namespace, name, true)
      : macro.includes("select")
        ? formsInCatalogs(namespace, name, false)
        : [name];
    for (const keyName of derived) {
      keys.push({
        keyName,
        namespace,
        line: code.slice(0, tokens[i].at).split("\n").length,
        file: fileName,
      });
    }
  }
  return { keys };
}
