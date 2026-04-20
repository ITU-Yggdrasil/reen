---
name: audit
description: Run post-implementation discrepancy audit between draft spec and Rust code
---

Audit the implementation against the draft specification to find discrepancies.

Steps:
1. Identify the context/data/projection to audit (from argument or ask the user).
2. Read the draft markdown, prepared YAML, and implementation `.rs` file.
3. Check that every role method's responsibility is observable in the corresponding function body.
4. Check that every functionality's enactment matches its prose description.
5. Check that every invariant from the draft is enforced in the code.
6. Report discrepancies with:
   - Kind: `role_method_responsibility_missing`, `functionality_divergent_flow`, `invariant_not_enforced`, `code_not_in_spec`
   - Spec citation and code citation (file:line)
   - Severity: high (blocking) or low (informational)
7. Do not auto-apply changes. Present the report for the user to triage.
