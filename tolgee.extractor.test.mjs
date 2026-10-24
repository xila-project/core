import assert from "node:assert/strict";
import { test } from "node:test";
import extract from "./tolgee.extractor.mjs";

const clock = "executables/clock/src/interface.rs";
const shell = "executables/shell/command_line/src/commands/dns.rs";
const summarize = result => result.keys.map(({ keyName, namespace }) => [keyName, namespace]);

test("extracts semantic keys and preserves per-crate namespaces", () => {
  assert.deepEqual(summarize(extract(
    'translate!("tabs.clock");\ntranslate!(c"timer.controls.start");', clock
  )), [
    ["tabs.clock", "executables/clock"],
    ["timer.controls.start", "executables/clock"],
  ]);
});

test("extracts locale-specific CLDR categories without inventing forms", () => {
  const code = 'embedded_i18n::translate_plural_write!(&mut output, "stopwatch.laps.recorded", count);';
  assert.deepEqual(extract(code, clock).keys.map(x => x.keyName), [
    "stopwatch.laps.recorded.few",
    "stopwatch.laps.recorded.many",
    "stopwatch.laps.recorded.one",
    "stopwatch.laps.recorded.other",
  ]);
  assert.deepEqual(extract('translate_plural!("not.in.catalog", count)', clock).keys, []);
});

test("extracts plural forms from the shell namespace", () => {
  assert.deepEqual(extract(
    'xila::internationalization::translate_plural!("dns.records.unit", count)', shell
  ).keys.map(x => x.keyName), [
    "dns.records.unit.few", "dns.records.unit.many",
    "dns.records.unit.one", "dns.records.unit.other",
  ]);
});

test("handles context arguments and qualified macro paths", () => {
  const source = [
    'embedded_i18n::translate!(context = "menu", "file.open");',
    'xila::internationalization::translate_write!(context = "errors", &mut output, "missing", name = value);',
    'translate_args!(context = "labels", "user_count", count = 1);',
  ].join("\n");
  assert.deepEqual(extract(source, clock).keys.map(x => x.keyName), [
    "menu.file.open", "errors.missing", "labels.user_count",
  ]);
});

test("supports C-string macros, multiline formatting and write macros", () => {
  const source = [
    'translate!(\n  c"tabs.alarm"\n);',
    'translate_write!(\n  &mut output,\n  "errors.message",\n  code = 42,\n);',
    'translate_select_write!(&mut output, "tabs", selected);',
  ].join("\n");
  const keys = extract(source, clock).keys;
  assert.equal(keys[0].keyName, "tabs.alarm");
  assert.equal(keys[0].line, 1);
  assert.equal(keys[1].keyName, "errors.message");
  assert.ok(keys.some(x => x.keyName === "tabs.clock"));
  assert.ok(keys.some(x => x.keyName === "tabs.timer"));
});

test("ignores nested comments, strings, and unrelated macro names", () => {
  const source = [
    '// translate!("fake.comment")',
    '/* outer /* nested translate!(c"fake.nested") */ translate!("fake.block") */',
    'let sample = r#"translate!(\"fake.raw\")"#;',
    'let regular = "translate!(\"fake.string\")";',
    'not_translate!("fake.other");',
    'translate!(c"tabs.clock");',
  ].join("\n");
  assert.deepEqual(extract(source, clock).keys.map(x => x.keyName), ["tabs.clock"]);
});

test("select extraction discovers variants that exist in catalogs", () => {
  const result = extract('translate_select!("tabs", selected)', clock);
  assert.deepEqual(result.keys.map(x => x.keyName), [
    "tabs.alarm", "tabs.clock", "tabs.stopwatch", "tabs.timer",
  ]);
});

test("non-Rust files are ignored", () => {
  assert.deepEqual(extract('translate!("fake")', "readme.md").keys, []);
});

test("covers semantic translation keys in inline and separate Rust tests", () => {
  const inlineTest = `
    #[cfg(test)]
    mod tests {
      fn checks_help() {
        assert_eq!(translate!("help.built_in_commands_title"), "Built-in commands:");
        assert!(output.contains(
          xila::internationalization::translate!("help.external_executable_description")
        ));
      }
    }
  `;
  const separateTest = 'assert!(message.contains(xila::internationalization::translate!("errors.line_too_long")));';
  assert.deepEqual(
    extract(inlineTest, "executables/shell/command_line/src/commands/mod.rs")
      .keys.map(x => x.keyName),
    ["help.built_in_commands_title", "help.external_executable_description"],
  );
  assert.deepEqual(
    extract(separateTest, "executables/shell/command_line/src/execution_tests.rs")
      .keys.map(x => x.keyName),
    ["errors.line_too_long"],
  );
});
