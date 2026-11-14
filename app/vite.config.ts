import { defineConfig } from "vite";
import preact from "@preact/preset-vite";

export default defineConfig({
  plugins: [preact()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  build: {
    target: ["es2021", "safari15"],
    cssMinify: true,
    reportCompressedSize: false,
    chunkSizeWarningLimit: 300,
  },
});
