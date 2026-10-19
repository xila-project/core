import { execFileSync } from "node:child_process";
import { existsSync, readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

export const ALL_LOCALES = ["en", "fr", "es", "de", "pt-BR", "it", "pl"];
const FONT_LOCALES = ["es", "de", "pt-BR", "it", "pl"];
const GUEST_PACKAGES = [
  "calculator",
  "weather",
  "clock",
  "pixel_painter",
  "text_editor",
  "synth",
];

function isFullCheckPath(file) {
  if (
    [
      "Makefile.toml",
      "tolgee.config.ts",
      "tolgee.extractor.mjs",
      "tolgee.extractor.test.mjs",
      "package.json",
      "package-lock.json",
      ".github/scripts/localization_scope.mjs",
      ".github/scripts/localization_scope.test.mjs",
      "rust-toolchain",
      "rust-toolchain.toml",
    ].includes(file)
  ) {
    return true;
  }
  if ([".github/", ".cargo/", "development/"].some((prefix) => file.startsWith(prefix))) {
    return true;
  }
  const parts = file.split("/");
  return (
    file.endsWith("/Cargo.lock") ||
    file.endsWith("/build.rs") ||
    parts.includes("build") ||
    parts.includes("fonts_generator") ||
    parts.includes("bindings_generator")
  );
}

function sourceFiles(directory) {
  if (!existsSync(directory)) return [];
  const result = [];
  for (const entry of readdirSync(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) result.push(...sourceFiles(entryPath));
    else if (entry.isFile() && entry.name.endsWith(".rs")) result.push(entryPath);
  }
  return result;
}

function hasLocalization(root) {
  const locales = path.join(root, "locales");
  if (existsSync(locales) && readdirSync(locales).some((name) => name.endsWith(".json"))) {
    return true;
  }

  const localeDirectory = path.join(root, "locale");
  if (existsSync(localeDirectory) && sourceFiles(localeDirectory).length > 0) {
    return true;
  }

  const localizationMacro = /\b(?:track_locales|translate|translation)\s*!/;
  return ["src", "tests"]
    .flatMap((directory) => sourceFiles(path.join(root, directory)))
    .some((file) => localizationMacro.test(readFileSync(file, "utf8")));
}

function packageRoots(packages, workspace) {
  return packages
    .map((pkg) => {
      const manifest = path.resolve(pkg.manifest_path);
      const packageRoot = path.dirname(manifest);
      const root = path.relative(workspace, packageRoot).split(path.sep).join("/") || ".";
      const dependencies = new Set(
        pkg.dependencies.map((dependency) =>
          typeof dependency === "string" ? dependency : dependency.name,
        ),
      );
      return {
        name: pkg.name,
        root,
        dependencies,
        localized: pkg.localization ?? hasLocalization(packageRoot),
      };
    })
    .sort((left, right) => right.root.length - left.root.length);
}

function findPackage(file, packages) {
  for (const pkg of packages) {
    if (pkg.root === ".") {
      if (file === "src" || file.startsWith("src/") || file === "tests" || file.startsWith("tests/")) {
        return pkg.name;
      }
    } else if (file === pkg.root || file.startsWith(`${pkg.root}/`)) {
      return pkg.name;
    }
  }
  return null;
}

function dependsOn(packageName, target, dependencies, visited = new Set()) {
  if (packageName === target) return true;
  if (visited.has(packageName)) return false;
  visited.add(packageName);
  return [...(dependencies.get(packageName) ?? [])].some(
    (dependency) => dependency === target || dependsOn(dependency, target, dependencies, visited),
  );
}

function fullScope(reason) {
  return {
    full: true,
    locales: [...ALL_LOCALES],
    hostPackages: [],
    guestPackages: [],
    fontLocales: [...FONT_LOCALES],
    reason,
  };
}

export function selectScope(paths, metadataPackages, workspace = process.cwd()) {
  const changedFiles = [...new Set(paths.map((file) => file.replace(/^\.\//, "")))].sort();
  if (changedFiles.length === 0) return fullScope("no PR file list");

  const packages = packageRoots(metadataPackages, workspace);
  const names = new Set(packages.map((pkg) => pkg.name));
  const dependencies = new Map(packages.map((pkg) => [pkg.name, pkg.dependencies]));
  const localized = new Map(packages.map((pkg) => [pkg.name, pkg.localized]));
  const changedPackages = new Set();
  const catalogLocales = new Set();
  const fontLocales = new Set();
  let hasCodeChange = false;
  let workspaceConfigChanged = false;

  for (const file of changedFiles) {
    if (file === "Cargo.toml" || file === "Cargo.lock") {
      workspaceConfigChanged = true;
      continue;
    }
    if (isFullCheckPath(file)) {
      return fullScope(`shared localization/build input changed: ${file}`);
    }

    const packageName = findPackage(file, packages);
    if (!packageName) return fullScope(`cannot map changed file to a Cargo package: ${file}`);

    if (file.endsWith(".rs")) {
      hasCodeChange = true;
      if (localized.get(packageName)) changedPackages.add(packageName);
    } else if (file.endsWith("/Cargo.toml")) {
      hasCodeChange = true;
      if (localized.get(packageName)) changedPackages.add(packageName);
    } else if (file.includes("/locales/") && file.endsWith(".json")) {
      changedPackages.add(packageName);
      const locale = path.posix.basename(file, ".json");
      if (!ALL_LOCALES.includes(locale)) {
        return fullScope(`unsupported catalog locale: ${locale}`);
      }
      catalogLocales.add(locale);
      if (FONT_LOCALES.includes(locale)) fontLocales.add(locale);
    } else {
      return fullScope(`unclassified localization input: ${file}`);
    }
  }

  if (changedPackages.size === 0 && catalogLocales.size === 0 && workspaceConfigChanged) {
    return fullScope("workspace configuration changed without a package-level localization scope");
  }
  if (changedPackages.size === 0 && catalogLocales.size === 0) {
    return {
      full: false,
      locales: [],
      hostPackages: [],
      guestPackages: [],
      fontLocales: [],
      reason: "changed Rust packages contain no localization catalogs or macros",
    };
  }

  const locales = hasCodeChange
    ? [...ALL_LOCALES]
    : ALL_LOCALES.filter((locale) => catalogLocales.has(locale));
  const guestPackages = GUEST_PACKAGES.filter(
    (guest) =>
      names.has(guest) &&
      [...changedPackages].some((changed) => dependsOn(guest, changed, dependencies)),
  ).sort();

  return {
    full: false,
    locales,
    hostPackages: [...changedPackages].sort(),
    guestPackages,
    fontLocales: FONT_LOCALES.filter((locale) => fontLocales.has(locale)),
    reason: "changed package and locale scope",
  };
}

function workspacePackages() {
  const metadata = JSON.parse(
    execFileSync(
      "cargo",
      ["metadata", "--format-version", "1", "--locked", "--filter-platform", "wasm32-wasip1"],
      { encoding: "utf8", maxBuffer: 64 * 1024 * 1024 },
    ),
  );
  const packageNames = new Map(metadata.packages.map((pkg) => [pkg.id, pkg.name]));
  const dependencies = new Map(
    metadata.resolve.nodes.map((node) => [
      node.id,
      [...new Set(node.deps.map((dependency) => packageNames.get(dependency.pkg)))],
    ]),
  );
  const members = new Set(metadata.workspace_members);
  return metadata.packages
    .filter((pkg) => members.has(pkg.id))
    .map((pkg) => ({
      ...pkg,
      dependencies: dependencies.get(pkg.id) ?? [],
    }));
}

function main() {
  const paths =
    process.env.GITHUB_EVENT_NAME === "pull_request"
      ? JSON.parse(process.env.LOCALIZATION_FILES || "[]")
      : [];
  const scope = selectScope(paths, workspacePackages());
  const outputs = {
    full: String(scope.full),
    locales: JSON.stringify(scope.locales),
    host_packages: JSON.stringify(scope.hostPackages),
    guest_packages: JSON.stringify(scope.guestPackages),
    font_locales: JSON.stringify(scope.fontLocales),
  };
  for (const [key, value] of Object.entries(outputs)) console.log(`${key}=${value}`);
  console.error(`Localization scope: ${scope.reason}`);
}

if (process.argv[1] && fileURLToPath(import.meta.url) === path.resolve(process.argv[1])) {
  main();
}
