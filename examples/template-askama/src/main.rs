//! template-askama example: Axum + Askama templates + axum-vite.
//!
//! Demonstrates **Option B**: your template engine owns `index.html`.
//! axum-vite handles:
//!   - **Dev**: proxying `/static/…` to the Vite dev server + HMR preamble via `hmr_scripts()`.
//!   - **Prod**: serving embedded, content-hashed assets directly from the binary.
//!
//! **Production JS/CSS paths** come from `dist/.vite/manifest.json`, read via
//! `config.entry_assets("index.html")` — no runtime file I/O, the JSON is embedded in the binary.
//!
//! ```sh
//! # Terminal 1 — Vite dev server
//! cd examples/template-askama/frontend && npm install && npm run dev
//!
//! # Terminal 2 — Axum server (proxies /static/… to Vite)
//! cargo run -p template-askama
//! ```
//!
//! Release build:
//! ```sh
//! cd examples/template-askama/frontend && npm run build && cd ../../..
//! cargo build --release -p template-askama
//! ./target/release/template-askama
//! ```

use std::sync::Arc;

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};
use axum_vite::{EntryAssets, ViteConfig, frameworks::Framework, router as asset_router};
use tokio::net::TcpListener;

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    vite: Arc<ViteConfig>,
    /// Resolved once at startup: hashed paths in prod, source paths in dev.
    entry: EntryAssets,
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "home.html")]
struct IndexPage {
    hmr_scripts: String,
    entry: EntryAssets,
}

#[derive(Template)]
#[template(path = "about.html")]
struct AboutPage {
    hmr_scripts: String,
    entry: EntryAssets,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn index(State(state): State<AppState>) -> Html<String> {
    Html(
        IndexPage {
            hmr_scripts: state.vite.hmr_scripts(),
            entry: state.entry.clone(),
        }
        .render()
        .unwrap(),
    )
}

async fn about(State(state): State<AppState>) -> Html<String> {
    Html(
        AboutPage {
            hmr_scripts: state.vite.hmr_scripts(),
            entry: state.entry.clone(),
        }
        .render()
        .unwrap(),
    )
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    env_logger::init();

    let config = ViteConfig {
        framework: Framework::React,
        ..ViteConfig::from_env(axum_vite::embedded_dir!(
            "$CARGO_MANIFEST_DIR/frontend/dist"
        ))
    };
    let entry = config.entry_assets("index.html");
    let config = Arc::new(config);

    let static_prefix = format!("/{}", config.prefix.trim_matches('/'));

    let app = Router::new()
        .route("/", get(index))
        .route("/about", get(about))
        // Serve static assets: proxied to Vite in dev, embedded in production.
        .nest(&static_prefix, asset_router((*config).clone()))
        .with_state(AppState {
            vite: config,
            entry,
        });

    let addr = "127.0.0.1:3000";
    let listener = TcpListener::bind(addr).await.unwrap();
    log::info!("listening on http://{addr}");
    axum::serve(listener, app).await.unwrap();
}
