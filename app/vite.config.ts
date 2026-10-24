import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5183, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/target/**"] } },
  build: { target: "chrome110", outDir: "dist", emptyOutDir: true, rollupOptions: { output: {
    manualChunks(id) {
      if (id.includes("/src/v2/data/")) return "catalog-snapshots";
      if (id.includes("/node_modules/") && /[\\/]react(?:-dom)?[\\/@]/.test(id)) return "react-vendor";
    },
  } } },
});
