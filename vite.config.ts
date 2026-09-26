import { defineConfig } from "vitest/config";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    // Rust executables are locked during linking/running on Windows.
    watch: { ignored: ["**/target/**", "**/src-tauri/**", "**/.tools/**", "**/.artifacts/**", "**/downloads/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: { target: "chrome105" },
  test: {
    environment: "jsdom",
    setupFiles: ["./src/test/setup.ts"],
    restoreMocks: true,
  },
});
