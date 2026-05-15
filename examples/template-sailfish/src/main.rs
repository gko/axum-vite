//! template-sailfish example: Axum + Sailfish templates + axum-vite.
//!
//! Demonstrates **Option B**: your template engine owns `index.html`.
//! axum-vite handles:
//!   - **Dev**: proxying `/static/…` to the Vite dev server + HMR preamble via `hmr_scripts()`.
//!   - **Prod**: serving embedded, content-hashed assets directly from the binary.
//!
//! **Production JS/CSS paths** come from `dist/.vite/manifest.json`, read via
//! `config.entry_assets()` — no runtime file I/O, the JSON is embedded in the binary.
//!
//! This example uses `auto_start: true`, so a single terminal is enough:
//! ```sh
//! cd examples/template-sailfish/frontend && npm install
//! cargo run -p template-sailfish
//! ```
//!
//! Release build:
//! ```sh
//! cd examples/template-sailfish/frontend && npm run build && cd ../../..
//! cargo build --release -p template-sailfish
//! ./target/release/template-sailfish
//! ```

use std::path::PathBuf;
use std::sync::Arc;

use axum::{Router, extract::State, response::Html, routing::get};
use axum_vite::{EntryAssets, ViteConfig, frameworks::Framework, router as asset_router};
use sailfish::TemplateSimple;
use tokio::net::TcpListener;

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    vite: Arc<ViteConfig>,
    /// Resolved once: hashed paths in prod, source paths in dev.
    entry: EntryAssets,
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(TemplateSimple)]
#[template(path = "home.stpl")]
struct IndexPage {
    title: String,
    hmr_scripts: String,
    entry: EntryAssets,
    script_url: Option<String>,
}

#[derive(TemplateSimple)]
#[template(path = "about.stpl")]
struct AboutPage {
    title: String,
    hmr_scripts: String,
    entry: EntryAssets,
    script_url: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

async fn index(State(state): State<AppState>) -> Html<String> {
    Html(
        IndexPage {
            title: "Home".to_string(),
            hmr_scripts: state.vite.hmr_scripts(),
            entry: state.entry.clone(),
            script_url: Some(state.entry.script.clone()),
        }
        .render_once()
        .unwrap(),
    )
}

async fn about(State(state): State<AppState>) -> Html<String> {
    Html(
        AboutPage {
            title: "About".to_string(),
            hmr_scripts: state.vite.hmr_scripts(),
            entry: state.entry.clone(),
            script_url: None,
        }
        .render_once()
        .unwrap(),
    )
}

// ── Main ─────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() {
    env_logger::init();

    let config = ViteConfig {
        auto_start: true,
        framework: Framework::React,
        prefix: "/static/".to_string(),
        frontend_root: Some(PathBuf::from("examples/template-sailfish/frontend/")),
        ..ViteConfig::from_env(axum_vite::embedded_dir!(
            "$CARGO_MANIFEST_DIR/frontend/dist"
        ))
    };

    // Optional: automatically start the Vite dev server (dev mode only).
    // The handle must be kept alive — dropping it kills the child process.
    let _dev_server = config.maybe_spawn_dev_server();

    let entry = config.entry_assets();
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
