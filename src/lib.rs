use axum::{
    Router,
    body::{Body, Bytes},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{any, get},
};
#[cfg(not(debug_assertions))]
use include_dir::File;
#[allow(unused_imports)]
use log::{debug, info, trace, warn};
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Arc;

pub mod frameworks;
use frameworks::Framework;

/// Re-export of the [`include_dir`] crate and its [`Dir`] type.
///
/// Prefer the [`embedded_dir!`] macro over using these directly — it handles
/// the `#[cfg(debug_assertions)]` boilerplate for you.
pub use include_dir;
pub use include_dir::Dir;

/// Embeds a directory at compile time in **release** builds; returns `None` in **debug** builds.
///
/// This is the recommended way to pass the built frontend assets to [`ViteConfig::from_env`].
/// Internally it calls [`include_dir::include_dir!`], so paths must follow include_dir's rules:
/// prefix relative paths with `$CARGO_MANIFEST_DIR` so the proc-macro can resolve them, or use
/// any other `$ENV_VAR` that expands to an absolute path at compile time.
///
/// In **debug** builds the `include_dir!` call is not compiled at all — the macro expands
/// to `None` so you never have to build the frontend just to do `cargo run`.
///
/// In **release** builds the directory is baked into the binary and `Some(&'static Dir)` is
/// returned, matching the signature of [`ViteConfig::from_env`].
///
/// # Example
///
/// ```rust,ignore
/// let config = ViteConfig::from_env(axum_vite::embedded_dir!("$CARGO_MANIFEST_DIR/dist"));
/// ```
#[macro_export]
macro_rules! embedded_dir {
    ($path:tt) => {{
        #[cfg(not(debug_assertions))]
        {
            static DIR: $crate::Dir<'static> = $crate::include_dir::include_dir!($path);
            Some(&DIR)
        }
        #[cfg(debug_assertions)]
        {
            // Explicit type annotation so inference never fails when the result
            // is stored in a variable without a type annotation.
            None::<&$crate::Dir<'static>>
        }
    }};
}

/// Configuration for the Vite dev server proxy and asset serving.
#[derive(Clone, Debug)]
pub struct ViteConfig {
    /// Port of the Vite dev server (default: 5173)
    pub dev_port: u16,
    /// Hostname of the Vite dev server (default: "localhost")
    /// Configurable via `VITE_DEV_HOST` for remote or custom local dev servers.
    pub dev_host: String,
    /// The directory containing the built assets (release mode).
    ///
    /// Embed your dist folder in your application crate and pass the result:
    /// ```rust,ignore
    /// static DIST: axum_vite::Dir<'static> =
    ///     axum_vite::include_dir::include_dir!("$CARGO_MANIFEST_DIR/dist");
    /// # let config = axum_vite::ViteConfig::from_env(Some(&DIST));
    /// ```
    /// Pass `None` in dev mode — the Vite proxy handles asset serving.
    pub dir: Option<&'static Dir<'static>>,
    /// Fallback file for 404s (e.g., "404.html")
    pub not_found: String,
    /// Static prefix used by Vite for assets (e.g., "/static/")
    pub prefix: String,
    /// Path to the frontend project root (where package.json is located)
    pub frontend_root: Option<PathBuf>,
    /// Command to start the Vite dev server (e.g., "npm run dev")
    pub dev_command: String,
    /// Whether to automatically start the Vite dev server on startup
    pub auto_start: bool,
    /// The frontend framework being used for HMR preamble generation
    pub framework: Framework,
    /// Path to the JS/TS entry file relative to the Vite project root.
    ///
    /// Used as the `<script src>` in dev mode, where Vite serves source files
    /// directly. Typical values: `"src/main.tsx"`, `"src/index.ts"`.
    /// Defaults to `"src/main.tsx"`.
    pub dev_script: String,
    /// Key to look up in `dist/.vite/manifest.json` in production builds.
    ///
    /// For single-page apps this is `"index.html"` (the Vite default).
    /// For multi-page apps use [`ViteConfig::entry_assets_for`] to pass a
    /// different key per page while sharing the same config.
    pub manifest_key: String,
    /// Path prefixes to exclude from static asset serving (release mode only).
    /// Paths starting with any of these strings will return a 404 instead of
    /// serving the file. Useful for protecting server-rendered templates that
    /// are embedded in the asset directory but must not be served directly.
    pub excluded_prefixes: Vec<String>,
    /// Shared HTTP client for proxying requests to the Vite dev server.
    /// Created once per `ViteConfig` and reused across requests via `Clone`.
    #[cfg(debug_assertions)]
    pub client: reqwest::Client,
}

impl Default for ViteConfig {
    fn default() -> Self {
        Self {
            dev_port: 5173,
            dev_host: "localhost".to_string(),
            dir: None,
            not_found: "404.html".to_string(),
            prefix: "/static/".to_string(),
            frontend_root: None,
            dev_command: "npm run dev".to_string(),
            auto_start: false,
            framework: Framework::default(),
            dev_script: "src/main.tsx".to_string(),
            manifest_key: "index.html".to_string(),
            excluded_prefixes: Vec::new(),
            #[cfg(debug_assertions)]
            client: reqwest::Client::new(),
        }
    }
}

impl ViteConfig {
    pub fn from_env(dir: Option<&'static Dir<'static>>) -> Self {
        let port = std::env::var("VITE_PORT")
            .unwrap_or_else(|_| "5173".to_string())
            .parse()
            .unwrap_or(5173);

        let prefix = std::env::var("VITE_STATIC_PREFIX").unwrap_or_else(|_| "/static/".to_string());

        let frontend_root = std::env::var("VITE_ROOT").ok().map(PathBuf::from);

        let dev_command =
            std::env::var("VITE_DEV_CMD").unwrap_or_else(|_| "npm run dev".to_string());

        let auto_start = std::env::var("VITE_AUTO_START")
            .map(|v| v == "true" || v == "1")
            .unwrap_or(false);

        let framework = std::env::var("VITE_FRAMEWORK")
            .map(|v| match v.to_lowercase().as_str() {
                "react" => Framework::React,
                "vue" => Framework::Vue,
                "svelte" => Framework::Svelte,
                _ => Framework::None,
            })
            .unwrap_or_default();

        let dev_host = std::env::var("VITE_DEV_HOST").unwrap_or_else(|_| "localhost".to_string());

        Self {
            dev_port: port,
            dev_host,
            dir,
            not_found: "404.html".to_string(),
            prefix,
            frontend_root,
            dev_command,
            auto_start,
            framework,
            dev_script: std::env::var("VITE_DEV_SCRIPT")
                .unwrap_or_else(|_| "src/main.tsx".to_string()),
            manifest_key: std::env::var("VITE_MANIFEST_KEY")
                .unwrap_or_else(|_| "index.html".to_string()),
            excluded_prefixes: Vec::new(),
            #[cfg(debug_assertions)]
            client: reqwest::Client::new(),
        }
    }

    /// Generates the HTML scripts required for Vite HMR and framework-specific preambles.
    ///
    /// Returns an empty string in release builds. In debug builds, produces
    /// the `@vite/client` script tag and any framework-specific preamble.
    pub fn hmr_scripts(&self) -> String {
        if !cfg!(debug_assertions) {
            return String::new();
        }

        let prefix = self.prefix.trim_end_matches('/');

        // Framework preamble MUST come before @vite/client for React Refresh
        let mut scripts = String::new();

        if let Some(preamble) = self.framework.preamble(prefix) {
            scripts.push_str(&preamble);
            scripts.push('\n');
        }

        scripts.push_str(&format!(
            r#"<script type="module" src="{}/{}"></script>"#,
            prefix, "@vite/client"
        ));

        scripts
    }
}

/// Resolved asset paths for the JS entry point of a Vite project.
///
/// In **dev** mode, `script` points at the raw source file (`src/main.tsx`)
/// and `stylesheets` is empty — Vite injects CSS via the JS module at runtime.
///
/// In **production**, both are read from `dist/.vite/manifest.json` and carry
/// the content hash (e.g. `assets/main-A1b2C3.js`).
///
/// Obtain an instance via [`ViteConfig::entry_assets`].
///
/// # Example
///
/// ```rust,no_run
/// use axum_vite::{ViteConfig, embedded_dir, frameworks::Framework};
///
/// let config = ViteConfig {
///     framework: Framework::React,
///     ..ViteConfig::from_env(embedded_dir!("$CARGO_MANIFEST_DIR/frontend/dist"))
/// };
/// let entry = config.entry_assets();
/// // entry.script  → "/static/src/main.tsx"           (dev)
/// //               → "/static/assets/main-A1b2C3.js"  (release)
/// // entry.stylesheets → []                            (dev)
/// //                   → ["/static/assets/index-B2c3.css"] (release)
/// ```
#[derive(Clone, Default, Debug)]
pub struct EntryAssets {
    /// Value for `<script type="module" src="…">`.
    pub script: String,
    /// Values for `<link rel="stylesheet" href="…">` tags.
    pub stylesheets: Vec<String>,
}

impl ViteConfig {
    /// Resolves the JS entry point and CSS paths using [`ViteConfig::manifest_key`]
    /// and [`ViteConfig::dev_script`].
    ///
    /// In dev mode returns `dev_script` prefixed with [`ViteConfig::prefix`] and
    /// an empty `stylesheets` vec — Vite injects CSS via the JS module at runtime.
    /// In release mode reads `dist/.vite/manifest.json` from the embedded dir and
    /// looks up `manifest_key`. If the manifest is missing or the key is not found
    /// a warning is logged and the dev-mode fallback is returned.
    ///
    /// For multi-page apps with multiple entry points use [`entry_assets_for`](Self::entry_assets_for)
    /// to pass a different manifest key while reusing the same config.
    ///
    /// Requires `build: { manifest: true }` in `vite.config` so Vite writes
    /// `dist/.vite/manifest.json` during `npm run build`.
    pub fn entry_assets(&self) -> EntryAssets {
        self.entry_assets_for(&self.manifest_key.clone(), &self.dev_script.clone())
    }

    /// Like [`entry_assets`](Self::entry_assets) but explicitly specifies both the
    /// manifest key (used in production) and the dev script path (used in dev mode).
    ///
    /// Use this for **secondary entry points** in multi-page apps where the manifest
    /// key (an HTML file) differs from the dev-mode source path:
    ///
    /// ```rust,no_run
    /// # use axum_vite::{ViteConfig, embedded_dir};
    /// # let config = ViteConfig::from_env(None);
    /// // Primary entry: convenience wrapper uses ViteConfig fields
    /// let home = config.entry_assets();
    ///
    /// // Secondary entry: supply manifest key + dev source path explicitly
    /// let editor = config.entry_assets_for(
    ///     "templates/components/editor.html",  // manifest key (prod)
    ///     "src/features/PostEditor/editor.tsx", // dev script path
    /// );
    /// ```
    ///
    /// In dev mode `dev_script` is prepended with [`ViteConfig::prefix`] and
    /// returned directly — the manifest key is ignored. In production the
    /// manifest key is looked up in `dist/.vite/manifest.json`.
    #[allow(unused)]
    pub fn entry_assets_for(&self, manifest_key: &str, dev_script: &str) -> EntryAssets {
        let base = self.prefix.trim_end_matches('/');

        #[cfg(not(debug_assertions))]
        if let Some(dir) = self.dir {
            if let Some(file) = dir.get_file(".vite/manifest.json") {
                if let Some(json) = file.contents_utf8() {
                    return EntryAssets::from_manifest(json, base, manifest_key);
                }
            }
            warn!(
                "[axum-vite] entry_assets: dist/.vite/manifest.json not found in embedded dir. \
                 Add `build: {{ manifest: true }}` to vite.config and rebuild the frontend. \
                 Falling back to dev-mode paths — assets will 404 in production."
            );
        }

        #[cfg(not(debug_assertions))]
        let _ = manifest_key;

        // Dev: Vite serves the entry file directly; CSS is injected by the JS module.
        EntryAssets {
            script: format!("{base}/{}", dev_script),
            stylesheets: vec![],
        }
    }
}

impl EntryAssets {
    #[cfg(not(debug_assertions))]
    fn from_manifest(json: &str, base: &str, key: &str) -> Self {
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(json) else {
            warn!("[axum-vite] entry_assets: failed to parse manifest.json as JSON");
            return Self::default();
        };
        let Some(entries) = manifest.as_object() else {
            return Self::default();
        };
        let Some(entry) = entries.get(key) else {
            warn!(
                "[axum-vite] entry_assets: key {:?} not found in manifest.json. \
                 Available keys: {}",
                key,
                entries.keys().cloned().collect::<Vec<_>>().join(", ")
            );
            return Self::default();
        };

        let script = entry
            .get("file")
            .and_then(|f: &serde_json::Value| f.as_str())
            .map(|f| format!("{base}/{f}"))
            .unwrap_or_default();

        let stylesheets = entry
            .get("css")
            .and_then(|c: &serde_json::Value| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|s: &serde_json::Value| s.as_str())
            .map(|s| format!("{base}/{s}"))
            .collect();

        Self {
            script,
            stylesheets,
        }
    }
}

/// A handle to a running Vite dev server child process.
///
/// Dropping this handle **kills the child process**, preventing orphaned Node
/// processes when the Rust dev server restarts (e.g. with `cargo watch`).
/// Keep it alive for the full lifetime of your server:
///
/// ```rust,no_run
/// # use axum_vite::{ViteConfig, spawn_dev_server};
/// let config = ViteConfig::from_env(None);
/// // Bind to a variable so it lives until the end of main.
/// let _dev_server = spawn_dev_server(&config);
/// ```
pub struct DevServerHandle {
    child: Child,
}

impl std::fmt::Debug for DevServerHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DevServerHandle").finish_non_exhaustive()
    }
}

impl Drop for DevServerHandle {
    fn drop(&mut self) {
        let _ = self.child.kill();
        // Reap the process so it doesn't linger as a zombie on Unix.
        let _ = self.child.wait();
    }
}

/// Spawns the Vite dev server as a child process.
///
/// Returns a [`DevServerHandle`] that kills the process on drop.
/// This should only be called in debug mode and when `auto_start` is enabled.
pub fn spawn_dev_server(config: &ViteConfig) -> std::io::Result<DevServerHandle> {
    let root = config.frontend_root.as_ref().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "frontend_root must be set to spawn dev server",
        )
    })?;

    info!(
        "[axum-vite] spawning dev server in {:?}  (`{}`)",
        root, config.dev_command
    );

    #[cfg(unix)]
    {
        // Prefix the command with `exec` so the shell replaces itself with
        // the Node process. This means `kill()` in Drop reaches Node directly
        // with no intermediate shell left as an orphan.
        Command::new("sh")
            .arg("-c")
            .arg(format!("exec {}", config.dev_command))
            .current_dir(root)
            .spawn()
            .map(|child| DevServerHandle { child })
    }
    #[cfg(windows)]
    {
        // `cmd /C` does not forward signals to its child process, so killing
        // the handle may leave the Node process running. If that matters,
        // consider spawning node/npm directly instead of going through cmd,
        // or use `taskkill /T /F /PID <pid>` in the Drop impl.
        Command::new("cmd")
            .arg("/C")
            .arg(&config.dev_command)
            .current_dir(root)
            .spawn()
            .map(|child| DevServerHandle { child })
    }
}

pub fn router<S>(config: ViteConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let config = Arc::new(config);

    Router::new().route(
        "/{*path}",
        any(move |
            method: Method,
            axum::extract::Path(path): axum::extract::Path<String>,
            uri: axum::http::Uri,
            headers: axum::http::HeaderMap,
            body: Bytes,
        | {
            let config = config.clone();
            // Path<String> handles two things we need:
            //   1. Strips the nest prefix (e.g. /static/) automatically.
            //   2. Percent-decodes the segment so include_dir can match
            //      actual filenames (e.g. "my image.png" not "my%20image.png").
            // We re-attach the raw query string separately so Vite receives
            // its cache-busting params (?vue&type=style, ?import, ?t=) intact.
            let full_path = match uri.query() {
                Some(q) => format!("{}?{}", path, q),
                None => path,
            };
            async move { serve_asset(Some(full_path), None, headers, method, body, config).await }
        }),
    )
}

/// Convenience router for a single-page application.
///
/// Equivalent to wiring up `serve_index` on `/` and a catch-all `/{*path}`,
/// nesting the Vite asset `router` under `/{static_prefix}`, and wrapping
/// everything in [`hmr_injection_middleware`]. Use this when your Axum app
/// has no server-side HTML rendering and just needs to serve the SPA shell:
///
/// ```rust,no_run
/// use axum::{Router, routing::get};
/// use axum_vite::{ViteConfig, spa_router};
///
/// let config = ViteConfig::from_env(None);
/// let app = Router::<()>::new()
///     .route("/api/hello", get(|| async { "hello" }))
///     .merge(spa_router(config));
/// ```
///
/// If you render HTML server-side (e.g. with Askama), prefer calling
/// `config.hmr_scripts()` in your template and skipping this helper — see the
/// README for details.
pub fn spa_router<S>(config: ViteConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let static_prefix = format!("/{}", config.prefix.trim_matches('/'));
    let config = Arc::new(config);
    let c1 = config.clone();
    let c2 = config.clone();
    let c3 = config.clone();
    let c_mw = config;

    // In dev mode the catch-all tries to proxy the path to Vite first (handles
    // root-relative asset requests like /src/main.tsx that the browser produces
    // from relative URLs in the HTML), then falls back to index.html for real
    // SPA navigation routes (which Vite would 404).
    //
    // In release mode the catch-all always serves index.html; real assets are
    // handled by the more-specific prefix-mounted router below.
    let mut r = Router::new()
        .route(
            "/",
            get(move || {
                let c = c1.clone();
                async move { _serve_index(c).await }
            }),
        )
        .route(
            "/{*path}",
            any(
                move |method: Method,
                      axum::extract::Path(path): axum::extract::Path<String>,
                      uri: axum::http::Uri,
                      headers: axum::http::HeaderMap,
                      body: Bytes| {
                    let c = c2.clone();
                    // Path<String> decodes percent-encoding and strips the nest
                    // prefix. Re-attach the raw query string so Vite's module
                    // protocol (?vue&type=style, ?import, ?t= HMR busters) works.
                    let full_path = match uri.query() {
                        Some(q) => format!("{}?{}", path, q),
                        None => path,
                    };
                    async move { _serve_spa_catchall(full_path, headers, method, body, c).await }
                },
            ),
        );

    // Only nest the static asset router when there is a real prefix.
    // If the prefix is just "/" there is no distinct mount point and the
    // catch-all handles everything.
    if static_prefix != "/" {
        r = r.nest(&static_prefix, router((*c3).clone()));
    }

    r.layer(axum::middleware::from_fn_with_state(
        c_mw,
        hmr_injection_middleware,
    ))
}

/// Serves the SPA shell (`index.html`).
///
/// Use this as the handler for your root route and any catch-all route so the
/// single-page app can handle client-side navigation:
///
/// ```rust,no_run
/// use axum::{Router, routing::get};
/// use axum_vite::{ViteConfig, serve_index};
///
/// let config = ViteConfig::from_env(None);
/// let app = Router::<()>::new()
///     .route("/", get({
///         let c = config.clone();
///         move || serve_index(c.clone())
///     }))
///     .nest("/static", axum_vite::router(config));
/// ```
///
/// In **dev** builds this proxies `GET /` to the Vite dev server so that HMR
/// and the React/Vue/Svelte preamble are injected by Vite itself.
///
/// In **release** builds this reads `index.html` from the embedded asset
/// directory and serves it with `Cache-Control: no-store`.
pub async fn serve_index(config: ViteConfig) -> impl IntoResponse {
    _serve_index(Arc::new(config)).await
}

#[cfg(debug_assertions)]
async fn _serve_index(config: Arc<ViteConfig>) -> Response {
    let headers = axum::http::HeaderMap::new();
    // The root is always fetched with GET by the browser.
    proxy_to_vite("", &config, &headers, &Method::GET, Bytes::new()).await
}

/// SPA catch-all: tries to proxy the path to Vite first.
/// On Vite 404 (client-side navigation route) falls back to `index.html`.
/// In release builds always returns `index.html` — assets are handled by
/// the prefix-mounted `router()` which is more specific and wins first.
#[cfg(debug_assertions)]
async fn _serve_spa_catchall(
    path: String,
    headers: axum::http::HeaderMap,
    method: Method,
    body: Bytes,
    config: Arc<ViteConfig>,
) -> Response {
    let response = proxy_raw(&path, &config, &headers, &method, body).await;
    // Only fall back to the SPA shell for browser page-navigation requests.
    // API fetches, missing images, and non-GET requests must receive the real
    // status code — not index.html — or `fetch()` callers get back HTML and
    // crash with "Unexpected token '<'" when they try to parse it as JSON.
    let is_html_nav = method == Method::GET
        && headers
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.contains("text/html"));
    if response.status() == StatusCode::NOT_FOUND && is_html_nav {
        _serve_index(config).await
    } else {
        response
    }
}

#[cfg(not(debug_assertions))]
async fn _serve_spa_catchall(
    path: String,
    headers: axum::http::HeaderMap,
    method: Method,
    _body: Bytes,
    config: Arc<ViteConfig>,
) -> Response {
    let clean = path.trim_start_matches('/');
    let file_key = clean.split_once('?').map_or(clean, |(p, _)| p);

    // Respect excluded_prefixes consistently with serve_asset so a catchall
    // request cannot bypass a configured exclusion.
    if config
        .excluded_prefixes
        .iter()
        .any(|p| file_key.starts_with(p.as_str()))
    {
        return StatusCode::NOT_FOUND.into_response();
    }

    if let Some(dir) = config.dir {
        if let Some(file) = dir.get_file(file_key) {
            return serve_embedded_file(file, None);
        }
    }

    // Only fall back to the SPA shell for browser page-navigation requests.
    // API fetches, missing images, and non-GET requests must receive 404 —
    // not index.html — or `fetch()` callers get back HTML and crash with
    // "Unexpected token '<'" when they try to parse it as JSON.
    let is_html_nav = method == Method::GET
        && headers
            .get(axum::http::header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .is_some_and(|s| s.contains("text/html"));
    if is_html_nav {
        _serve_index(config).await
    } else {
        StatusCode::NOT_FOUND.into_response()
    }
}

#[cfg(not(debug_assertions))]
async fn _serve_index(config: Arc<ViteConfig>) -> Response {
    let dir = match config.dir {
        Some(d) => d,
        None => {
            warn!(
                "[axum-vite] serve_index: no embedded dir configured — pass the include_dir! output to ViteConfig::from_env"
            );
            return StatusCode::NOT_FOUND.into_response();
        }
    };
    match dir.get_file("index.html") {
        Some(file) => Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CACHE_CONTROL, "no-store")
            .body(Body::from(file.contents()))
            .unwrap(),
        None => {
            warn!("[axum-vite] serve_index: index.html not found in embedded dir");
            StatusCode::NOT_FOUND.into_response()
        }
    }
}

/// Core proxy logic shared by the `/static/` router and the dev fallback.
///
/// Always prepends the Vite `base` prefix from `config.prefix` — this is the
/// single source of truth for the asset path namespace. When `base` is `/`
/// (no prefix) this effectively becomes a no-op; when it's `/static/` the
/// prefix is added automatically.
#[cfg(debug_assertions)]
async fn do_proxy(
    url: &str,
    log_path: &str,
    config: &ViteConfig,
    headers: &axum::http::HeaderMap,
    method: &Method,
    body: Bytes,
) -> Response {
    trace!("[axum-vite] → /{}", log_path);

    let mut request_builder = config.client.request(method.clone(), url);
    for (name, value) in headers.iter() {
        // Strip Host (we target a known localhost URL) and Accept-Encoding:
        // reqwest decompresses transparently but still forwards the upstream
        // Content-Encoding header, causing ERR_CONTENT_DECODING_FAILED in the
        // browser. Forcing an uncompressed response is fine on localhost.
        if name != axum::http::header::HOST && name != axum::http::header::ACCEPT_ENCODING {
            request_builder = request_builder.header(name, value);
        }
    }

    match request_builder.body(body).send().await {
        Ok(resp) => {
            let mut builder = Response::builder().status(resp.status());
            for (name, value) in resp.headers().iter() {
                // Drop hop-by-hop and size headers: we buffer the full body
                // into memory, so hyper will compute the correct Content-Length
                // from the actual Bytes. Keeping the original could cause a
                // mismatch (e.g. chunked response with an incorrect length, or
                // a body that was decompressed in transit).
                if name != header::TRANSFER_ENCODING && name != header::CONTENT_LENGTH {
                    builder = builder.header(name, value);
                }
            }
            builder
                .body(Body::from(resp.bytes().await.unwrap_or_default()))
                .unwrap()
        }
        Err(_) => {
            warn!("[axum-vite] dev server unreachable at {}", url);
            Response::builder()
                .status(StatusCode::SERVICE_UNAVAILABLE)
                .body(Body::from("Vite dev server unreachable"))
                .unwrap()
        }
    }
}

/// Proxies `path_str` to Vite under the configured static prefix.
/// Used by the asset `router()` where Axum has already stripped the prefix
/// from the path segment.
#[cfg(debug_assertions)]
async fn proxy_to_vite(
    path_str: &str,
    config: &ViteConfig,
    headers: &axum::http::HeaderMap,
    method: &Method,
    body: Bytes,
) -> Response {
    let prefix = config.prefix.trim_matches('/');
    // Do not strip "public/" here. Vite serves files from the `public/`
    // folder at the dist root (e.g. `public/logo.png` → `/logo.png`), so the
    // developer must reference them without the `public/` prefix in both dev
    // and release. Silently stripping it in dev mode only would create a
    // dev/release inconsistency where broken URLs work locally but 404 in
    // production.
    let clean = path_str.trim_start_matches('/');
    let url = if prefix.is_empty() {
        format!("http://{}:{}/{}", config.dev_host, config.dev_port, clean)
    } else {
        format!(
            "http://{}:{}/{}/{}",
            config.dev_host, config.dev_port, prefix, clean
        )
    };
    do_proxy(&url, path_str, config, headers, method, body).await
}

/// Proxies `path_str` to Vite at the root — NO prefix added.
/// Used for paths that browsers request relative to `/` (e.g. `/src/main.tsx`,
/// `/@vite/client`) which Vite serves at its own root regardless of the
/// static asset prefix configured in this crate.
#[cfg(debug_assertions)]
async fn proxy_raw(
    path_str: &str,
    config: &ViteConfig,
    headers: &axum::http::HeaderMap,
    method: &Method,
    body: Bytes,
) -> Response {
    let clean = path_str.trim_start_matches('/');
    let url = format!("http://{}:{}/{}", config.dev_host, config.dev_port, clean);
    do_proxy(&url, path_str, config, headers, method, body).await
}

#[cfg(debug_assertions)]
pub async fn serve_asset(
    path: Option<String>,
    _mime_type: Option<&str>,
    headers: axum::http::HeaderMap,
    method: Method,
    body: Bytes,
    config: Arc<ViteConfig>,
) -> impl IntoResponse {
    match path {
        Some(path_str) => proxy_to_vite(&path_str, &config, &headers, &method, body)
            .await
            .into_response(),
        None => (StatusCode::NOT_FOUND, "Not Found").into_response(),
    }
}

/// Middleware that injects Vite HMR scripts into HTML responses in dev mode.
///
/// Registered automatically by [`spa_router`] via `from_fn_with_state`.
/// The middleware receives the `ViteConfig` it was built with, so custom
/// `prefix` and `framework` settings are always respected.
///
/// Scans `text/html` responses and injects the preamble + `@vite/client`
/// script immediately before `</head>`. In release builds this is a no-op.
///
/// **Note:** the entire HTML body is buffered in memory for the injection.
/// This is only active in debug builds and is fine for typical HTML pages;
/// do not route large non-HTML downloads through an HTML route in dev mode.
#[cfg(debug_assertions)]
pub async fn hmr_injection_middleware(
    axum::extract::State(config): axum::extract::State<Arc<ViteConfig>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let response = next.run(request).await;
    if let Some(content_type) = response.headers().get(header::CONTENT_TYPE)
        && content_type.to_str().unwrap_or("").contains("text/html")
    {
        let hmr_scripts = config.hmr_scripts();
        if hmr_scripts.is_empty() {
            return response;
        }

        let (parts, body) = response.into_parts();
        let bytes = match axum::body::to_bytes(body, usize::MAX).await {
            Ok(b) => b,
            Err(_) => return Response::from_parts(parts, Body::empty()),
        };

        if let Ok(mut html) = String::from_utf8(bytes.to_vec()) {
            // Vite already injected its own preamble (e.g. when proxying /),
            // so skip injection to avoid duplicating the scripts.
            if html.contains("@vite/client") {
                return Response::from_parts(parts, Body::from(html));
            }
            if let Some(pos) = html.find("</head>") {
                html.insert_str(pos, &format!("\n{}\n", hmr_scripts));
            } else if let Some(pos) = html.find("</body>") {
                html.insert_str(pos, &format!("\n{}\n", hmr_scripts));
            } else {
                html.push_str(&format!("\n{}\n", hmr_scripts));
            }
            let mut res = Response::from_parts(parts, Body::from(html.clone()));
            res.headers_mut().insert(
                header::CONTENT_LENGTH,
                axum::http::HeaderValue::from(html.len()),
            );
            return res;
        } else {
            return Response::from_parts(parts, Body::from(bytes));
        }
    }
    response
}

/// Builds a response for a single embedded file with correct MIME type and
/// cache headers. Used by both `serve_asset` and `_serve_spa_catchall`.
#[cfg(not(debug_assertions))]
fn serve_embedded_file(file: &'static File<'static>, mime_type: Option<&str>) -> Response {
    let path_buf = PathBuf::from(file.path());
    let resolved_mime = match mime_type {
        Some(m) => m.to_string(),
        None => mime_guess::from_path(&path_buf)
            .first_or_octet_stream()
            .to_string(),
    };
    // Cache-Control strategy:
    //   - HTML: no-store (entry point, always revalidate)
    //   - Service workers: no-store (stale SW can strand users for the cache TTL)
    //   - Web manifests: 1 day (not content-hashed by Vite, changes infrequently)
    //   - Everything else: 1 year (Vite content-hashes all JS/CSS filenames)
    let cache_header = if resolved_mime.contains("text/html") {
        "no-store"
    } else if path_buf
        .file_name()
        .and_then(|n| n.to_str())
        .is_some_and(|n| matches!(n, "sw.js" | "service-worker.js" | "service-worker.ts"))
    {
        "no-store"
    } else if resolved_mime.contains("manifest")
        || path_buf
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e == "webmanifest")
    {
        "public, max-age=86400"
    } else {
        // Vite content-hashes filenames only for files inside the `assets/`
        // directory (the output of Rollup bundling). Everything else — favicon,
        // robots.txt, translation JSON, images from `public/` — lands in the
        // dist root without a hash and must be revalidated on every request.
        if path_buf.components().any(|c| c.as_os_str() == "assets") {
            "public, max-age=31536000, immutable"
        } else {
            "public, no-cache"
        }
    };
    Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, resolved_mime)
        .header(header::CACHE_CONTROL, cache_header)
        // file.contents() is &'static [u8] because include_dir embeds the
        // data directly in the binary. Body::from(&'static [u8]) is zero-copy.
        .body(Body::from(file.contents()))
        .unwrap()
}

/// Release-mode stub — passes the request through unmodified.
#[cfg(not(debug_assertions))]
pub async fn hmr_injection_middleware(
    axum::extract::State(_config): axum::extract::State<Arc<ViteConfig>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    next.run(request).await
}

#[cfg(not(debug_assertions))]
pub async fn serve_asset(
    path: Option<String>,
    mime_type: Option<&str>,
    _headers: axum::http::HeaderMap,
    _method: Method,
    _body: Bytes,
    config: Arc<ViteConfig>,
) -> impl IntoResponse {
    let serve_not_found = || {
        if let Some(dir) = config.dir {
            if let Some(f) = dir.get_file(&config.not_found) {
                let mut res = serve_embedded_file(f, Some("text/html; charset=utf-8"));
                *res.status_mut() = StatusCode::NOT_FOUND;
                return res.into_response();
            }
        }
        StatusCode::NOT_FOUND.into_response()
    };

    match path {
        Some(path_str) => {
            // Trim the leading slash and query before *all* checks so that a
            // request to "/templates/secret.html" cannot bypass an
            // `excluded_prefixes = ["templates/"]` rule.
            let clean = path_str.trim_start_matches('/');
            // Strip query string — Vite appends cache-busting params
            // (?v=, ?import, ?t=) that are not part of the embedded file
            // path stored by include_dir.
            let file_key = clean.split_once('?').map_or(clean, |(p, _)| p);

            if config
                .excluded_prefixes
                .iter()
                .any(|p| file_key.starts_with(p.as_str()))
            {
                return serve_not_found();
            }

            if let Some(dir) = config.dir {
                if let Some(file) = dir.get_file(file_key) {
                    debug!("[axum-vite] serving /{}", clean);
                    return serve_embedded_file(file, mime_type).into_response();
                }
            }
            warn!("[axum-vite] 404 /{}", clean);
            serve_not_found()
        }
        None => serve_not_found(),
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
#[allow(clippy::useless_vec)]
mod tests {
    use super::*;
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt; // for `oneshot`

    // -----------------------------------------------------------------------
    // ViteConfig helpers
    // -----------------------------------------------------------------------

    #[test]
    fn default_config_values() {
        let config = ViteConfig::default();
        assert_eq!(config.dev_port, 5173);
        assert_eq!(config.dev_host, "localhost");
        assert_eq!(config.prefix, "/static/");
        assert_eq!(config.not_found, "404.html");
        assert!(!config.auto_start);
        assert!(config.excluded_prefixes.is_empty());
        assert!(config.dir.is_none());
    }

    #[test]
    fn hmr_scripts_empty_in_release() {
        let config = ViteConfig {
            framework: frameworks::Framework::None,
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let scripts = config.hmr_scripts();
        if cfg!(debug_assertions) {
            assert!(scripts.contains("@vite/client"), "missing @vite/client");
            assert!(
                !scripts.contains("@react-refresh"),
                "unexpected react preamble"
            );
        } else {
            assert!(scripts.is_empty(), "expected empty in release");
        }
    }

    #[test]
    fn hmr_scripts_react_preamble_in_debug() {
        if !cfg!(debug_assertions) {
            return;
        }
        let config = ViteConfig {
            framework: frameworks::Framework::React,
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let scripts = config.hmr_scripts();
        assert!(scripts.contains("@react-refresh"), "missing react preamble");
        assert!(
            scripts.contains("injectIntoGlobalHook"),
            "missing injectIntoGlobalHook"
        );
        let preamble_pos = scripts.find("@react-refresh").unwrap();
        let client_pos = scripts.find("@vite/client").unwrap();
        assert!(
            preamble_pos < client_pos,
            "preamble must precede @vite/client"
        );
    }

    #[test]
    fn hmr_scripts_prefix_interpolated() {
        if !cfg!(debug_assertions) {
            return;
        }
        let config = ViteConfig {
            framework: frameworks::Framework::React,
            prefix: "/assets/".to_string(),
            ..Default::default()
        };
        let scripts = config.hmr_scripts();
        assert!(
            scripts.contains("/assets/@react-refresh"),
            "prefix not interpolated in preamble"
        );
        assert!(
            scripts.contains("/assets/@vite/client"),
            "prefix not interpolated in @vite/client"
        );
    }

    // -----------------------------------------------------------------------
    // excluded_prefixes
    // -----------------------------------------------------------------------

    #[test]
    fn excluded_prefixes_default_empty() {
        assert!(ViteConfig::default().excluded_prefixes.is_empty());
    }

    #[test]
    fn excluded_prefixes_match_correctly() {
        let excluded = vec!["templates/".to_string(), "index.html".to_string()];
        let is_excluded = |path: &str| excluded.iter().any(|p| path.starts_with(p.as_str()));
        assert!(is_excluded("templates/base.html"));
        assert!(is_excluded("index.html"));
        assert!(!is_excluded("assets/main.js"));
        assert!(!is_excluded("favicon.ico"));
    }

    // -----------------------------------------------------------------------
    // spawn_dev_server: returns an error when frontend_root is not set
    // -----------------------------------------------------------------------

    #[test]
    fn spawn_dev_server_errors_without_root() {
        let config = ViteConfig::default(); // frontend_root: None
        let err = spawn_dev_server(&config).expect_err("expected error when root is None");
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    // -----------------------------------------------------------------------
    // router — proxy path (dev only, no real Vite needed: port 1 → refused)
    // -----------------------------------------------------------------------

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn router_returns_unavailable_when_vite_not_running() {
        let config = ViteConfig {
            dev_port: 1,
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let app: Router = Router::new().nest("/static", router(config));
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/static/main.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // router() with no prefix — used standalone, no nest(). The path reaches
    // Vite at the root so we still expect SERVICE_UNAVAILABLE (port 1), not 404.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn router_no_prefix_returns_unavailable() {
        let config = ViteConfig {
            dev_port: 1,
            prefix: "/".to_string(),
            ..Default::default()
        };
        let app: Router = router(config);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/main.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // spa_router catchall proxies unknown paths to Vite and passes the
    // response through unmodified — it must NOT silently replace it with
    // index.html when the Vite response is not a 404 (here: 503 because
    // Vite is unreachable on port 1).
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn spa_router_unknown_path_passes_through_vite_response() {
        let config = ViteConfig {
            dev_port: 1,
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let app: Router = spa_router(config);
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/some/spa/page")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        // Vite is unreachable → SERVICE_UNAVAILABLE, not 200 with index.html.
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // The API-404-swallow guard: a POST to an unknown path must pass through
    // Vite's response rather than falling back to index.html. In dev mode with
    // Vite unreachable the proxy returns SERVICE_UNAVAILABLE; the important
    // thing is the catchall does not short-circuit to 200/index.html.
    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn spa_router_post_to_unknown_path_is_not_swallowed() {
        let config = ViteConfig {
            dev_port: 1,
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let app: Router = spa_router(config);
        let response = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/missing")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_ne!(
            response.status(),
            StatusCode::OK,
            "POST to unknown path must not return 200 (index.html swallow)"
        );
    }

    // -----------------------------------------------------------------------
    // serve_index (dev only)
    // -----------------------------------------------------------------------

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn serve_index_unavailable_when_vite_not_running() {
        let config = ViteConfig {
            dev_port: 1,
            ..Default::default()
        };
        let response = serve_index(config).await.into_response();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn serve_index_route_registered() {
        let config = ViteConfig {
            dev_port: 1,
            ..Default::default()
        };
        let app: Router = Router::new().route(
            "/",
            get({
                let c = config.clone();
                move || serve_index(c.clone())
            }),
        );
        let response = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }

    // -----------------------------------------------------------------------
    // HMR middleware — tested through a real Axum router so the middleware
    // signature (State + Next) is exercised correctly.
    // -----------------------------------------------------------------------

    #[cfg(debug_assertions)]
    fn make_hmr_app(framework: frameworks::Framework, prefix: &str) -> Router {
        let config = Arc::new(ViteConfig {
            framework,
            prefix: prefix.to_string(),
            ..Default::default()
        });
        // A simple handler that returns a plain HTML page.
        let html_handler = || async {
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                .body(Body::from(
                    "<html><head><title>T</title></head><body></body></html>",
                ))
                .unwrap()
        };
        let js_handler = || async {
            axum::response::Response::builder()
                .status(StatusCode::OK)
                .header(axum::http::header::CONTENT_TYPE, "application/javascript")
                .body(Body::from("console.log('hi')"))
                .unwrap()
        };
        Router::new()
            .route("/page", get(html_handler))
            .route("/app.js", get(js_handler))
            .layer(axum::middleware::from_fn_with_state(
                config,
                hmr_injection_middleware,
            ))
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn hmr_middleware_injects_before_head_close() {
        let app = make_hmm_app(frameworks::Framework::React, "/static/");
        let response = app
            .oneshot(Request::builder().uri("/page").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        let head_pos = html.find("</head>").expect("missing </head>");
        let client_pos = html
            .find("@vite/client")
            .expect("@vite/client not injected");
        assert!(
            client_pos < head_pos,
            "@vite/client should be before </head>"
        );
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn hmr_middleware_skips_already_injected_html() {
        // Build a router whose HTML already contains @vite/client.
        let config = Arc::new(ViteConfig {
            framework: frameworks::Framework::React,
            ..Default::default()
        });
        let html_with_client = r#"<html><head><script type="module" src="/@vite/client"></script></head><body></body></html>"#;
        let handler = move || {
            let h = html_with_client;
            async move {
                axum::response::Response::builder()
                    .status(StatusCode::OK)
                    .header(axum::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                    .body(Body::from(h))
                    .unwrap()
            }
        };
        let app =
            Router::new()
                .route("/page", get(handler))
                .layer(axum::middleware::from_fn_with_state(
                    config,
                    hmr_injection_middleware,
                ));
        let response = app
            .oneshot(Request::builder().uri("/page").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert_eq!(
            html.matches("@vite/client").count(),
            1,
            "should not double-inject"
        );
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn hmr_middleware_leaves_non_html_untouched() {
        let app = make_hmm_app(frameworks::Framework::None, "/static/");
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/app.js")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(body.as_ref(), b"console.log('hi')");
    }

    #[cfg(debug_assertions)]
    #[tokio::test]
    async fn hmr_middleware_respects_custom_prefix_and_framework() {
        // Verify the middleware uses the config it was built with, not a fresh env parse.
        let app = make_hmm_app(frameworks::Framework::React, "/assets/");
        let response = app
            .oneshot(Request::builder().uri("/page").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        let html = String::from_utf8(body.to_vec()).unwrap();
        assert!(html.contains("/assets/@vite/client"), "wrong prefix used");
        assert!(
            html.contains("/assets/@react-refresh"),
            "wrong framework/prefix"
        );
    }

    // Helper — typo-fix alias used in tests above.
    #[cfg(debug_assertions)]
    fn make_hmm_app(framework: frameworks::Framework, prefix: &str) -> Router {
        make_hmr_app(framework, prefix)
    }

    // -----------------------------------------------------------------------
    // embedded_dir! macro
    // -----------------------------------------------------------------------

    #[test]
    fn embedded_dir_returns_none_in_debug_mode() {
        // In debug builds the include_dir! branch is not compiled at all, so the
        // path doesn't need to exist on disk.  The macro must unconditionally
        // produce None.
        #[cfg(debug_assertions)]
        {
            let result: Option<&'static Dir<'static>> =
                embedded_dir!("$CARGO_MANIFEST_DIR/nonexistent/path");
            assert!(
                result.is_none(),
                "embedded_dir! must return None in debug builds"
            );
        }
        // Release-mode coverage is provided by the examples/basic.rs build which
        // actually embeds `examples/frontend/dist` and must compile + serve files.
        #[cfg(not(debug_assertions))]
        {}
    }

    #[test]
    fn embedded_dir_type_is_option_dir() {
        // Confirm the macro produces the correct type so ViteConfig::from_env
        // can accept it without an explicit cast.
        #[cfg(debug_assertions)]
        {
            let result: Option<&'static Dir<'static>> = embedded_dir!("$CARGO_MANIFEST_DIR");
            assert!(result.is_none());
        }
    }

    // -----------------------------------------------------------------------
    // EntryAssets / entry_assets()
    // -----------------------------------------------------------------------

    // In debug builds entry_assets must always return the dev-mode fallback
    // using the config's dev_script (manifest is never read in dev).
    #[cfg(debug_assertions)]
    #[test]
    fn entry_assets_dev_returns_dev_script() {
        let config = ViteConfig {
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let entry = config.entry_assets();
        assert_eq!(entry.script, "/static/src/main.tsx"); // default dev_script
        assert!(
            entry.stylesheets.is_empty(),
            "dev mode must not return stylesheets"
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn entry_assets_dev_respects_custom_dev_script() {
        let config = ViteConfig {
            prefix: "/assets/".to_string(),
            dev_script: "src/index.ts".to_string(),
            ..Default::default()
        };
        let entry = config.entry_assets();
        assert_eq!(entry.script, "/assets/src/index.ts");
    }

    // entry_assets_for must use the caller-supplied dev_script, not self.dev_script.
    // This is the key invariant for secondary MPA entries (e.g. editor.tsx).
    #[cfg(debug_assertions)]
    #[test]
    fn entry_assets_for_dev_uses_explicit_dev_script() {
        let config = ViteConfig {
            prefix: "/static/".to_string(),
            dev_script: "src/main.tsx".to_string(), // primary entry — must not bleed through
            ..Default::default()
        };
        let entry = config.entry_assets_for(
            "templates/components/editor.html", // manifest key (ignored in dev)
            "src/features/PostEditor/editor.tsx",
        );
        assert_eq!(entry.script, "/static/src/features/PostEditor/editor.tsx");
        assert!(entry.stylesheets.is_empty());
    }

    // entry_assets() is still sugar for entry_assets_for(manifest_key, dev_script).
    #[cfg(debug_assertions)]
    #[test]
    fn entry_assets_delegates_to_entry_assets_for() {
        let config = ViteConfig {
            prefix: "/static/".to_string(),
            dev_script: "src/app.ts".to_string(),
            manifest_key: "index.html".to_string(),
            ..Default::default()
        };
        let via_shortcut = config.entry_assets();
        let via_explicit =
            config.entry_assets_for(&config.manifest_key.clone(), &config.dev_script.clone());
        assert_eq!(via_shortcut.script, via_explicit.script);
        assert_eq!(via_shortcut.stylesheets, via_explicit.stylesheets);
    }

    #[cfg(debug_assertions)]
    #[test]
    fn entry_assets_dev_trims_trailing_slash_in_prefix() {
        // prefix is stored as "/static/" — the trailing slash must not produce
        // a double-slash like "/static//src/main.tsx".
        let config = ViteConfig {
            prefix: "/static/".to_string(),
            ..Default::default()
        };
        let entry = config.entry_assets();
        assert!(
            !entry.script.contains("//"),
            "double-slash in script path: {}",
            entry.script
        );
    }

    // from_manifest is only compiled in release — test it directly via a helper
    // that mirrors the same logic so we can exercise it in debug test runs too.
    #[test]
    fn entry_assets_from_manifest_secondary_entry_no_css() {
        // Real-world pattern: an HTML entry that only produces a JS chunk (no CSS).
        let json = r#"{
            "index.html": {
                "file": "assets/main-A1b2C3.js",
                "css": ["assets/index-B2c3D4.css"]
            },
            "templates/components/editor.html": {
                "file": "assets/templates/components/editor-oW_rDizN.js"
            }
        }"#;
        let entry =
            parse_manifest_for_test(json, "/static", "templates/components/editor.html");
        assert_eq!(
            entry.script,
            "/static/assets/templates/components/editor-oW_rDizN.js"
        );
        assert!(
            entry.stylesheets.is_empty(),
            "secondary entry has no CSS chunk"
        );
    }

    #[test]
    fn entry_assets_from_manifest_happy_path() {
        let json = r#"{
            "index.html": {
                "file": "assets/main-A1b2C3.js",
                "css": ["assets/index-B2c3D4.css"]
            }
        }"#;
        let entry = parse_manifest_for_test(json, "/static", "index.html");
        assert_eq!(entry.script, "/static/assets/main-A1b2C3.js");
        assert_eq!(entry.stylesheets, vec!["/static/assets/index-B2c3D4.css"]);
    }

    #[test]
    fn entry_assets_from_manifest_multiple_css() {
        let json = r#"{
            "index.html": {
                "file": "assets/main.js",
                "css": ["assets/a.css", "assets/b.css"]
            }
        }"#;
        let entry = parse_manifest_for_test(json, "/s", "index.html");
        assert_eq!(entry.stylesheets.len(), 2);
        assert_eq!(entry.stylesheets[0], "/s/assets/a.css");
        assert_eq!(entry.stylesheets[1], "/s/assets/b.css");
    }

    #[test]
    fn entry_assets_from_manifest_no_css_key() {
        // Vite omits "css" when there are no stylesheets for the entry.
        let json = r#"{"index.html": {"file": "assets/main.js"}}"#;
        let entry = parse_manifest_for_test(json, "/static", "index.html");
        assert_eq!(entry.script, "/static/assets/main.js");
        assert!(entry.stylesheets.is_empty());
    }

    #[test]
    fn entry_assets_from_manifest_key_not_found_returns_default() {
        let json = r#"{"index.html": {"file": "assets/main.js"}}"#;
        let entry = parse_manifest_for_test(json, "/static", "admin/index.html");
        assert!(
            entry.script.is_empty(),
            "expected empty script on missing key"
        );
        assert!(entry.stylesheets.is_empty());
    }

    #[test]
    fn entry_assets_from_manifest_invalid_json_returns_default() {
        let entry = parse_manifest_for_test("not json at all {{{", "/static", "index.html");
        assert!(entry.script.is_empty());
        assert!(entry.stylesheets.is_empty());
    }

    #[test]
    fn entry_assets_from_manifest_prefix_no_trailing_slash() {
        // Verify the helper strips trailing slash from base the same way
        // entry_assets() does.
        let json = r#"{"index.html": {"file": "assets/main.js", "css": ["assets/a.css"]}}"#;
        let entry = parse_manifest_for_test(json, "/static/", "index.html");
        assert!(
            !entry.script.contains("//"),
            "double-slash in script: {}",
            entry.script
        );
        assert!(
            !entry.stylesheets[0].contains("//"),
            "double-slash in css: {}",
            entry.stylesheets[0]
        );
    }

    /// Mirror of `EntryAssets::from_manifest` that is always compiled (not
    /// gated on `#[cfg(not(debug_assertions))]`) so the manifest-parsing logic
    /// can be unit-tested in both debug and release runs.
    fn parse_manifest_for_test(json: &str, base: &str, key: &str) -> EntryAssets {
        let base = base.trim_end_matches('/');
        let Ok(manifest) = serde_json::from_str::<serde_json::Value>(json) else {
            return EntryAssets::default();
        };
        let Some(entries) = manifest.as_object() else {
            return EntryAssets::default();
        };
        let Some(entry) = entries.get(key) else {
            return EntryAssets::default();
        };
        let script = entry
            .get("file")
            .and_then(|f: &serde_json::Value| f.as_str())
            .map(|f| format!("{base}/{f}"))
            .unwrap_or_default();
        let stylesheets = entry
            .get("css")
            .and_then(|c: &serde_json::Value| c.as_array())
            .into_iter()
            .flatten()
            .filter_map(|s: &serde_json::Value| s.as_str())
            .map(|s| format!("{base}/{s}"))
            .collect();
        EntryAssets {
            script,
            stylesheets,
        }
    }
}
