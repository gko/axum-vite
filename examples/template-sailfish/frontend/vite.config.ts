import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // base must match ViteConfig::prefix (default: "/static/").
  base: '/static/',
  build: {
    // Required: generates dist/.vite/manifest.json for production asset path resolution.
    manifest: true,
  },
})
