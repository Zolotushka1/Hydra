import { defineConfig } from "vite";
import solid from "vite-plugin-solid";

export default defineConfig({
  plugins: [solid()],
  server: {
    port: 4173,
    proxy: {
      "/api": "http://127.0.0.1:8080",
      "/sub": "http://127.0.0.1:8080",
    },
  },
  preview: {
    port: 4173,
  },
  build: {
    target: "es2022",
    sourcemap: true,
  },
});
