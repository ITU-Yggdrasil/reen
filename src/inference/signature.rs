//! Signature inference (bug 5).
//!
//! Walks scenarios per data operation, context functionality, and
//! projection query. Patterns are an explicit allow-list — when none
//! match we leave `derived_signature` as `None` and codegen falls back
//! to the `(&self)` stub. New shapes get added here in a controlled way
//! as drafts demand them, never by widening to regex magic.

use crate::manifest::DciVocabulary;
use crate::prepared::{
    Ambiguity, ApiSourceSpec, ArtifactKind, AttributeSpec, DerivedSignature, ParamSpec,
    PreparedArtifact, RolePlayerSpec, ScenarioSpec,
};

use super::phrases::{
    classify_projection_return, classify_return_shape, count_phrase, first_quoted_after,
    is_rejection_then,
};
use super::query_params::parse_query_parameters;

pub fn derive_signatures(artifact: &mut PreparedArtifact, vocab: &DciVocabulary) {
    let host_type = artifact.export.name.clone();
    let scenarios: Vec<ScenarioSpec> = artifact.scenarios.clone();
    let kind = artifact.kind.clone();
    let role_players: Vec<RolePlayerSpec> = artifact.role_players.clone();
    let attributes: Vec<AttributeSpec> = artifact.attributes.clone();

    let mut findings: Vec<Ambiguity> = Vec::new();

    match kind {
        ArtifactKind::Data => {
            for op in &mut artifact.operations {
                if op.kind.as_deref() == Some("with") {
                    // `with` operations have a fixed signature shape; do not
                    // touch them.
                    continue;
                }
                let (sig, finding) =
                    infer_for_data_operation(&op.name, &host_type, &scenarios, vocab);
                op.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        ArtifactKind::Context => {
            for func in &mut artifact.functionalities {
                let (sig, finding) =
                    infer_for_context_functionality(&func.name, &scenarios);
                func.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        ArtifactKind::Projection => {
            // Auto-generated api projections carry a structured Source block;
            // signature inference reads it directly. Hand-authored projections
            // walk scenarios as before.
            let api_source = artifact.api_source.clone();
            for func in &mut artifact.functionalities {
                let (sig, finding) = if let Some(src) = api_source.as_ref() {
                    (Some(infer_from_api_source(src)), None)
                } else {
                    infer_for_projection_query(
                        &func.name,
                        &scenarios,
                        &role_players,
                        vocab,
                    )
                };
                func.derived_signature = sig;
                if let Some(f) = finding {
                    findings.push(f);
                }
            }
        }
        _ => {}
    }

    let _ = attributes; // attributes are not signature-relevant
    artifact.ambiguities.extend(findings);
}

fn scenario_invokes(when_clause: &str, op_name: &str) -> bool {
    let when_lower = when_clause.to_ascii_lowercase();
    let op_lower = op_name.to_ascii_lowercase();
    let op_with_underscores = op_lower.replace(' ', "_");
    let op_with_spaces = op_lower.replace('_', " ");
    when_lower.contains(&op_lower)
        || when_lower.contains(&op_with_underscores)
        || when_lower.contains(&op_with_spaces)
}

fn infer_for_data_operation(
    op_name: &str,
    host_type: &str,
    scenarios: &[ScenarioSpec],
    _vocab: &DciVocabulary,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "operation `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When invokes this operation".into(),
                ),
                source_line: None,
            }),
        );
    }

    // Rejection scenarios specify error behaviour, not the success path.
    // The signature is derived from the happy-path scenarios only — including
    // a rejection in the aggregate would force the return to `()` and lose
    // the actual return type. If only rejections exist we fall back to all
    // matching scenarios so something is still inferred.
    let happy: Vec<&ScenarioSpec> = matching
        .iter()
        .copied()
        .filter(|s| !is_rejection_then(&s.then))
        .collect();
    let chosen: Vec<&ScenarioSpec> = if happy.is_empty() { matching } else { happy };

    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &chosen {
        // Count host-type references in Given to detect binary vs. unary.
        let host_refs = count_phrase(&sc.given, &format!("a {}", host_type));
        let mut parameters: Vec<ParamSpec> = Vec::new();
        if host_refs >= 2 {
            parameters.push(ParamSpec {
                name: "other".into(),
                rust_type: format!("&{}", host_type),
            });
        }
        // Quoted-string param in When clause (e.g. `to "awaiting-input"`).
        if let Some(quoted) = first_quoted_after(&sc.when, " to ") {
            let _ = quoted;
            parameters.push(ParamSpec {
                name: "value".into(),
                rust_type: "&str".into(),
            });
        }

        let returns = classify_return_shape(&sc.then, host_type);
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters,
            returns,
            source: "derived".into(),
        });
    }

    aggregate_signatures(op_name, sigs)
}

fn infer_for_context_functionality(
    op_name: &str,
    scenarios: &[ScenarioSpec],
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "functionality `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When invokes this functionality".into(),
                ),
                source_line: None,
            }),
        );
    }
    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &matching {
        let returns = classify_return_shape(&sc.then, "");
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters: Vec::new(),
            returns,
            source: "derived".into(),
        });
    }
    aggregate_signatures(op_name, sigs)
}

fn infer_for_projection_query(
    op_name: &str,
    scenarios: &[ScenarioSpec],
    role_players: &[RolePlayerSpec],
    _vocab: &DciVocabulary,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    let matching: Vec<&ScenarioSpec> = scenarios
        .iter()
        .filter(|s| scenario_invokes(&s.when, op_name))
        .collect();
    if matching.is_empty() {
        return (
            None,
            Some(Ambiguity {
                message: format!(
                    "query `{}` has no scenario; signature cannot be derived",
                    op_name,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario whose When queries this functionality".into(),
                ),
                source_line: None,
            }),
        );
    }

    // For projections, the Given describes role players already on self; we
    // look at When for parameters and Then for the return shape. The list
    // role player's element type is the canonical list element.
    let element_type = role_players
        .first()
        .map(|rp| {
            let ty = &rp.data_type;
            ty.clone()
        })
        .unwrap_or_default();

    let mut sigs: Vec<DerivedSignature> = Vec::new();
    for sc in &matching {
        let parameters = parse_query_parameters(&sc.when);
        let returns = classify_projection_return(&sc.then, &element_type);
        sigs.push(DerivedSignature {
            receiver: "&self".into(),
            parameters,
            returns,
            source: "derived".into(),
        });
    }
    aggregate_signatures(op_name, sigs)
}

/// Derive a projection's signature directly from an OpenAPI-shaped
/// [`ApiSourceSpec`]. The Source block is the source of truth for
/// auto-generated projections; no scenario walking, no aggregation.
fn infer_from_api_source(source: &ApiSourceSpec) -> DerivedSignature {
    let parameters = source
        .parameters
        .iter()
        .map(|p| ParamSpec {
            name: p.name.clone(),
            rust_type: p.rust_type.clone(),
        })
        .collect();
    let returns = if source.returns.is_empty() {
        "()".to_string()
    } else {
        source.returns.clone()
    };
    DerivedSignature {
        receiver: "&self".into(),
        parameters,
        returns,
        source: "api_source".into(),
    }
}

fn aggregate_signatures(
    op_name: &str,
    sigs: Vec<DerivedSignature>,
) -> (Option<DerivedSignature>, Option<Ambiguity>) {
    if sigs.is_empty() {
        return (None, None);
    }
    let first = sigs[0].clone();
    let all_agree = sigs.iter().all(|s| s == &first);
    if all_agree {
        return (Some(first), None);
    }

    // Disagreement on parameter arity is blocking; on return shape only,
    // informational (codegen still gets a useful signature from the first
    // shape, but we surface the discrepancy).
    let arity_disagrees = sigs.iter().any(|s| s.parameters.len() != first.parameters.len());
    let severity = if arity_disagrees { "blocking" } else { "low" };
    (
        None,
        Some(Ambiguity {
            message: format!(
                "scenarios for `{}` disagree on signature shape; cannot derive a single signature",
                op_name,
            ),
            severity: severity.to_string(),
            suggested_resolution: Some(
                "rewrite the scenarios so they describe a consistent receiver, parameters, and result".into(),
            ),
            source_line: None,
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::OperationSpec;

    fn data_artifact_with_op(name: &str, op: &str, scenarios: Vec<ScenarioSpec>) -> PreparedArtifact {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Data,
            format!("data/{}.md", name),
            name.into(),
            name.into(),
        );
        a.operations.push(OperationSpec {
            name: op.into(),
            description: "test op".into(),
            ..Default::default()
        });
        a.scenarios = scenarios;
        a
    }

    fn scenario(when: &str, given: &str, then: &str) -> ScenarioSpec {
        ScenarioSpec {
            name: "test".into(),
            given: given.into(),
            when: when.into(),
            then: then.into(),
        }
    }

    #[test]
    fn derive_signature_binary_operation_from_scenarios() {
        // Money.add — Given carries two `a Money of …` references, Then
        // returns `a Money of …`. Inference must produce
        // `&self, other: &Money) -> Money`.
        let mut a = data_artifact_with_op(
            "Money",
            "add",
            vec![scenario(
                "add is performed",
                "a Money of 5000 in DKK and a Money of 2500 in DKK",
                "the result is a Money of 7500 in DKK",
            )],
        );
        let vocab = DciVocabulary::default();
        derive_signatures(&mut a, &vocab);

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.receiver, "&self");
        assert_eq!(sig.parameters.len(), 1);
        assert_eq!(sig.parameters[0].name, "other");
        assert_eq!(sig.parameters[0].rust_type, "&Money");
        assert_eq!(sig.returns, "Money");
        assert_eq!(sig.source, "derived");
    }

    #[test]
    fn derive_signature_text_return_from_quoted_result() {
        // Money.display_as_major_units — Given has one Money, Then is
        // a quoted string. Inference: `&self) -> String`.
        let mut a = data_artifact_with_op(
            "Money",
            "display as major units",
            vec![scenario(
                "display as major units is performed",
                "a Money of 10050 in DKK",
                "the result is \"100.50 DKK\"",
            )],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.receiver, "&self");
        assert!(sig.parameters.is_empty());
        assert_eq!(sig.returns, "String");
    }

    #[test]
    fn derive_signature_boolean_return_from_affirmative_negative() {
        // Session.is_expired — Then is `the answer is negative`.
        let mut a = data_artifact_with_op(
            "Session",
            "is expired",
            vec![
                scenario(
                    "is expired is checked",
                    "a Session with expiry in the past",
                    "the answer is affirmative",
                ),
                scenario(
                    "is expired is checked",
                    "a Session with expiry in the future",
                    "the answer is negative",
                ),
            ],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        let sig = a.operations[0].derived_signature.as_ref().expect("sig");
        assert_eq!(sig.returns, "bool");
        assert_eq!(sig.receiver, "&self");
        assert!(sig.parameters.is_empty());
    }

    #[test]
    fn derive_signature_no_scenarios_emits_finding() {
        let mut a = data_artifact_with_op("Money", "untested_op", vec![]);
        derive_signatures(&mut a, &DciVocabulary::default());

        assert!(a.operations[0].derived_signature.is_none());
        let has_finding = a.ambiguities.iter().any(|x| {
            x.message.contains("untested_op") && x.message.contains("no scenario")
        });
        assert!(has_finding, "expected a no-scenario finding, got {:?}", a.ambiguities);
    }

    #[test]
    fn derive_signature_disagreement_emits_blocking_finding() {
        // Two scenarios for the same op disagree on arity — one supplies a
        // single Money operand, the other supplies two. Inference must
        // refuse to emit and surface a blocking finding.
        let mut a = data_artifact_with_op(
            "Money",
            "tricky",
            vec![
                scenario(
                    "tricky is performed",
                    "a Money of 100 in DKK",
                    "the result is a Money of 200 in DKK",
                ),
                scenario(
                    "tricky is performed",
                    "a Money of 100 in DKK and a Money of 50 in DKK",
                    "the result is a Money of 50 in DKK",
                ),
            ],
        );
        derive_signatures(&mut a, &DciVocabulary::default());

        assert!(a.operations[0].derived_signature.is_none());
        let blocker = a
            .ambiguities
            .iter()
            .find(|x| x.message.contains("tricky") && x.message.contains("disagree"))
            .expect("expected disagreement finding");
        assert_eq!(blocker.severity, "blocking");
    }
}
