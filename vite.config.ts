import react from '@vitejs/plugin-react';
import { defineConfig } from 'vite';

// Porta fixa: o tauri.conf.json (devUrl) aponta para ela.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5183, strictPort: true },
  envPrefix: ['VITE_', 'TAURI_ENV_'],
  build: {
    target: 'chrome110',
    sourcemap: false,
  },
});
