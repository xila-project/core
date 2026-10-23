// Build-time font cache, tested independently of Cargo and the converter.
import { createHash } from "node:crypto";
import { createRequire } from "node:module";
import { dirname, join, resolve } from "node:path";
import {
  readFileSync,
  writeFileSync,
  readdirSync,
  realpathSync,
  mkdirSync,
  renameSync,
  unlinkSync,
} from "node:fs";

export function digest(parts) {
  const hash = createHash("sha256");
  for (const part of parts) {
    const bytes = Buffer.isBuffer(part) ? part : Buffer.from(part);
    hash.update(`${bytes.length}:`);
    hash.update(bytes);
  }
  return hash.digest("hex");
}

function remove(path) {
  try {
    unlinkSync(path);
  } catch (error) {
    if (error.code !== "ENOENT") throw error;
  }
}

export function ensureOutput(path, identity, generate) {
  const marker = `${path}.json`;
  try {
    const metadata = JSON.parse(readFileSync(marker, "utf8"));
    const output = readFileSync(path);
    if (
      metadata?.version === 1 &&
      metadata.identity === identity &&
      output.length > 0 &&
      metadata.output === digest([output])
    )
      return false;
  } catch (error) {
    if (error.code !== "ENOENT" && !(error instanceof SyntaxError)) throw error;
  }
  mkdirSync(dirname(path), { recursive: true });
  remove(marker);
  const temporary = `${path}.partial`;
  const temporaryMarker = `${marker}.partial`;
  try {
    remove(temporary);
    generate(temporary);
    const output = readFileSync(temporary);
    if (output.length === 0)
      throw new Error(`Generated font is empty: ${path}`);
    renameSync(temporary, path);
    writeFileSync(
      temporaryMarker,
      JSON.stringify({ version: 1, identity, output: digest([output]) }),
    );
    renameSync(temporaryMarker, marker);
  } finally {
    remove(temporary);
    remove(temporaryMarker);
  }
  return true;
}

// Resolve from each owner, not from the workspace: npm can bundle a different
// version under lv_font_conv/node_modules than the root package uses.
function packageRoot(owner, name) {
  const require = createRequire(join(owner, "package.json"));
  try {
    return dirname(require.resolve(`${name}/package.json`));
  } catch (error) {
    if (error.code !== "ERR_PACKAGE_PATH_NOT_EXPORTED") throw error;
    let path = dirname(require.resolve(name));
    for (;;) {
      try {
        if (
          JSON.parse(readFileSync(join(path, "package.json"), "utf8")).name ===
          name
        )
          return path;
      } catch (failure) {
        if (failure.code !== "ENOENT") throw failure;
      }
      const parent = dirname(path);
      if (parent === path)
        throw new Error(`Cannot find package root for ${name}`);
      path = parent;
    }
  }
}

export function converterInputs(converter) {
  const roots = new Set();
  const paths = new Set();
  const parts = [];
  function visit(root) {
    root = realpathSync(root);
    if (roots.has(root)) return;
    roots.add(root);
    function walk(path, relative) {
      for (const entry of readdirSync(path, { withFileTypes: true }).sort(
        (a, b) => a.name.localeCompare(b.name, "en"),
      )) {
        // Dependencies are resolved separately, including external/hoisted ones.
        if (entry.name === "node_modules" || entry.name === ".git") continue;
        const file = join(path, entry.name);
        if (entry.isDirectory()) walk(file, `${relative}/${entry.name}`);
        else {
          paths.add(file);
          parts.push(`${root}/${relative}/${entry.name}`, readFileSync(file));
        }
      }
    }
    walk(root, "");
    const metadata = JSON.parse(
      readFileSync(join(root, "package.json"), "utf8"),
    );
    const dependencies = {
      ...metadata.dependencies,
      ...metadata.optionalDependencies,
    };
    for (const name of Object.keys(dependencies).sort()) {
      let dependency;
      try {
        dependency = packageRoot(root, name);
      } catch (error) {
        if (
          error.code === "MODULE_NOT_FOUND" &&
          Object.hasOwn(metadata.optionalDependencies ?? {}, name)
        )
          continue;
        throw error;
      }
      visit(dependency);
    }
  }
  visit(resolve(converter));
  // Watching package directories also detects added files and npm replacement.
  return { identity: digest(parts), paths: [...roots, ...paths].sort() };
}
