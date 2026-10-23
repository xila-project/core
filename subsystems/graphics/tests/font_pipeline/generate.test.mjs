import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync, mkdirSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { planSources, converterArguments } from "../../build/generate.mjs";

test("source plans deduplicate coverage and invalidate only relevant font inputs", (t) => {
  const root = mkdtempSync(join(tmpdir(), "xila-font-plan-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  mkdirSync(join(root, "fonts"));
  for (const name of [
    "Montserrat-Regular.ttf",
    "unscii-16-full.woff",
    "FontAwesome5-Solid+Brands+Regular.woff",
  ])
    writeFileSync(join(root, "fonts", name), name);
  const plan = (ranges = "32-126", common = "converter-v1") =>
    planSources(root, join(root, "out"), ranges, common);
  const before = plan();
  assert.equal(before.length, 6);
  assert.deepEqual(
    plan().map((s) => s.path),
    before.map((s) => s.path),
  );
  assert.ok(plan("32-126,160-591").every((s, i) => s.path !== before[i].path));
  assert.ok(
    plan("32-126", "converter-v2").every((s, i) => s.path !== before[i].path),
  );
  writeFileSync(
    join(root, "fonts/FontAwesome5-Solid+Brands+Regular.woff"),
    "new icons",
  );
  const icons = plan();
  assert.ok(icons.slice(0, 4).every((s, i) => s.path !== before[i].path));
  assert.deepEqual(icons.slice(4), before.slice(4));
  writeFileSync(join(root, "fonts/unscii-16-full.woff"), "new unscii");
  const unscii = plan();
  assert.deepEqual(unscii.slice(0, 4), icons.slice(0, 4));
  assert.ok(unscii.slice(4).every((s, i) => s.path !== icons[i + 4].path));
  mkdirSync(join(root, "out"));
  writeFileSync(join(root, "out/unrelated.c"), "must not be compiled");
  assert.equal(plan().length, 6);
});

test("converter options preserve font sizes, symbols, icons and rendering", () => {
  const source = {
    size: 14,
    name: "font_montserrat_14",
    font: "/fonts/Montserrat-Regular.ttf",
    icons: "/fonts/icons.woff",
    ranges: "32-126",
    path: "/out/font.c",
  };
  const args = converterArguments(source, "/out/font.c.partial");
  assert.deepEqual(args.slice(0, 14), [
    "--bpp",
    "4",
    "--size",
    "14",
    "--no-compress",
    "--stride",
    "0",
    "--align",
    "1",
    "--font",
    source.font,
    "-r",
    "32-126",
    "--font",
  ]);
  assert.ok(args.some((argument) => argument.startsWith("61441,")));
  assert.deepEqual(args.slice(-6), [
    "--format",
    "lvgl",
    "--lv-font-name",
    "font_montserrat_14",
    "-o",
    "/out/font.c.partial",
  ]);
});
