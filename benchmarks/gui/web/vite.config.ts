import { svelte } from '@sveltejs/vite-plugin-svelte';
import { defineConfig } from 'vite';
import { fileURLToPath } from 'node:url';
import solid from 'vite-plugin-solid';

const root = fileURLToPath(new URL('.', import.meta.url));

export default defineConfig({
  root,
  plugins: [svelte({ configFile: false }), solid()],
  build: {
    outDir: fileURLToPath(new URL('../.build/web', import.meta.url)),
    emptyOutDir: true,
  },
});
