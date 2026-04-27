use serde::{Deserialize, Serialize};

pub const PREPARED_SCHEMA_VERSION: &str = "reen.prepare/v5";
/// Retained for code that historically distinguished v2 from v3 — both now point
/// at the unified v3 schema after the projection-adopts-context-shape rename.
pub const PREPARED_SCHEMA_V3_VERSION: &str = "reen.prepare/v3";

// Schema migration notes
// ──────────────────────
// v3 → v4 (Group 2 / bugs 5+6):
//   * Added `derived_signature: Option<DerivedSignature>` to `OperationSpec`
//     and `FunctionalitySpec`. Populated by the scenario-driven inference pass
//     in `prepare`. Absent fields → codegen falls back to `(&self)` stub.
//   * Added `multiplicity: Multiplicity` to `RolePlayerSpec` and
//     `AttributeSpec`. Default `Single`. `List` causes codegen to wrap the
//     field type in `Vec<T>`. v3 documents without the field deserialise
//     successfully (the default kicks in).
// v4 → v5 (Group 4 / bug 1 long-term):
//   * Added `data_variants: Vec<DataVariantSpec>` on `PreparedArtifact`,
//     populated for variant data types (sum types modelled as `### <name>
//     attributes` H3 sub-sections under `## Attributes`). Empty for non-
//     variant data types. `DataVariantSpec` carries a name and a list of
//     `AttributeSpec` (the variant's own attributes). v4 documents without
//     the field deserialise unchanged.

/// DCI artifact kinds.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Data,
    Projection,
    Context,
    Component,
    Brand,
    RoutingTable,
    App,
}

impl ArtifactKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            ArtifactKind::Data => "data",
            ArtifactKind::Projection => "projection",
            ArtifactKind::Context => "context",
            ArtifactKind::Component => "component",
            ArtifactKind::Brand => "brand",
            ArtifactKind::RoutingTable => "routing_table",
            ArtifactKind::App => "app",
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
    /// Variant groups for sum-typed data drafts (e.g. ChatTurn, TransferRequest).
    /// Empty for ordinary data types — the wrapper-struct + payload-enum codegen
    /// only fires when this list is non-empty.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub data_variants: Vec<DataVariantSpec>,

    // ── Projection (v3+) ──────────────────────────────────────────
    // Projections share the context-shaped fields above (role_players,
    // role_methods, props, functionalities). The legacy `source_data` /
    // `queries` fields have been removed from the schema.

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

    // ── Auto-generated api projection (Phase A) ──────────────────
    /// Populated for projections emitted by `reen apis ingest` from an
    /// OpenAPI sidecar. Carries the structured HTTP shape so signature
    /// inference can derive the projection's `derived_signature`
    /// directly, without scenarios.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub api_source: Option<ApiSourceSpec>,

    // ── App-only ─────────────────────────────────────────────────
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub application_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_context: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry_functionality: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub app_dependencies: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub notes: Option<String>,

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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct RolePlayerSpec {
    pub name: String,
    /// Data or projection name from the manifest.
    pub data_type: String,
    pub responsibility: String,
    /// Whether the role binds a single value or a collection of them.
    /// Inferred at prepare time from the role player's name plurality, the
    /// description prose, and (decisively) the shape of `Given:` clauses
    /// across scenarios. Defaults to `Single` so v3-era prepared YAML stays
    /// valid.
    #[serde(default, skip_serializing_if = "Multiplicity::is_single")]
    pub multiplicity: Multiplicity,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FunctionalitySpec {
    pub name: String,
    /// Prose describing the enactment.
    pub description: String,
    /// Role method names referenced in this functionality's prose.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub role_method_refs: Vec<String>,
    /// Inferred from scenarios at prepare time. Absent when no scenario
    /// invokes this functionality or when scenarios disagree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_signature: Option<DerivedSignature>,
}

// ── API source (Phase A) ─────────────────────────────────────────

/// The structured `## Source` block emitted by `reen apis ingest` into
/// auto-generated projection drafts under `drafts/projections/.api/`.
/// Carries the HTTP shape (method, path, parameters, return type) that
/// would otherwise have to be reconstructed from prose. Signature
/// inference reads this directly when present, bypassing scenario
/// walking — auto-generated drafts have no scenarios by design.
///
/// Optional on `PreparedArtifact`; absent for hand-authored projections.
/// v5 documents that don't carry the field deserialise unchanged.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiSourceSpec {
    /// The OpenAPI sidecar reference, e.g.
    /// `nordea-openbanking#operations/getAccounts`. Informational only;
    /// signature inference does not parse it.
    pub openapi_ref: String,
    /// HTTP method (`GET`, `POST`, etc.).
    pub method: String,
    /// Path with `{param}` placeholders.
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<ApiParamSpec>,
    /// Pre-rendered return type fragment, e.g. `Vec<AccountSummary>`,
    /// `String`, `Vec<u8>`. Empty string means unit return.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub returns: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ApiParamSpec {
    pub name: String,
    /// One of `path`, `query`, `header`, `body`.
    pub location: String,
    pub required: bool,
    /// Pre-rendered Rust type fragment, e.g. `&str`, `i64`,
    /// `&AccountSummary`.
    pub rust_type: String,
}

// ── Inference outputs (v4) ───────────────────────────────────────

/// Cardinality of a role player or attribute. Inferred at prepare time.
/// `List` causes codegen to wrap the field type in `Vec<T>`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum Multiplicity {
    #[default]
    Single,
    List,
}

impl Multiplicity {
    pub fn is_single(&self) -> bool {
        matches!(self, Multiplicity::Single)
    }
}

/// A signature derived from prose + scenarios. The fields hold pre-rendered
/// Rust fragments so codegen can splice them directly. Absence is the
/// fallback; codegen then emits `pub fn <name>(&self) { todo!() }`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct DerivedSignature {
    /// "&self" or "&mut self" or "self".
    pub receiver: String,
    pub parameters: Vec<ParamSpec>,
    /// Rust return type fragment, e.g. "()", "bool", "String", "Money",
    /// "Vec<Transaction>". The codegen splices this verbatim.
    pub returns: String,
    /// Always "derived" today; reserved for future "authored" sources.
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ParamSpec {
    pub name: String,
    /// Rust type fragment, e.g. "&Money", "String", "i64".
    pub rust_type: String,
}

// ── Data types ────────────────────────────────────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct AttributeSpec {
    pub name: String,
    pub description: String,
    /// Whether the attribute holds a single value or a collection. Same
    /// inference rules as role-player multiplicity; defaults to `Single`.
    #[serde(default, skip_serializing_if = "Multiplicity::is_single")]
    pub multiplicity: Multiplicity,
}

/// One variant of a sum-typed data type. `name` is canonical snake_case
/// (e.g. `user_utterance`, `confirmation_card_rejected`); `attributes` are the
/// variant's own per-instance fields. Top-level shared attributes live on the
/// surrounding `PreparedArtifact.attributes`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DataVariantSpec {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attributes: Vec<AttributeSpec>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct OperationSpec {
    pub name: String,
    pub description: String,
    /// Kind marker. `Some("with")` indicates a non-destructive mutation
    /// operation — the scaffolder emits `with_<field>(self, value) -> Result<Self>`.
    /// Authored in the draft as `(kind: with)` after the operation name.
    /// Additive field; absent in legacy prepared YAML.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// Signature inferred from scenarios at prepare time. Absent when no
    /// scenario invokes this operation or when scenarios disagree.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_signature: Option<DerivedSignature>,
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

// ── Construction helpers ──────────────────────────────────────────

impl PreparedArtifact {
    pub fn new(kind: ArtifactKind, path: String, title: String, export_name: String) -> Self {
        let kind_str = kind.as_str();
        // All artefact kinds now use the unified v3 schema after the
        // projection-adopts-context-shape rename.
        let schema = PREPARED_SCHEMA_VERSION.to_string();
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
            data_variants: Vec::new(),
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
            api_source: None,
            // App-only
            application_kind: None,
            entry_context: None,
            entry_functionality: None,
            app_dependencies: Vec::new(),
            notes: None,
        }
    }

    pub fn blocking_ambiguities(&self) -> impl Iterator<Item = &Ambiguity> {
        self.ambiguities
            .iter()
            .filter(|a| a.severity.eq_ignore_ascii_case("blocking"))
    }
}
