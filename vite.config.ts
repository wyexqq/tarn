import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  clearScreen: false,
  server: { port: 1427, strictPort: true },
  build: {
    target: "chrome110",
    cssMinify: true,
    chunkSizeWarningLimit: 300,
  },
});
