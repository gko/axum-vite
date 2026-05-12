import { resolve } from 'path'
import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// https://vite.dev/config/
export default defineConfig({
  plugins: [react()],
  // base must match ViteConfig::prefix. The crate default is "/static/" and
  // this example uses that default, so no VITE_STATIC_PREFIX env var is needed.
  // If you change this, set VITE_STATIC_PREFIX to the same value.
  base: '/static/',
  build: {
    // Generate dist/.vite/manifest.json so the Rust binary can resolve
    // content-hashed asset paths at startup via EntryAssets::from_config.
    manifest: true,
    rollupOptions: {
      input: {
        // Primary SPA entry — used by home.html and any page that needs the
        // full React app.
        main: resolve(__dirname, 'index.html'),
        // Secondary entry — loaded only on /dashboard. Produces a separate
        // chunk so visitors to other pages never download this code.
        // The manifest key becomes "src/widget.tsx".
        widget: resolve(__dirname, 'src/widget.tsx'),
      },
    },
  },
})
