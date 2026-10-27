import { globSync } from "glob";
import * as path from "path";

type PushFile = {
  path: string;
  language: string;
  namespace: string;
};

type TolgeeConfig = {
  apiUrl: string;
  projectId: number;
  format: "JSON_C";
  extractor?: string;
  patterns?: string[];
  push: {
    files: PushFile[];
    removeOtherKeys: boolean;
  };
  pull: {
    path: string;
    delimiter?: string | null;
    fileStructureTemplate: string;
  };
};

// 1. Find all your translation files
// This looks for: ./subsystems/**/locales/*.json, ./libraries/**/locales/*.json and ./executables/**/locales/*.json
const files = globSync("./**/locales/*.json");

// 2. Map these files to Tolgee's expected format
const pushFiles: PushFile[] = files.map((filePath) => {
  // Extract namespace: everything before "/locales/"
  // Example: "libraries/authentication/locales/en.json" -> "libraries/authentication"
  const namespace = filePath
    .split(`${path.sep}locales${path.sep}`)[0]
    .replace(/^\.\//, "");

  console.log(`Processing file: ${filePath}: namespace: ${namespace}`);

  // Extract language: the filename without extension
  // Example: "en.json" -> "en"
  const language = path.basename(filePath, ".json");

  return {
    path: filePath,
    language,
    namespace,
  };
});

const config: TolgeeConfig = {
  apiUrl: "https://translate.anneraud.fr",
  projectId: 2,
  format: "JSON_C",
  extractor: "./tolgee.extractor.mjs",
  patterns: [
    "./src/**/*.rs",
    "./subsystems/**/src/**/*.rs",
    "./libraries/**/src/**/*.rs",
    "./drivers/core/**/src/**/*.rs",
  ],
  push: {
    files: pushFiles, // Use the dynamically generated list
    removeOtherKeys: false, // Do not delete existing Tolgee namespaces during the layout migration
  },
  pull: {
    path: "./",
    delimiter: ".", // Tolgee structured JSON: split dotted semantic keys into nested objects
    fileStructureTemplate: "{namespace}/locales/{languageTag}.json",
  },
};

export default config;
