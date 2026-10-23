// Generate/cache C sources only. C compilation belongs to graphics/build.rs.
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { digest, ensureOutput, converterInputs } from "./cache.mjs";

const FONTS = [
  [10, "Montserrat-Regular.ttf", "montserrat", true],
  [14, "Montserrat-Regular.ttf", "montserrat", true],
  [18, "Montserrat-Regular.ttf", "montserrat", true],
  [28, "Montserrat-Regular.ttf", "montserrat", true],
  [8, "unscii-16-full.woff", "unscii", false],
  [16, "unscii-16-full.woff", "unscii", false],
];
const ICON_FILE = "FontAwesome5-Solid+Brands+Regular.woff";
const ICON_RANGES = [
  61441, 61448, 61451, 61452, 61452, 61453, 61457, 61459, 61461, 61465, 61468,
  61473, 61478, 61479, 61480, 61502, 61512, 61515, 61516, 61517, 61521, 61522,
  61523, 61524, 61543, 61544, 61550, 61552, 61553, 61556, 61559, 61560, 61561,
  61563, 61587, 61589, 61636, 61637, 61639, 61671, 61674, 61683, 61724, 61732,
  61787, 61931, 62016, 62017, 62018, 62019, 62020, 62087, 62099, 62212, 62189,
  62810, 63231, 63426, 63650,
].join(",");

export function converterArguments(source, output) {
  const args = [
    "--bpp",
    "4",
    "--size",
    String(source.size),
    "--no-compress",
    "--stride",
    "0",
    "--align",
    "1",
    "--font",
    source.font,
    "-r",
    source.ranges,
  ];
  if (source.icons) args.push("--font", source.icons, "-r", ICON_RANGES);
  args.push("--format", "lvgl", "--lv-font-name", source.name, "-o", output);
  return args;
}

export function planSources(root, output, ranges, commonIdentity) {
  return FONTS.map(([size, file, family, icons]) => {
    const source = {
      size,
      name: `font_${family}_${size}`,
      font: join(root, "fonts", file),
      ranges,
    };
    if (icons) source.icons = join(root, "fonts", ICON_FILE);
    const identity = digest([
      "xila-font-source-v1",
      commonIdentity,
      JSON.stringify(converterArguments(source, "OUTPUT")),
      readFileSync(source.font),
      ...(source.icons ? [readFileSync(source.icons)] : []),
    ]);
    const path = join(
      output,
      digest([ranges]),
      source.name,
      identity,
      `${source.name}.generated.c`,
    );
    return { ...source, identity, path };
  });
}

function main() {
  const [rootArg, outputArg, active, ...variants] = process.argv.slice(2);
  const root = resolve(rootArg);
  const output = resolve(outputArg);
  const converter = join(root, "../../node_modules/lv_font_conv");
  const dependencies = converterInputs(converter);
  const watched = [
    ...dependencies.paths,
    process.execPath,
    join(root, "../../package-lock.json"),
    ...[
      "build.rs",
      "build/fonts.rs",
      "build/cache.mjs",
      "build/generate.mjs",
    ].map((path) => join(root, path)),
  ];
  const common = digest([
    dependencies.identity,
    process.execPath,
    process.version,
    ...watched
      .filter((path) => !dependencies.paths.includes(path))
      .flatMap((path) => [path, readFileSync(path)]),
  ]);
  const sources = new Map();
  let generated = 0,
    reused = 0;
  for (const ranges of new Set([active, ...variants])) {
    const plan = planSources(root, output, ranges, common);
    sources.set(ranges, plan);
    for (const source of plan) {
      const changed = ensureOutput(
        source.path,
        source.identity,
        (temporary) => {
          execFileSync(
            process.execPath,
            [
              join(converter, "lv_font_conv.js"),
              ...converterArguments(source, temporary),
            ],
            { stdio: ["ignore", "pipe", "pipe"] },
          );
        },
      );
      if (changed) generated++;
      else reused++;
      watched.push(
        source.font,
        ...(source.icons ? [source.icons] : []),
        source.path,
        `${source.path}.json`,
      );
    }
  }
  for (const path of new Set(watched))
    console.log(`cargo:rerun-if-changed=${path}`);
  console.log(
    `cargo:warning=Font sources: generated ${generated}, reused ${reused}`,
  );
  // The only paths handed to cc; never discover sources by scanning OUT_DIR.
  for (const source of sources.get(active))
    console.log(`source\t${source.path}`);
}

if (
  process.argv[1] &&
  resolve(process.argv[1]) === fileURLToPath(import.meta.url)
)
  main();
