import { defineConfig } from "vite";

export default defineConfig({
  base: "./",
  server: {
    proxy: {
      "/api": {
        target: process.env.WAVEFORM_API_URL ?? "http://127.0.0.1:8091",
        changeOrigin: true,
      },
    },
  },
});
