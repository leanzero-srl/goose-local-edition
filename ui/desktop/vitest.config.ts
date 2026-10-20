/// <reference types="vitest" />
import { defineConfig } from 'vitest/config';
import react from '@vitejs/plugin-react';
import { resolve } from 'node:path';

const cfg = {
  plugins: [react()],
  resolve: {
    alias: {
      '@': resolve(__dirname, './src'),
    },
  },
  test: {
    globals: true,
    environment: 'jsdom',
    setupFiles: ['./src/test/setup.ts'],
    css: true,
    include: ['src/**/*.{test,spec}.{js,jsx,ts,tsx}'],
    // The CI runner is the workhorse at nice 10, deliberately yielding to its video/sound work: measured
    // 2026-10-20, single tests ran 25-38x slower there than idle in bursts (BenchmarkView.forge 13.2 s vs
    // ~0.23 s locally after jsdom 29). Locally the 5 s default still catches a slow test early.
    testTimeout: process.env.CI ? 60_000 : 5_000,
  },
} satisfies Record<string, any>;

export default defineConfig(cfg as any);
