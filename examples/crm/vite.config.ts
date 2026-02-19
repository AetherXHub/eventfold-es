import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
    root: "src-frontend",
    plugins: [react(), tailwindcss()],
    resolve: {
        alias: {
            "@": fileURLToPath(new URL("./src-frontend/src", import.meta.url)),
        },
    },
    build: {
        outDir: "../dist",
        emptyOutDir: true,
    },
    server: {
        port: 5173,
        strictPort: true,
    },
});
