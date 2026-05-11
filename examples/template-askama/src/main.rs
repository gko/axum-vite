//! template-askama example: Axum + Askama templates + axum-vite.
//!
//! Demonstrates **Option B**: your template engine owns `index.html`.
//! axum-vite handles:
//!   - **Dev**: proxying `/static/…` to the Vite dev server + HMR preamble via `hmr_scripts()`.
//!   - **Prod**: serving embedded, content-hashed assets directly from the binary.
//!
//! **Production JS/CSS paths** come from `dist/.vite/manifest.json`. `EntryAssets::from_config`
//! reads the manifest at startup — no runtime file I/O, the JSON is embedded in the binary.
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
use axum_vite::{ViteConfig, frameworks::Framework, router as asset_router};
use tokio::net::TcpListener;

// ── App state ────────────────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    vite: Arc<ViteConfig>,
    /// Resolved once at startup: hashed paths in prod, source paths in dev.
    entry: EntryAssets,
}

/// Hashed asset paths for the JS entry point, resolved once at startup.
///
/// In **dev** `script` points at the unbuilt source file — Vite transforms it
/// on the fly. `stylesheets` is empty because Vite injects CSS via the JS
/// module itself in dev mode; no separate `<link>` tag is needed.
///
/// In **production** both come from `dist/.vite/manifest.json` and include
/// the content hash (e.g. `assets/main-A1b2C3.js`).
#[derive(Clone, Default)]
struct EntryAssets {
    /// `src` attribute for the `<script type="module">` tag.
    script: String,
    /// `href` attributes for any `<link rel="stylesheet">` tags.
    stylesheets: Vec<String>,
}

impl EntryAssets {
    fn from_config(config: &ViteConfig) -> Self {
        let base = config.prefix.trim_end_matches('/');

        // `config.dir` is `Some` only in release builds (set by `embedded_dir!`).
        // In dev it's `None`, so the manifest branch is naturally skipped.
        if let Some(dir) = config.dir {
            if let Some(file) = dir.get_file(".vite/manifest.json") {
                if let Some(json) = file.contents_utf8() {
                    return Self::from_manifest(json, base);
                }
            }
        }

        // Dev: Vite serves `src/main.tsx` directly; CSS is injected by the JS module.
        Self {
            script: format!("{base}/src/main.tsx"),
            stylesheets: vec![],
        }
    }

    /// Parses `dist/.vite/manifest.json` and returns the hashed paths for the
    /// main entry point.
    ///
    /// Looks up `"index.html"` in the manifest — the key Vite uses for the
    /// root entry point. For multi-page apps with several entry points, call
    /// this once per entry (e.g. `from_manifest(json, base, "admin/index.html")`).
    fn from_manifest(json: &str, base: &str) -> Self {
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(json) else {
            return Self::default();
        };
        let Some(entries) = manifest.as_object() else {
            return Self::default();
        };

        let Some(entry) = entries.get("index.html") else {
            return Self::default();
        };

        let script = entry
            .get("file")
            .and_then(|f| f.as_str())
            .map(|f| format!("{base}/{f}"))
            .unwrap_or_default();

        let stylesheets = entry
            .get("css")
            .and_then(|c| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|s| s.as_str())
            .map(|s| format!("{base}/{s}"))
            .collect();

        Self { script, stylesheets }
    }
}

// ── Templates ────────────────────────────────────────────────────────────────

#[derive(Template)]
#[template(path = "index.html")]
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
    let entry = EntryAssets::from_config(&config);
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
