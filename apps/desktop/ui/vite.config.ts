import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

// 前端构建配置。
// - 开发：vite dev server（127.0.0.1:5173），Tauri 通过 devUrl 加载
// - 生产：输出 dist/，由 Tauri 以内嵌资源加载（tauri.conf.json frontendDist）
export default defineConfig({
  plugins: [react(), tailwindcss()],
  // vitest 与 vite 共用配置；DOM 环境的测试目前不需要，纯逻辑测试用 node 环境即可
  test: {
    environment: "node",
  },
  server: {
    strictPort: true,
    port: 5173,
  },
  clearScreen: false,
});
