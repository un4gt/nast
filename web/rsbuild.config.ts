import { defineConfig } from '@rsbuild/core';
import { pluginReact } from '@rsbuild/plugin-react';

export default defineConfig({
  plugins: [pluginReact()],
  source: {
    entry: {
      index: './src/main.tsx',
    },
  },
  output: {
    dist: 'dist',
    assetPrefix: '/',
  },
  server: {
    port: 3000,
    proxy: {
      '/ws': {
        target: 'ws://127.0.0.1:8000',
        ws: true,
      },
      '/upload': {
        target: 'http://127.0.0.1:8000',
      },
      '/thumbnail': {
        target: 'http://127.0.0.1:8000',
      },
    },
  },
  html: {
    title: 'nast',
  },
});
