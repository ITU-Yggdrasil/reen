//! Multiplicity inference (bug 6).
//!
//! Three cues, ordered weakest → strongest:
//!   1. **Name plurality** — `transactions`, `accounts`, `scopes`,
//!      `turns`. Strong list cue. Singular nouns are a strong single
//!      cue.
//!   2. **Description prose** — phrases like `a list of`,
//!      `the transactions`, `all the X` suggest a collection.
//!   3. **Scenario `Given:` shape (decisive)** — if the entity appears
//!      with a count word (`five transactions`, `transactions with
//!      amounts X, Y, Z`), it's a list. If it appears as a single
//!      instance (`a Session with …`), it's single.
//!
//! Aggregation: scenarios beat names beat prose. Disagreements (plural
//! name but singular scenarios) raise an informational finding so the
//! BA can clarify with a scenario.

use crate::prepared::{Ambiguity, Multiplicity, PreparedArtifact, ScenarioSpec};

pub fn derive_multiplicity(artifact: &mut PreparedArtifact) {
    let scenarios: Vec<ScenarioSpec> = artifact.scenarios.clone();

    // Role players first.
    let mut findings: Vec<Ambiguity> = Vec::new();
    for rp in &mut artifact.role_players {
        let name_cue = name_plurality(&rp.name);
        let prose_cue = prose_plurality(&rp.responsibility);
        let scenario_cue = scenario_plurality(&rp.name, &scenarios);

        let resolved = combine_multiplicity_cues(scenario_cue, name_cue, prose_cue);
        rp.multiplicity = resolved.multiplicity;
        if let Some(message) = resolved.disagreement {
            findings.push(Ambiguity {
                message: format!(
                    "role player `{}` has conflicting multiplicity cues: {}",
                    rp.name, message,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario that makes the cardinality unambiguous".into(),
                ),
                source_line: None,
            });
        }
    }

    for attr in &mut artifact.attributes {
        let name_cue = name_plurality(&attr.name);
        let prose_cue = prose_plurality(&attr.description);
        let scenario_cue = scenario_plurality(&attr.name, &scenarios);

        let resolved = combine_multiplicity_cues(scenario_cue, name_cue, prose_cue);
        attr.multiplicity = resolved.multiplicity;
        if let Some(message) = resolved.disagreement {
            findings.push(Ambiguity {
                message: format!(
                    "attribute `{}` has conflicting multiplicity cues: {}",
                    attr.name, message,
                ),
                severity: "low".to_string(),
                suggested_resolution: Some(
                    "add a scenario that makes the cardinality unambiguous".into(),
                ),
                source_line: None,
            });
        }
    }

    // Per-variant attributes get the same inference. Scenarios are shared at
    // the data-draft level, so a multi-instance phrase in any scenario can
    // still inform a variant attribute's multiplicity.
    for variant in &mut artifact.data_variants {
        for attr in &mut variant.attributes {
            let name_cue = name_plurality(&attr.name);
            let prose_cue = prose_plurality(&attr.description);
            let scenario_cue = scenario_plurality(&attr.name, &scenarios);
            let resolved = combine_multiplicity_cues(scenario_cue, name_cue, prose_cue);
            attr.multiplicity = resolved.multiplicity;
            if let Some(message) = resolved.disagreement {
                findings.push(Ambiguity {
                    message: format!(
                        "variant `{}` attribute `{}` has conflicting multiplicity cues: {}",
                        variant.name, attr.name, message,
                    ),
                    severity: "low".to_string(),
                    suggested_resolution: Some(
                        "add a scenario that makes the cardinality unambiguous".into(),
                    ),
                    source_line: None,
                });
            }
        }
    }

    artifact.ambiguities.extend(findings);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Cue {
    Single,
    List,
    Unknown,
}

pub(super) struct ResolvedMultiplicity {
    pub multiplicity: Multiplicity,
    pub disagreement: Option<String>,
}

pub(super) fn combine_multiplicity_cues(
    scenario: Cue,
    name: Cue,
    prose: Cue,
) -> ResolvedMultiplicity {
    // Scenario decides outright when it knows.
    let chosen = match (scenario, name, prose) {
        (Cue::List, _, _) => Cue::List,
        (Cue::Single, _, _) => Cue::Single,
        (_, Cue::List, _) => Cue::List,
        (_, Cue::Single, _) => Cue::Single,
        (_, _, Cue::List) => Cue::List,
        (_, _, Cue::Single) => Cue::Single,
        _ => Cue::Single,
    };
    let multiplicity = match chosen {
        Cue::List => Multiplicity::List,
        _ => Multiplicity::Single,
    };

    // Disagreement detection: scenario says one thing, name says the opposite.
    let disagreement = match (scenario, name) {
        (Cue::Single, Cue::List) => Some(format!(
            "name `{:?}` is plural but scenarios consistently supply a single instance",
            name,
        )),
        (Cue::List, Cue::Single) => Some(format!(
            "name `{:?}` is singular but scenarios supply multiple instances",
            name,
        )),
        _ => None,
    };
    ResolvedMultiplicity {
        multiplicity,
        disagreement,
    }
}

const IRREGULAR_PLURALS: &[(&str, &str)] = &[
    ("children", "child"),
    ("people", "person"),
    ("men", "man"),
    ("women", "woman"),
    ("feet", "foot"),
    ("teeth", "tooth"),
    ("mice", "mouse"),
    ("geese", "goose"),
];

pub(super) fn name_plurality(name: &str) -> Cue {
    let lower = name.to_ascii_lowercase();
    let last_word = lower
        .rsplit(|c: char| c == ' ' || c == '_' || c == '-')
        .next()
        .unwrap_or(&lower);
    for (plural, _) in IRREGULAR_PLURALS {
        if last_word == *plural {
            return Cue::List;
        }
    }
    if last_word.ends_with("ies") && last_word.len() > 3 {
        return Cue::List;
    }
    if last_word.ends_with("ses") || last_word.ends_with("xes") || last_word.ends_with("zes") {
        return Cue::List;
    }
    if last_word.ends_with('s') && !last_word.ends_with("ss") && last_word.len() > 2 {
        // Coarse English plural rule. False positives ("status", "ios") are
        // rare in this domain; the scenario cue overrides anyway.
        return Cue::List;
    }
    Cue::Single
}

pub(super) fn prose_plurality(prose: &str) -> Cue {
    let lower = prose.to_ascii_lowercase();
    if lower.contains("a list of")
        || lower.contains("the list of")
        || lower.contains("the ordered ")
        || lower.contains("the collection of")
        || lower.contains("all the ")
    {
        return Cue::List;
    }
    Cue::Unknown
}

pub(super) fn scenario_plurality(role_name: &str, scenarios: &[ScenarioSpec]) -> Cue {
    let role_lower = role_name.to_ascii_lowercase();
    let role_singular = depluralise(&role_lower);

    let mut list_hits = 0usize;
    let mut single_hits = 0usize;

    for sc in scenarios {
        let blob = format!("{}\n{}\n{}", sc.given, sc.when, sc.then).to_ascii_lowercase();
        // Multiple-instance phrases.
        if blob.contains(&format!("{} with amounts ", role_lower))
            || blob.contains(&format!("{}: ", role_lower)) // not reliable; skip
            || COUNT_WORDS
                .iter()
                .any(|w| blob.contains(&format!("{} {}", w, role_lower)))
        {
            list_hits += 1;
            continue;
        }
        // Single-instance phrases.
        if blob.contains(&format!("a {}", role_singular))
            || blob.contains(&format!("the {}", role_singular))
            || blob.contains(&format!("{} with ", role_singular))
        {
            single_hits += 1;
        }
    }

    if list_hits > 0 && list_hits >= single_hits {
        Cue::List
    } else if single_hits > 0 {
        Cue::Single
    } else {
        Cue::Unknown
    }
}

const COUNT_WORDS: &[&str] = &[
    "two", "three", "four", "five", "six", "seven", "eight", "nine", "ten",
    "eleven", "twelve", "many", "several",
];

pub(super) fn depluralise(word: &str) -> String {
    if word.ends_with("ies") && word.len() > 3 {
        return format!("{}y", &word[..word.len() - 3]);
    }
    for (plural, singular) in IRREGULAR_PLURALS {
        if word == *plural {
            return (*singular).to_string();
        }
    }
    if word.ends_with("ses") || word.ends_with("xes") || word.ends_with("zes") {
        return word[..word.len() - 2].to_string();
    }
    if word.ends_with('s') && !word.ends_with("ss") && word.len() > 2 {
        return word[..word.len() - 1].to_string();
    }
    word.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::prepared::{ArtifactKind, RolePlayerSpec};

    fn scenario(when: &str, given: &str, then: &str) -> ScenarioSpec {
        ScenarioSpec {
            name: "test".into(),
            given: given.into(),
            when: when.into(),
            then: then.into(),
        }
    }

    #[test]
    fn derive_multiplicity_list_from_plural_name_and_multiple_instances() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            "projections/transactions.md".into(),
            "TransactionList".into(),
            "TransactionList".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "transactions".into(),
            data_type: "Transaction".into(),
            responsibility: "the transactions to filter".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "filtered is queried",
            "five transactions spanning the last week",
            "all five transactions are returned",
        )];
        derive_multiplicity(&mut a);

        assert_eq!(a.role_players[0].multiplicity, Multiplicity::List);
        // No disagreement finding because name and scenarios agree.
        assert!(a.ambiguities.is_empty(), "no findings expected, got {:?}", a.ambiguities);
    }

    #[test]
    fn derive_multiplicity_single_from_singular_name_and_one_instance() {
        let mut a = PreparedArtifact::new(
            ArtifactKind::Context,
            "contexts/auth.md".into(),
            "Auth".into(),
            "Auth".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "session".into(),
            data_type: "Session".into(),
            responsibility: "the active session".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "authenticate is performed",
            "a Session with chat state \"idle\"",
            "the session is authenticated",
        )];
        derive_multiplicity(&mut a);

        assert_eq!(a.role_players[0].multiplicity, Multiplicity::Single);
    }

    #[test]
    fn derive_multiplicity_disagreement_emits_finding() {
        // Plural-named role player but every scenario supplies a single
        // instance — surface a finding so the BA clarifies.
        let mut a = PreparedArtifact::new(
            ArtifactKind::Projection,
            "projections/odd.md".into(),
            "Odd".into(),
            "Odd".into(),
        );
        a.role_players.push(RolePlayerSpec {
            name: "transactions".into(),
            data_type: "Transaction".into(),
            responsibility: "the transactions".into(),
            ..Default::default()
        });
        a.scenarios = vec![scenario(
            "queried",
            "a transaction posted yesterday",
            "the transaction is returned",
        )];
        derive_multiplicity(&mut a);

        let finding = a
            .ambiguities
            .iter()
            .find(|x| x.message.contains("transactions") && x.message.contains("conflicting"));
        assert!(finding.is_some(), "expected disagreement finding, got {:?}", a.ambiguities);
    }
}
