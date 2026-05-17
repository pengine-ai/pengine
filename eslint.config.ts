import js from "@eslint/js";
import tseslint from "typescript-eslint";
import reactHooks from "eslint-plugin-react-hooks";

export default tseslint.config(
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    ignores: [
      "dist/**",
      "node_modules/**",
      "src-tauri/**",
      "playwright-report/**",
      "test-results/**",
    ],
  },
  // Node ESM smoke scripts under tools/ (console + process; not browser code).
  {
    files: ["tools/mcp-probe-filemanager/**/*.mjs"],
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: "module",
      globals: {
        console: "readonly",
        process: "readonly",
      },
    },
    rules: {
      "no-console": "off",
    },
  },
  {
    files: ["**/*.{ts,tsx,js,jsx}"],
    plugins: {
      "react-hooks": reactHooks,
    },
    rules: {
      "max-lines": ["warn", { max: 1000, skipBlankLines: true, skipComments: true }],
      "no-console": "warn",
      "no-debugger": "error",
      "no-var": "error",
      "prefer-const": "error",
      "prefer-template": "error",
      "no-useless-return": "error",
      "no-useless-concat": "error",
      "no-loop-func": "error",
      "no-iterator": "error",
      "no-duplicate-imports": "warn",
      "no-duplicate-case": "error",
      "no-dupe-keys": "error",
      "no-dupe-class-members": "error",
      "no-dupe-else-if": "warn",
      "max-params": ["warn", 6],
      complexity: ["warn", { max: 40 }],
      "max-lines-per-function": [
        "warn",
        {
          max: 450,
          skipBlankLines: true,
          skipComments: true,
          IIFEs: true,
        },
      ],
      "max-depth": ["warn", 4],
      "max-statements": ["warn", { max: 55 }],
      "@typescript-eslint/no-explicit-any": "warn",
      "@typescript-eslint/no-unused-vars": [
        "error",
        {
          argsIgnorePattern: "^_",
          varsIgnorePattern: "^_",
          caughtErrorsIgnorePattern: "^_",
          args: "none",
        },
      ],
      "react-hooks/rules-of-hooks": "error",
      "react-hooks/exhaustive-deps": "warn",
    },
  },
);
