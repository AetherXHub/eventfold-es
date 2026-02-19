import eslint from "@eslint/js";
import reactHooks from "eslint-plugin-react-hooks";
import reactRefresh from "eslint-plugin-react-refresh";
import tseslint from "typescript-eslint";

export default tseslint.config(
    {
        ignores: ["dist/", "src-frontend/src/vite-env.d.ts"],
    },
    eslint.configs.recommended,
    ...tseslint.configs.strictTypeChecked,
    ...tseslint.configs.stylisticTypeChecked,
    {
        languageOptions: {
            parserOptions: {
                projectService: true,
                tsconfigRootDir: import.meta.dirname,
            },
        },
        plugins: {
            "react-hooks": reactHooks,
            "react-refresh": reactRefresh,
        },
        rules: {
            ...reactHooks.configs.recommended.rules,
            "react-refresh/only-export-components": ["warn", { allowConstantExport: true }],
            "@typescript-eslint/no-explicit-any": "error",
            "@typescript-eslint/no-non-null-assertion": "off",
            "@typescript-eslint/no-confusing-void-expression": "off",
            "@typescript-eslint/no-invalid-void-type": "off",
            "@typescript-eslint/no-misused-promises": [
                "error",
                { checksVoidReturn: { arguments: false, attributes: false } },
            ],
            "@typescript-eslint/restrict-template-expressions": [
                "error",
                { allowNumber: true },
            ],
        },
    },
);
