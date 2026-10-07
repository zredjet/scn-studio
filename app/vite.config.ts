// Viteの設定。Tauriの手順（create-tauri-app のひな形）に合わせる。
import process from "node:process";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],
  // Rustのエラーを隠さない。
  clearScreen: false,
  server: {
    // Tauriは決まったポートを使う。空いていなければ失敗させる。
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
});
