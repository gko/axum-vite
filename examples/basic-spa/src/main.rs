//! Basic example: Axum server with axum-vite dev proxy + HMR injection.
//!
//! **Dev mode** — proxy to a running Vite dev server:
//!
//! ```bash
//! # Terminal 1 — Vite dev server
//! cd examples/frontend && npm install && npm run dev
//!
//! # Terminal 2 — Axum server
//! cargo run --example basic
//! ```
//!
//! **Release mode** — embed the built dist into the binary at compile time:
//!
//! ```bash
//! cd examples/frontend && npm run build && cd ../..
//! cargo build --release --example basic
//! ./target/release/examples/basic
//! ```
//!
//! The `examples/frontend/dist` directory is baked into the binary at compile time —
//! no environment variables required. The dist folder can be deleted afterwards.
//! `VITE_ROOT` is the dev-mode project root (where `package.json` lives),
//! only used when running without `--release`.
//!
//! Open http://localhost:3000

use axum::Router;
#[allow(unused_imports)]
use axum_vite::{ViteConfig, spa_router, spawn_dev_server};
use tokio::net::TcpListener;

#[tokio::main]
async fn main() {
    env_logger::init();

    // embedded_dir! embeds the dist folder at compile time in release builds
    // and returns None in debug builds — no #[cfg] boilerplate required.
    let config = ViteConfig::from_env(axum_vite::embedded_dir!(
        "$CARGO_MANIFEST_DIR/frontend/dist"
    ));

    // Optional: automatically start the Vite dev server (dev mode only).
    // The handle must be kept alive — dropping it kills the child process.
    #[cfg(debug_assertions)]
    let _dev_server = config
        .auto_start
        .then(|| match spawn_dev_server(&config) {
            Ok(handle) => {
                log::info!("Vite dev server spawned");
                Some(handle)
            }
            Err(e) => {
                log::warn!("Failed to spawn Vite dev server: {}", e);
                None
            }
        })
        .flatten();

    let app = Router::new()
        // Your API routes go here — they take priority over the SPA catch-all.
        // .route("/api/hello", get(|| async { "hello" }))
        .merge(spa_router(config));

    let addr = "127.0.0.1:3000";
    let listener = TcpListener::bind(addr).await.unwrap();
    log::info!("listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
