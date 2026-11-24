import { defineConfig } from 'vite';
import vue from '@vitejs/plugin-vue';

// Tauri expects a fixed dev port and leaves the terminal output alone.
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: '127.0.0.1', watch: { ignored: ['**/src-tauri/**'] } },
  build: { target: 'es2022', cssCodeSplit: false, sourcemap: false },
  define: { __VUE_PROD_DEVTOOLS__: false, __VUE_OPTIONS_API__: false },
});
