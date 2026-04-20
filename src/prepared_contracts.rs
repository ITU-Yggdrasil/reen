use crate::prepared::{Ambiguity, PreparedArtifact};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractIssue {
    pub artifact_index: usize,
    pub ambiguity: Ambiguity,
}

/// Validate cross-artifact contracts.
///
/// Phase 1 stub: returns an empty list. Full contract validation is reimplemented in Phase 2
/// alongside the verifier.
pub fn collect_contract_issues(_artifacts: &[PreparedArtifact]) -> Vec<ContractIssue> {
    Vec::new()
}
