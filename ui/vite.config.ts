import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

export default defineConfig({
  plugins: [react()],
  build: { assetsDir: 'assets', assetsInlineLimit: 0 },
  test: { environment: 'jsdom', maxWorkers: 2 },
});
