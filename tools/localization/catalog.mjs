/**
 * Normalize a Tolgee/embedded-i18n catalog to dotted semantic keys.
 *
 * Accepts flat keys, nested JSON groups, and a mixture of both. Refuse
 * duplicate flattened keys instead of silently dropping a translation
 * during an import/export round trip.
 */
export function flattenCatalog(catalog) {
  if (catalog === null || Array.isArray(catalog) || typeof catalog !== "object") {
    throw new TypeError("expected a translation JSON object");
  }

  const translations = Object.create(null);
  function visit(node, prefix = "") {
    for (const [segment, value] of Object.entries(node)) {
      const key = prefix ? prefix + "." + segment : segment;
      if (value !== null && !Array.isArray(value) && typeof value === "object") {
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
        throw new Error("translation path collision: " + ancestor + " / " + key);
      }
    }
  }
  return translations;
}
