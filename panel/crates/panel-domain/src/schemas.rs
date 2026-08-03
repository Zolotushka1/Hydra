//! Schema version registry.
//!
//! The single place version numbers live. Document constructors read their
//! version from here and `GET /api/ui/contracts` publishes the same registry, so
//! the version advertised to the frontend and the version inside a document body
//! match by construction.
//!
//! Before the registry these were two lists and had already diverged: the
//! contract advertised `protocol_capabilities` version 1 while
//! `ProtocolCapabilitiesView` emitted 3.
//!
//! Rules for changing versions live in `docs/api.md`, section Versioning Policy.

use serde::{Deserialize, Serialize};

/// What exactly is versioned.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SchemaKind {
    /// A concrete document: it has exactly one constructor, which must take its
    /// version from the registry.
    Document,
    /// A data model spread across several documents. It has no constructor of its
    /// own; the version describes the evolution of the model as a whole.
    Model,
    /// An on-disk file format. Not published to the external contract.
    Persistence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SchemaSpec {
    pub name: &'static str,
    pub version: u16,
    pub kind: SchemaKind,
}

/// Schema identifier.
///
/// Exists for the same reason as `RouteId` in `routes.rs`: a `match` over it is
/// exhaustive, so a new schema without a registry entry fails to compile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SchemaId {
    UiBootstrap,
    UiOverview,
    UiContracts,
    ResourceBudget,
    SubscriptionClientAccessPreview,
    SubscriptionBundle,
    SubscriptionCatalog,
    NodeRuntimeConfig,
    ApplyPlan,
    ProtocolCapabilities,
    NodeRuntimeValidation,
    PanelAccessModes,
    PanelInstallPlan,
    PanelInstallerBootstrap,
    PanelInstallerSession,
    NodeProvisioningExecutorContract,
    RouteMaterialStore,
    RealityMaterialStore,
}

impl SchemaId {
    pub const ALL: &'static [Self] = &[
        Self::UiBootstrap,
        Self::UiOverview,
        Self::UiContracts,
        Self::ResourceBudget,
        Self::SubscriptionClientAccessPreview,
        Self::SubscriptionBundle,
        Self::SubscriptionCatalog,
        Self::NodeRuntimeConfig,
        Self::ApplyPlan,
        Self::ProtocolCapabilities,
        Self::NodeRuntimeValidation,
        Self::PanelAccessModes,
        Self::PanelInstallPlan,
        Self::PanelInstallerBootstrap,
        Self::PanelInstallerSession,
        Self::NodeProvisioningExecutorContract,
        Self::RouteMaterialStore,
        Self::RealityMaterialStore,
    ];

    /// Exhaustive `match`: a schema without an entry fails to compile.
    pub const fn spec(self) -> SchemaSpec {
        match self {
            Self::UiBootstrap => SchemaSpec {
                name: "ui_bootstrap",
                version: 2,
                kind: SchemaKind::Document,
            },
            Self::UiOverview => SchemaSpec {
                name: "ui_overview",
                version: 1,
                kind: SchemaKind::Document,
            },
            // 2: endpoints changed from strings to {method, path, paginated}
            // objects, and both the route list and enum values became generated.
            // 3: protocol values were removed from the enum registry. The document
            // shape did not change, but a client holding a cached contract would
            // keep offering vmess/trojan/shadowsocks and keep sending values the
            // panel now rejects. The version is its only signal to re-read, so it
            // is raised.
            Self::UiContracts => SchemaSpec {
                name: "ui_contracts",
                version: 3,
                kind: SchemaKind::Document,
            },
            Self::ResourceBudget => SchemaSpec {
                name: "resource_budget",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::SubscriptionClientAccessPreview => SchemaSpec {
                name: "subscription_client_access_preview",
                version: 1,
                kind: SchemaKind::Document,
            },
            // The `diagnostic_json` document served to a subscription client.
            Self::SubscriptionBundle => SchemaSpec {
                name: "subscription_bundle",
                version: 1,
                kind: SchemaKind::Document,
            },
            // The subscription catalog model as a whole: plans, clients, devices,
            // enrollment grants. It has no single constructor; the version tracks
            // the model's evolution rather than one document body.
            Self::SubscriptionCatalog => SchemaSpec {
                name: "subscription_catalog",
                version: 8,
                kind: SchemaKind::Model,
            },
            Self::NodeRuntimeConfig => SchemaSpec {
                name: "node_runtime_config",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::ApplyPlan => SchemaSpec {
                name: "apply_plan",
                version: 2,
                kind: SchemaKind::Document,
            },
            Self::ProtocolCapabilities => SchemaSpec {
                name: "protocol_capabilities",
                version: 5,
                kind: SchemaKind::Document,
            },
            Self::NodeRuntimeValidation => SchemaSpec {
                name: "node_runtime_validation",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::PanelAccessModes => SchemaSpec {
                name: "panel_access_modes",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::PanelInstallPlan => SchemaSpec {
                name: "panel_install_plan",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::PanelInstallerBootstrap => SchemaSpec {
                name: "panel_installer_bootstrap",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::PanelInstallerSession => SchemaSpec {
                name: "panel_installer_session",
                version: 1,
                kind: SchemaKind::Document,
            },
            Self::NodeProvisioningExecutorContract => SchemaSpec {
                name: "node_provisioning_executor_contract",
                version: 1,
                kind: SchemaKind::Document,
            },
            // On-disk store format. Never published outward.
            Self::RouteMaterialStore => SchemaSpec {
                name: "route_material_store",
                version: 1,
                kind: SchemaKind::Persistence,
            },
            // Per-inbound Reality key pairs and short ids. Private keys are
            // encrypted and the store is never published outward.
            Self::RealityMaterialStore => SchemaSpec {
                name: "reality_material_store",
                version: 1,
                kind: SchemaKind::Persistence,
            },
        }
    }

    pub const fn version(self) -> u16 {
        self.spec().version
    }

    pub const fn name(self) -> &'static str {
        self.spec().name
    }

    pub const fn kind(self) -> SchemaKind {
        self.spec().kind
    }
}

/// Schemas published to the frontend: everything except on-disk formats.
pub fn published_schemas() -> impl Iterator<Item = SchemaId> {
    SchemaId::ALL
        .iter()
        .copied()
        .filter(|schema| !matches!(schema.kind(), SchemaKind::Persistence))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn schema_names_are_unique_and_identifier_shaped() {
        let mut seen = HashSet::new();
        for schema in SchemaId::ALL {
            let name = schema.name();
            assert!(seen.insert(name), "duplicate schema name {name}");
            assert!(
                !name.is_empty()
                    && name.len() <= 64
                    && name.chars().all(|symbol| symbol.is_ascii_lowercase()
                        || symbol.is_ascii_digit()
                        || symbol == '_'),
                "schema name {name} does not look like an identifier"
            );
        }
    }

    #[test]
    fn schema_versions_start_at_one() {
        for schema in SchemaId::ALL {
            assert!(
                schema.version() >= 1,
                "{}: schema versions start at 1",
                schema.name()
            );
        }
    }

    #[test]
    fn persistence_schemas_are_not_published() {
        let published: HashSet<&str> = published_schemas().map(|schema| schema.name()).collect();
        let persistence = SchemaId::ALL
            .iter()
            .filter(|schema| matches!(schema.kind(), SchemaKind::Persistence))
            .count();
        assert!(
            persistence >= 1,
            "persistence schemas vanished from the registry"
        );
        for schema in SchemaId::ALL {
            if matches!(schema.kind(), SchemaKind::Persistence) {
                assert!(
                    !published.contains(schema.name()),
                    "{}: an on-disk store format leaked into the external contract",
                    schema.name()
                );
            }
        }
        assert_eq!(published.len(), SchemaId::ALL.len() - persistence);
    }
}
