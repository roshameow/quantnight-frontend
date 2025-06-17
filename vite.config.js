import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "path"; // ✅ 修复：导入 path 模块
import fs from 'fs'


const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/
export default defineConfig(async () => ({
  plugins: [vue(),
  {
    name: 'ensure-config-js',
    apply: 'build', // 只在构建时运行
    buildStart() {
      const configPath = path.resolve(__dirname, 'src/config.js')
      if (!fs.existsSync(configPath)) {
        fs.writeFileSync(
          configPath,
          `export const AppConfig = {};\n`,
          'utf-8'
        )
        console.log('[vite] ✅ 自动生成空的 src/config.js')
      }
    },
  },

  ],
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "src"),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
        protocol: "ws",
        host,
        port: 1421,
      }
      : undefined,
    watch: {
      // 3. tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
}));
