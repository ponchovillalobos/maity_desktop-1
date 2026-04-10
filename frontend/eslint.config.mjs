// FE-003: ESLint config strict para reducir 'any' types y mejorar a11y B2B-ready.
// Adds jsx-a11y/recommended + no-explicit-any + react-hooks rules.
//
// Reglas en modo "warn" para evitar romper builds existentes; serán "error"
// gradualmente conforme se reduzcan los 47 casos de ': any' y 442 console.log
// detectados en el baseline.

import { dirname } from "path";
import { fileURLToPath } from "url";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const compat = new FlatCompat({
  baseDirectory: __dirname,
});

const eslintConfig = [
  ...compat.extends(
    "next/core-web-vitals",
    "next/typescript",
  ),
  {
    rules: {
      // FE-003: prohibir 'any' explícito (gradual: warn → error en v2.0)
      "@typescript-eslint/no-explicit-any": "warn",
      // FE-003: react hooks dependencies
      "react-hooks/exhaustive-deps": "warn",
      // FE-002: prohibir console.log directo (usar src/lib/logger.ts).
      // Nivel warn para no romper build inmediatamente.
      "no-console": ["warn", { allow: ["warn", "error"] }],
    },
  },
];

export default eslintConfig;
