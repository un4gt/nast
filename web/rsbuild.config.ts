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
      '/api/tts': {
        target: 'http://127.0.0.1:8000',
      },
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
    tags: [
      { tag: 'link', attrs: { rel: 'icon', type: 'image/svg+xml', href: '/favicon.svg' } },
      { tag: 'link', attrs: { rel: 'icon', type: 'image/png', sizes: '32x32', href: '/favicon-32.png' } },
      { tag: 'link', attrs: { rel: 'icon', type: 'image/x-icon', href: '/favicon.ico' } },
      { tag: 'link', attrs: { rel: 'apple-touch-icon', sizes: '180x180', href: '/apple-touch-icon.png' } },
    ],
  },
});
