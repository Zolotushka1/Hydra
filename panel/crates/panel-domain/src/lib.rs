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
    /// This crate's own source carries no API that only works on a host.
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
    ///
    /// This covers only what is written here. A dependency that compiles for
    /// wasm32 and panics at run time is invisible to it and to the build alike,
    /// which is what `dependencies_stay_minimal` below is for.
    #[test]
    fn domain_source_is_free_of_host_only_apis() {
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

#[cfg(test)]
mod dependency_tests {
    use std::collections::BTreeSet;

    /// The domain depends on exactly three crates.
    ///
    /// This crate is compiled for `wasm32-unknown-unknown` by the Leptos
    /// frontend, and neither the build nor the source scan above catches the
    /// dangerous case: a dependency that compiles for wasm32 and then panics in
    /// the browser. Most of the ecosystem's host-oriented crates do exactly
    /// that, and the failure surfaces as an unreachable trap with no source
    /// location.
    ///
    /// So the set is pinned rather than the behaviour audited. Adding a fourth
    /// dependency turns this red, which forces the question — does it work on
    /// wasm32 at run time, not just at compile time — to be answered before the
    /// frontend inherits it.
    ///
    /// Parsed from the manifest rather than from a lockfile: these are the
    /// direct dependencies, and transitive ones are the business of whoever
    /// added the direct one.
    #[test]
    fn dependencies_stay_minimal() {
        const ALLOWED: &[&str] = &["serde", "serde_json", "thiserror"];

        let manifest = include_str!("../Cargo.toml");
        let mut declared = BTreeSet::new();
        let mut inside_dependencies = false;

        for line in manifest.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with('[') {
                // Only runtime dependencies. `[dev-dependencies]` never reaches
                // the frontend, and a build script does not run on the target.
                inside_dependencies = trimmed == "[dependencies]";
                continue;
            }
            if !inside_dependencies || trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((name, _)) = trimmed.split_once('=') {
                // `serde.workspace = true` puts the key on the left of the `=`,
                // so the crate name is the first dotted segment.
                let name = name.trim().split('.').next().unwrap_or_default();
                declared.insert(name.to_string());
            }
        }

        let allowed: BTreeSet<String> = ALLOWED.iter().map(|item| item.to_string()).collect();

        assert!(
            !declared.is_empty(),
            "the manifest scan found no dependencies, so it is not working"
        );

        let added: Vec<&String> = declared.difference(&allowed).collect();
        assert!(
            added.is_empty(),
            "the Leptos frontend compiles this crate for wasm32, where a crate can \
             compile and still trap at run time. Confirm these work on that target \
             before adding them here:\n  {}",
            added
                .iter()
                .map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join("\n  ")
        );

        let removed: Vec<&String> = allowed.difference(&declared).collect();
        assert!(
            removed.is_empty(),
            "these are no longer dependencies; drop them from ALLOWED:\n  {}",
            removed
                .iter()
                .map(|item| item.as_str())
                .collect::<Vec<_>>()
                .join("\n  ")
        );
    }
}
