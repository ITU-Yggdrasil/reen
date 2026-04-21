use serde::{Deserialize, Serialize};

pub const PREPARED_SCHEMA_VERSION: &str = "reen.prepare/v2";

/// DCI artifact kinds plus the operation kind for external API consumption.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
    Operation,
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
        let kind_str = match &kind {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
            ArtifactKind::Operation => "operation",
        };
        Self {
            schema: PREPARED_SCHEMA_VERSION.to_string(),
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
        }
    }

    pub fn blocking_ambiguities(&self) -> impl Iterator<Item = &Ambiguity> {
        self.ambiguities
            .iter()
            .filter(|a| a.severity.eq_ignore_ascii_case("blocking"))
    }
}
