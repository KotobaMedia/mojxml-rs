import { defineConfig } from 'vite';

export default defineConfig({
  server: {
    fs: {
      allow: ['.', '../crates/wasm/pkg'],
    }
  },
});
