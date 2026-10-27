import { defineConfig } from 'vite';
export default defineConfig({
  root: 'apps/desktop',
  server: { host: '127.0.0.1', port: 1427, strictPort: true },
  build: { outDir: '../../dist', emptyOutDir: true, target: 'es2022' },
});
