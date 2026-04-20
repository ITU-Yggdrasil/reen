# Reen Draft Format Reference

Three draft kinds. No types, no signatures, no Rust tokens. DCI terms only.

## Context (`drafts/contexts/<name>.md`)

```markdown
# Context: <Name>

## Purpose
One paragraph: what scenario this context enacts, in business terms.

## Role Players
- <role>: <data or projection name> — one-line responsibility

## Props
- <prop>: <description> (scalar configuration only; never a collaborator)

## Role Methods
### <role>
- <method name>: <what this role does in this enactment>

## Functionalities
### <functionality name>
Prose describing which roles do what, in what order, referencing role methods by name.

## Scenarios
### <scenario name>
Given: <role players in specific states>
When: <functionality invocation with concrete values>
Then: <observable outcome>
```

## Data (`drafts/data/<name>.md`)

```markdown
# Data: <Name>

## Purpose
One paragraph: what domain concept this represents.

## Attributes
- <name>: <prose description>

## Invariants
- <prose invariant>

## Operations
### <operation name>
Prose describing what this operation does as a business rule.

## Scenarios
### <scenario name>
Given: <data value>
When: <operation invoked>
Then: <resulting value>
```

## Projection (`drafts/projections/<name>.md`)

```markdown
# Projection: <Name>

## Purpose
One paragraph: what view this provides over underlying data.

## Source
- <data name>

## Queries
### <query name>
Prose describing what is observable through this projection.

## Scenarios
### <scenario name>
Given: <underlying data state>
When: <query invoked>
Then: <returned view>
```

Projections are read-only. The parser rejects mutation-shaped prose.
