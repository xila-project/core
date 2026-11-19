/**
 * Normalize a Tolgee/embedded-i18n catalog to dotted semantic keys.
 *
 * Accepts flat keys, nested JSON groups, and a mixture of both. Refuse
 * duplicate flattened keys instead of silently dropping a translation
 * during an import/export round trip.
 */
export function flattenCatalog(catalog) {
  if (
    catalog === null ||
    Array.isArray(catalog) ||
    typeof catalog !== "object"
  ) {
    throw new TypeError("expected a translation JSON object");
  }

  const translations = Object.create(null);
  function visit(node, prefix = "") {
    for (const [segment, value] of Object.entries(node)) {
      const key = prefix ? prefix + "." + segment : segment;
      if (
        value !== null &&
        !Array.isArray(value) &&
        typeof value === "object"
      ) {
        visit(value, key);
      } else {
        if (Object.hasOwn(translations, key)) {
          throw new Error("duplicate translation key: " + key);
        }
        translations[key] = value;
      }
    }
  }
  visit(catalog);
  for (const key of Object.keys(translations)) {
    const segments = key.split(".");
    for (let i = 1; i < segments.length; i++) {
      const ancestor = segments.slice(0, i).join(".");
      if (Object.hasOwn(translations, ancestor)) {
        throw new Error(
          "translation path collision: " + ancestor + " / " + key,
        );
      }
    }
  }
  return translations;
}

/**
 * Report French typography problems in one translation value.
 *
 * French uses a no-break space (U+00A0) before ":", ";", "!", "?" and "»" and
 * after "«", and typographic apostrophes. U+202F is rejected because the
 * shipped Montserrat and Unscii fonts have no glyph for it. Placeholders such
 * as {arg0:?} and URLs with a scheme followed by // are skipped. Straight
 * quotes and apostrophes inside backtick code spans are also ignored.
 */
export function frenchTypographyIssues(value) {
  let text = value;
  for (let previous = ""; previous !== text;) {
    previous = text;
    text = text.replace(/\{[^{}]*\}/g, "\uE000");
  }
  const prose = text.replace(/\b[a-z][a-z\d+.-]*:\/\/[^\s<>"«»`]+/gi, "x");
  const outsideCode = prose.replace(/`[^`\r\n]*`/g, "");
  const rules = [
    [
      /[  ](?=[:;!?»])/,
      "use U+00A0, not a plain or narrow space, before : ; ! ? or »",
    ],
    // A colon between two placeholders can be a time separator, as in {h}:{m}.
    [
      /(?!\uE000:\uE000)[^\s;!?][:;!?](?=[\s;!?\uE000]|$)/,
      "missing U+00A0 before : ; ! or ?",
    ],
    [/«(?! )/, "missing U+00A0 after «"],
    [/[^ ]»/, "missing U+00A0 before »"],
    [/'/, "use the typographic apostrophe ’", outsideCode],
    [/"/, "use « » for quotation marks", outsideCode],
    [/ /, "U+202F has no glyph in the shipped fonts; use U+00A0"],
  ];
  return rules
    .filter(([pattern, , input = prose]) => pattern.test(input))
    .map(([, message]) => message);
}
