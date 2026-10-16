//! Runtime-only exclusions for plugins that require the official Desktop host.

use codex_plugin::PluginId;

/// Whether the fork excludes this official Desktop-hosted plugin at runtime.
pub fn replaces_official_automation(plugin_key: &str) -> bool {
    let Ok(id) = PluginId::parse(plugin_key) else {
        return false;
    };
    matches!(
        id.marketplace_name.as_str(),
        "openai-bundled"
            | "openai-bundled-alpha"
            | "openai-curated"
            | "openai-api-curated"
            | "openai-curated-remote"
            | "openai-primary-runtime"
    ) && matches!(
        id.plugin_name.as_str(),
        "chrome"
            | "browser"
            | "browser-use"
            | "computer-use"
            | "unified-computer-use"
            | "codex-app-tools"
    )
}

#[cfg(test)]
mod tests {
    use super::replaces_official_automation;

    #[test]
    fn fork_automation_policy_is_limited_to_official_plugins() {
        for name in [
            "chrome",
            "browser",
            "browser-use",
            "computer-use",
            "unified-computer-use",
            "codex-app-tools",
        ] {
            for marketplace in [
                "openai-bundled",
                "openai-bundled-alpha",
                "openai-curated-remote",
            ] {
                assert!(replaces_official_automation(&format!(
                    "{name}@{marketplace}"
                )));
            }
        }
        for key in [
            "computer-use@my-marketplace",
            "codex-app-tools@my-marketplace",
            "code-review@openai-bundled",
            "open-computer-use@openai-bundled",
            "pdf@openai-primary-runtime",
            "visualize@openai-bundled",
            "invalid",
        ] {
            assert!(!replaces_official_automation(key), "{key}");
        }
    }
}
