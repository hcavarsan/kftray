import { fixupConfigRules, fixupPluginRules } from "@eslint/compat";
import react from "eslint-plugin-react";
import reactHooks from "eslint-plugin-react-hooks";
import typescriptEslint from "@typescript-eslint/eslint-plugin";
import simpleImportSort from "eslint-plugin-simple-import-sort";
import _import from "eslint-plugin-import";
import tsParser from "@typescript-eslint/parser";
import path from "node:path";
import { fileURLToPath } from "node:url";
import js from "@eslint/js";
import { FlatCompat } from "@eslint/eslintrc";

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);
const compat = new FlatCompat({
    baseDirectory: __dirname,
    recommendedConfig: js.configs.recommended,
    allConfig: js.configs.all
});

export default [{
    ignores: [
        "**/.DS_Store",
        "**/node_modules",
        "build",
        "package",
        "**/.env",
        "**/.env.*",
        "!**/.env.example",
        "**/pnpm-lock.yaml",
        "**/package-lock.json",
        "**/yarn.lock",
        "**/dist/",
        "**/crates/",
        "**/kftray-server/",
		"**/eslint.config.mjs",
    ],
}, ...fixupConfigRules(compat.extends(
    "plugin:react/recommended",
    "plugin:@typescript-eslint/recommended",
    "plugin:import/typescript",
)), {
    plugins: {
        react: fixupPluginRules(react),
        "react-hooks": fixupPluginRules(reactHooks),
        "@typescript-eslint": fixupPluginRules(typescriptEslint),
        "simple-import-sort": simpleImportSort,
        import: fixupPluginRules(_import),
    },

    languageOptions: {
        parser: tsParser,
        ecmaVersion: 2020,
        sourceType: "module",

        parserOptions: {
            ecmaFeatures: {
                jsx: true,
            },

            project: "./tsconfig.eslint.json",
        },
    },

    settings: {
        react: {
            version: "detect",
        },

        "import/resolver": {
            node: {
                extensions: [".js", ".jsx", ".ts", ".tsx"],
            },
        },
    },

    rules: {
        semi: ["error", "never"],

        indent: "off",

        complexity: ["error", {
            max: 30,
        }],

        curly: "error",
        quotes: ["error", "single"],
        "no-magic-numbers": "off",
		"max-len": "off",

        "padding-line-between-statements": ["error", {
            blankLine: "always",
            prev: ["const", "let", "var"],
            next: "*",
        }, {
            blankLine: "any",
            prev: ["const", "let", "var"],
            next: ["const", "let", "var"],
        }],

        "array-bracket-spacing": ["error", "never"],
        "array-callback-return": "error",
        "max-statements": ["error", 50],


        "max-lines-per-function": ["error", 1100],
        "max-params": ["error", 15],
        "newline-after-var": "error",
        "newline-before-return": "error",
        "prefer-arrow-callback": "error",
        "no-shadow": "off",
        "quote-props": ["error", "as-needed"],
        "space-in-parens": ["error", "never"],
        "prefer-const": "error",
        "callback-return": "error",
        "no-empty-function": "error",
        "space-infix-ops": "error",
        "object-curly-spacing": ["error", "always"],
        "simple-import-sort/imports": "error",
        "simple-import-sort/exports": "error",
        "import/first": "error",
        "import/newline-after-import": "error",
        "import/no-duplicates": "error",

        "keyword-spacing": ["error", {
            before: true,
            after: true,
        }],

        "space-before-blocks": "error",

        "comma-spacing": ["error", {
            before: false,
            after: true,
        }],

        "brace-style": "error",
        "no-multi-spaces": "error",
        "react/react-in-jsx-scope": "off",
        "react-hooks/exhaustive-deps": "warn",
    },
}, {
    files: ["**/*.js", "**/*.ts", "**/*.tsx"],

    rules: {
        "react/prop-types": "off",
		"react/display-name": "off",
		"@typescript-eslint/no-empty-object-type": "off",
		"@typescript-eslint/no-explicit-any": "off",

        "@typescript-eslint/no-unused-vars": ["warn", {
            argsIgnorePattern: "^_",
        }],

        "simple-import-sort/imports": ["error", {
            groups: [
                ["^react$", "^next", "^[a-z]"],
                ["^@"],
                ["^@/"],
                ["^~"],
                ["^\\.\\.(?!/?$)", "^\\.\\./?$"],
                ["^\\./(?=.*/)(?!/?$)", "^\\.(?!/?$)", "^\\./?$"],
                ["^.+\\.s?css$"],
                ["^\\u0000"],
            ],
        }],
    },
}, {
    files: ["**/__tests__/**/*.[jt]s?(x)", "**/?(*.)+(spec|test).[jt]s?(x)"],

    rules: {
        "no-magic-numbers": "off",
    },
}, {
    files: ["**/jest.config.js", "**/tailwind.config.js", "**/*.config.js"],

    rules: {
        "@typescript-eslint/no-var-requires": "off",
    },
}];
