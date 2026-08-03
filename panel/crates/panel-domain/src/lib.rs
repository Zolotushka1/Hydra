pub mod cluster;
pub mod configgen;
pub mod health;
pub mod installer;
pub mod network;
pub mod node;
pub mod provisioning;
pub mod routes;
pub mod schemas;
#[macro_use]
mod registry;
pub mod security;
pub mod subscription;
pub mod system;
pub mod telegram;
pub mod ui;
pub mod user;
pub mod xray;

#[cfg(test)]
mod wasm_portability_tests {
    /// The domain carries no API that only works on a host.
    ///
    /// The Leptos frontend compiles this crate for `wasm32-unknown-unknown`, and
    /// CI builds it for that target — but a build is only half a guarantee. A
    /// crate dependency that does not support wasm fails to compile there; a
    /// call to `std::fs` or `std::net` does not. Those compile into stubs and
    /// fail at run time instead, in the browser, where the failure is hardest to
    /// read.
    ///
    /// Source scanning because the property is "this code does not mention these
    /// APIs", which no type expresses. Checked per module rather than over the
    /// whole crate so the failure names the file.
    #[test]
    fn domain_stays_free_of_host_only_apis() {
        // Time is included because `SystemTime::now` panics on wasm32 rather
        // than returning an error: timestamps arrive from the server in the
        // documents themselves and are never taken locally.
        const FORBIDDEN: &[&str] = &[
            "std::fs",
            "std::net",
            "std::process",
            "std::env",
            "std::thread",
            "std::os::",
            "SystemTime",
            "Instant::now",
        ];

        let modules: &[(&str, &str)] = &[
            ("cluster.rs", include_str!("cluster.rs")),
            ("configgen.rs", include_str!("configgen.rs")),
            ("health.rs", include_str!("health.rs")),
            ("installer.rs", include_str!("installer.rs")),
            // Only the module declarations: this test's own body names every
            // forbidden API, so scanning it would report itself.
            (
                "lib.rs",
                include_str!("lib.rs")
                    .split("#[cfg(test)]")
                    .next()
                    .unwrap_or_default(),
            ),
            ("network.rs", include_str!("network.rs")),
            ("node.rs", include_str!("node.rs")),
            ("provisioning.rs", include_str!("provisioning.rs")),
            ("registry.rs", include_str!("registry.rs")),
            ("routes.rs", include_str!("routes.rs")),
            ("schemas.rs", include_str!("schemas.rs")),
            ("security.rs", include_str!("security.rs")),
            ("subscription.rs", include_str!("subscription.rs")),
            ("system.rs", include_str!("system.rs")),
            ("telegram.rs", include_str!("telegram.rs")),
            ("ui.rs", include_str!("ui.rs")),
            ("user.rs", include_str!("user.rs")),
            ("xray.rs", include_str!("xray.rs")),
        ];

        let mut found = Vec::new();
        for (name, source) in modules {
            for line in source.lines() {
                // This test names every forbidden API, so it would report
                // itself.
                if line.trim_start().starts_with("//") {
                    continue;
                }
                for api in FORBIDDEN {
                    if line.contains(api) {
                        found.push(format!("{name}: {api} in `{}`", line.trim()));
                    }
                }
            }
        }

        assert!(
            found.is_empty(),
            "the domain is compiled for wasm32 by the Leptos frontend, where these \
             either panic or fail at run time:\n  {}",
            found.join("\n  ")
        );
    }
}
