import { defineConfig } from "../../app/node_modules/vite";
import { fileURLToPath } from "node:url";
const root = fileURLToPath(new URL(".", import.meta.url));
export default defineConfig({
  root, esbuild: { jsx: "automatic" }, publicDir: fileURLToPath(new URL("../../app/public", import.meta.url)),
  resolve: { alias: {
    react: fileURLToPath(new URL("../../app/node_modules/react", import.meta.url)),
    "react-dom": fileURLToPath(new URL("../../app/node_modules/react-dom", import.meta.url)),
  } },
  server: { host: "127.0.0.1", port: 5202, strictPort: true, watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/native-test/**"] }, fs: { allow: [fileURLToPath(new URL("../../", import.meta.url))] } },
  build: { outDir: "dist" },
});
