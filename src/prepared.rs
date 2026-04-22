use serde::{Deserialize, Serialize};

pub const PREPARED_SCHEMA_VERSION: &str = "reen.prepare/v2";
pub const PREPARED_SCHEMA_V3_VERSION: &str = "reen.prepare/v3";

/// DCI artifact kinds plus the operation kind for external API consumption.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
    Operation,
    Component,
    Brand,
    RoutingTable,
}

impl ArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
            ArtifactKind::Operation => "operation",
            ArtifactKind::Component => "component",
            ArtifactKind::Brand => "brand",
            ArtifactKind::RoutingTable => "routing_table",
        }
    }
}

/// A fully-resolved, prepared artifact in the v2 schema.
///
/// Fields are grouped by artifact kind. Kind-inappropriate fields serialize as empty
/// collections (and are skipped in YAML output via `skip_serializing_if`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PreparedArtifact {
    pub schema: String,
    pub kind: ArtifactKind,
    pub source: SourceInfo,
    pub export: ExportInfo,

    // ── Context-only ──────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_players: Vec<RolePlayerSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_methods: Vec<RoleMethodGroupSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub props: Vec<PropSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub functionalities: Vec<FunctionalitySpec>,

    // ── Data-only ─────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invariants: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub operations: Vec<OperationSpec>,

    // ── Projection-only ───────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_data: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub queries: Vec<QuerySpec>,

    // ── Component-only (v3) ──────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<SlotSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<EventSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<VariantSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub composition: Option<CompositionSpec>,

    // ── Brand-only (v3) ──────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub token_groups: Vec<TokenGroupSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub prose_sections: Vec<ProseSectionSpec>,

    // ── Routing-table-only (v3) ──────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<RouteSpec>,

    // ── Context classification (v3) ──────────────────────────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub realisation_target: Option<String>,

    // ── Shared across all kinds ───────────────────────────────────
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scenarios: Vec<ScenarioSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub synthesised_tests: Vec<SynthesisedTest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub vocabulary_bindings: Vec<VocabularyBinding>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ambiguities: Vec<Ambiguity>,
}

// ── Source / Export ────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SourceInfo {
    pub path: String,
    pub kind: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ExportInfo {
    pub name: String,
}

// ── Context types ─────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePlayerSpec {
    pub name: String,
    /// Data or projection name from the manifest.
    pub data_type: String,
    pub responsibility: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleMethodGroupSpec {
    pub role: String,
    pub methods: Vec<RoleMethodSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RoleMethodSpec {
    pub name: String,
    /// The canonical `<role>_<method>` name used in Rust.
    pub canonical_name: String,
    /// Signature hint inferred from scenarios and role method description.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature_hint: Option<String>,
    /// Draft text that describes this role method's responsibility.
    pub citation: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PropSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionalitySpec {
    pub name: String,
    /// Prose describing the enactment.
    pub description: String,
    /// Role method names referenced in this functionality's prose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_method_refs: Vec<String>,
}

// ── Data types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributeSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationSpec {
    pub name: String,
    pub description: String,
}

// ── Projection types ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct QuerySpec {
    pub name: String,
    pub description: String,
}

// ── Component types (v3) ─────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SlotSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VariantSpec {
    pub name: String,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ChildBindingSpec {
    pub child: String,
    pub bindings: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventBindingSpec {
    pub child: String,
    pub event: String,
    pub context: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CompositionSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub child_bindings: Vec<ChildBindingSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub event_bindings: Vec<EventBindingSpec>,
}

// ── Brand types (v3) ─────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenSpec {
    pub name: String,
    pub role: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TokenGroupSpec {
    pub group: String,
    pub tokens: Vec<TokenSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ProseSectionSpec {
    pub name: String,
    pub body: String,
}

// ── Routing table types (v3) ─────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteSpec {
    pub path: String,
    pub component: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub params: Vec<RouteParamSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteParamSpec {
    pub name: String,
    pub data_type: String,
}

// ── Shared types ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScenarioSpec {
    pub name: String,
    pub given: String,
    pub when: String,
    pub then: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SynthesisedTest {
    /// `"scenario"` or `"invariant"`.
    pub source: String,
    pub given: String,
    pub when: String,
    pub then: String,
    /// Location in the draft that motivated this test.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub citation: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct VocabularyBinding {
    pub name: String,
    /// One of: `role_player`, `role`, `role_method`, `data`, `projection`, `prop`,
    /// `primitive`, `ambiguous`.
    pub resolution: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_to: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Ambiguity {
    pub message: String,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_resolution: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_line: Option<usize>,
}

// ── Operation spec (reen.operation/v1) ───────────────────────────

pub const OPERATION_SCHEMA_VERSION: &str = "reen.operation/v1";

/// A typed operation specification for an external API operation.
/// Deserialized directly from YAML — no prose parsing needed.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiOperationSpec {
    pub schema: String,
    pub api: String,
    pub name: String,
    pub transport: TransportSpec,
    pub auth: AuthSpec,
    pub input: IoSpec,
    pub output: IoSpec,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ErrorSpec>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TransportSpec {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub method: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AuthSpec {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct IoSpec {
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ErrorSpec {
    pub data_type: String,
}

/// The generated API manifest (`drafts/.apis/manifest.yml`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiManifest {
    pub schema: String,
    pub apis: Vec<ApiManifestEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiManifestEntry {
    pub name: String,
    pub openapi_hash: String,
    pub data_types: Vec<String>,
    pub operations: Vec<String>,
}

pub const API_MANIFEST_SCHEMA: &str = "reen.api-manifest/v1";

// ── Construction helpers ──────────────────────────────────────────

impl PreparedArtifact {
    pub fn new(kind: ArtifactKind, path: String, title: String, export_name: String) -> Self {
        let kind_str = kind.as_str();
        let schema = match &kind {
            ArtifactKind::Component | ArtifactKind::Brand | ArtifactKind::RoutingTable => {
                PREPARED_SCHEMA_V3_VERSION.to_string()
            }
            _ => PREPARED_SCHEMA_VERSION.to_string(),
        };
        Self {
            schema,
            source: SourceInfo {
                path,
                kind: kind_str.to_string(),
                title,
            },
            kind,
            export: ExportInfo { name: export_name },
            role_players: Vec::new(),
            role_methods: Vec::new(),
            props: Vec::new(),
            functionalities: Vec::new(),
            attributes: Vec::new(),
            invariants: Vec::new(),
            operations: Vec::new(),
            source_data: None,
            queries: Vec::new(),
            scenarios: Vec::new(),
            synthesised_tests: Vec::new(),
            vocabulary_bindings: Vec::new(),
            ambiguities: Vec::new(),
            // Component-only
            slots: Vec::new(),
            events: Vec::new(),
            variants: Vec::new(),
            composition: None,
            // Brand-only
            token_groups: Vec::new(),
            prose_sections: Vec::new(),
            // Routing-table-only
            routes: Vec::new(),
            // Context classification
            realisation_target: None,
        }
    }

    pub fn blocking_ambiguities(&self) -> impl Iterator<Item = &Ambiguity> {
        self.ambiguities
            .iter()
            .filter(|a| a.severity.eq_ignore_ascii_case("blocking"))
    }
}
