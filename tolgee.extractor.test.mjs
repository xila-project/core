import assert from "node:assert/strict";
import { test } from "node:test";
import extract from "./tolgee.extractor.mjs";

const fileName = "executables/clock/src/interface.rs";

// The extractor discovers the union of plural categories across all installed
// locales. Polish adds `few` and `many` beyond English's `one` and `other`.
const pluralCategories = ["few", "many", "one", "other"];

function expectedPluralKeys(keyName) {
  return pluralCategories.map((category) => `${keyName}.${category}`);
}

test("extracts ordinary strings without changing existing namespaces", () => {
  const result = extract('translate!("Clock");\ntranslate!(c"Start");', fileName);
  assert.deepEqual(result.keys.map(({ keyName, namespace }) => [keyName, namespace]), [
    ["Clock", "executables/clock"],
    ["Start", "executables/clock"],
  ]);
});

test("discovers plural forms from the existing flat Tolgee catalogs", () => {
  const result = extract(
    'embedded_i18n::translate_plural_write!(&mut laps, "Recorded laps", count);',
    fileName,
  );
  assert.deepEqual(
    result.keys.map(({ keyName }) => keyName),
    expectedPluralKeys("Recorded laps"),
  );
});

test("supports lookup-style plural macros too", () => {
  const result = extract('translate_plural!("Recorded laps", count);', fileName);
  assert.deepEqual(
    result.keys.map(({ keyName }) => keyName),
    expectedPluralKeys("Recorded laps"),
  );
});

test("extracts all plural forms across app and shell translation namespaces", () => {
  for (const [key, file, expectedNamespace] of [
    ["Recorded laps", "executables/clock/src/interface.rs", "executables/clock"],
    [
      "DNS records for domain",
      "executables/shell/command_line/src/commands/dns.rs",
      "executables/shell/command_line",
    ],
    [
      "PING data bytes",
      "executables/shell/command_line/src/commands/ping.rs",
      "executables/shell/command_line",
    ],
    [
      "ICMP bytes from",
      "executables/shell/command_line/src/commands/ping.rs",
      "executables/shell/command_line",
    ],
  ]) {
    const result = extract(
      `xila::internationalization::translate_plural!("${key}", count);`,
      file,
    );
    assert.deepEqual(
      result.keys.map(({ keyName, namespace }) => [keyName, namespace]),
      expectedPluralKeys(key).map((keyName) => [keyName, expectedNamespace]),
    );
  }
});
