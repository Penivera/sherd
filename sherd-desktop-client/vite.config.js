import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// base: "./" is required so the built index.html/assets resolve correctly
// when Electron loads them with file:// in production (npm run dist).
export default defineConfig({
  plugins: [react()],
  base: "./",
  server: {
    port: 5173,
    strictPort: true,
  },
});
