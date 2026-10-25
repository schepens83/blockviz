/// <reference types="vitest/config" />
import { defineConfig } from 'vite';
import { gitPlugin } from './server/plugin.ts';
// The git plugin (server/) is Node-only; the page never imports it.
// The page is built from src/ (the core as WebAssembly in src/wasm): the Rust
// sources and Cargo's tens of thousands of build files are not watched. The
// Rust crate's fixtures also include other projects' test files.
export default defineConfig({
  base: './',
  plugins: [gitPlugin()],
  worker: { format: 'es' },
  server: { port: 5173, watch: { ignored: ['**/crates/**', '**/target/**'] } },
  test: { include: ['src/**/*.test.ts', 'server/**/*.test.ts'] },
});
