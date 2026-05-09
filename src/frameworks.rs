/// Supported frontend frameworks for HMR preamble generation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Framework {
    #[default]
    None,
    React,
    Vue,
    Svelte,
}

impl Framework {
    /// Returns the specific HMR preamble script for the framework.
    ///
    /// `prefix` should be the Vite base path without a trailing slash
    /// (e.g. `"/static"`). This is used to build the correct import path for
    /// framework-specific virtual modules such as `/@react-refresh`.
    pub fn preamble(&self, prefix: &str) -> Option<String> {
        match self {
            Framework::React => Some(format!(
                r#"<script type="module">
    import {{ injectIntoGlobalHook }} from "{prefix}/@react-refresh";
    injectIntoGlobalHook(window);
    window.$RefreshReg$ = () => {{}};
    window.$RefreshSig$ = () => (type) => type;
</script>"#
            )),
            Framework::Vue => Some(format!(
                r#"<script type="module">
    import {{ createHotContext }} from '{prefix}/@vite/client';
    // Vue HMR is largely handled by @vite/client, but specific
    // preambles can be added here if needed for custom setups.
</script>"#
            )),
            Framework::Svelte => Some(
                r#"<script type="module">
    // Svelte HMR is primarily handled by the Vite Svelte plugin
    // and the @vite/client script.
</script>"#
                    .to_string(),
            ),
            Framework::None => None,
        }
    }

    /// Returns the Vite path markers specific to this framework.
    ///
    /// These are virtual module paths that the Vite dev server uses for HMR.
    /// They need to be recognised by the path resolver so that relative
    /// references from any page depth can be proxied correctly.
    pub fn path_markers(&self) -> &'static [&'static str] {
        match self {
            Framework::React => &["@react-refresh"],
            Framework::Vue => &["@vue/"],
            Framework::Svelte => &["@svelte/"],
            Framework::None => &[],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn react_has_refresh_marker() {
        let markers = Framework::React.path_markers();
        assert!(markers.contains(&"@react-refresh"));
    }

    #[test]
    fn vue_has_vue_marker() {
        let markers = Framework::Vue.path_markers();
        assert!(markers.contains(&"@vue/"));
    }

    #[test]
    fn none_has_no_markers() {
        assert!(Framework::None.path_markers().is_empty());
    }

    #[test]
    fn react_preamble_contains_refresh() {
        let preamble = Framework::React.preamble("/static").unwrap();
        assert!(preamble.contains("/@react-refresh"));
        assert!(preamble.contains("injectIntoGlobalHook"));
    }

    #[test]
    fn vue_preamble_contains_vite_client() {
        let preamble = Framework::Vue.preamble("/static").unwrap();
        assert!(preamble.contains("/@vite/client"));
    }

    #[test]
    fn none_preamble_is_none() {
        assert!(Framework::None.preamble("/static").is_none());
    }
}
