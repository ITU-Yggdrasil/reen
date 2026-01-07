# Specification Compliance Guide

This document explains how to verify that implementations strictly follow their specifications.

## Overview

Reen uses a **strict specification-first approach**. Implementations must follow specifications **exactly** with no deviations.

## Specification Structure

Each specification has these sections:

### 1. Role Players
Objects passed to the constructor that the context operates on.

```markdown
## Role Players

### role_name
**Type**: `SomeType`
**Description**: What this role player represents
```

**Implementation Rule**: Each role player becomes a struct field with the exact type specified.

### 2. Props
Values passed to the constructor.

```markdown
## Props

- **prop_name**: `PropType` - Description of the prop
```

**Implementation Rule**: Each prop becomes a struct field with the exact type specified.

### 3. Role Methods
Private methods that operate on role players using props.

```markdown
## Role Methods

### role_player.method_name
**Signature**: `fn method_name(this: RolePlayerType, ...) -> Result<...>`
**Description**: What this method does
```

**Implementation Rule**:
- These become **private methods** on the struct
- Signatures must match exactly
- These are the only private methods allowed
- No helper methods, no utilities

### 4. Functionality
Public methods available on the context.

```markdown
## Functionality

### method_name
**Signature**: `fn method_name(self) -> Result<...>`
**Description**: What this public method does
```

**Implementation Rule**:
- These become **public methods** on the struct
- Signatures must match exactly
- These are the only public methods allowed
- No convenience methods, no shortcuts

## Compliance Rules

### ✅ MUST DO

1. **Struct fields = Role Players + Props**
   - Every role player → struct field
   - Every prop → struct field
   - Nothing else

2. **Public methods = Functionality section**
   - Every function in Functionality → public method
   - Match signatures exactly
   - Nothing else

3. **Private methods = Role Methods section**
   - Every role method → private method
   - Match signatures exactly
   - Nothing else

4. **Follow interaction patterns**
   - Role methods use the specified props
   - Public methods call role methods as described
   - No shortcuts or alternative paths

### ❌ MUST NOT DO

1. **No extra fields**
   - No caches
   - No helper data
   - No temporary storage
   - If it's not in Role Players or Props, it doesn't exist

2. **No extra public methods**
   - No convenience methods
   - No shortcuts
   - No helpers
   - Only what's in Functionality

3. **No extra private methods**
   - No helper functions
   - No utilities
   - No validation methods
   - Only what's in Role Methods

4. **No deviations from signatures**
   - Parameter types must match exactly
   - Return types must match exactly
   - Method names must match exactly

## Verification Checklist

### Before Submitting Implementation

Run through this checklist:

#### 1. Struct Fields Audit
```bash
# Count struct fields
grep -c "^\s*[a-z_]*:" src/contexts/your_context.rs

# Compare with spec
# Count in "Role Players" + count in "Props"
# Numbers must match exactly
```

**Questions to ask**:
- [ ] Is every role player a field?
- [ ] Is every prop a field?
- [ ] Are there any extra fields?

#### 2. Public Methods Audit
```bash
# List all public methods
grep "pub fn" src/contexts/your_context.rs
```

**Questions to ask**:
- [ ] Is every function in "Functionality" implemented?
- [ ] Are all public methods listed in "Functionality"?
- [ ] Are there any extra public methods?

#### 3. Private Methods Audit
```bash
# List all private methods (non-pub fn)
grep -E "^\s+fn [a-z_]+" src/contexts/your_context.rs | grep -v "pub fn"
```

**Questions to ask**:
- [ ] Is every role method implemented?
- [ ] Are all private methods listed in "Role Methods"?
- [ ] Are there any helper methods?

#### 4. Signature Verification

For each method, verify:
- [ ] Parameter names match spec (or are reasonable equivalents)
- [ ] Parameter types match spec exactly
- [ ] Return type matches spec exactly
- [ ] Method name matches spec exactly

## Examples

### Example Specification

```markdown
## Role Players

### agent
**Type**: `String`
**Description**: The agent name

## Props

- **input**: `T` - Generic input data
- **registry**: `Registry` - The agent registry

## Role Methods

### agent.execute
**Signature**: `fn execute(this: String, registry: Registry) -> Result<Output>`

## Functionality

### run
**Signature**: `fn run(self) -> Result<Output>`
```

### ✅ CORRECT Implementation

```rust
pub struct AgentContext<T> {
    // Role players
    agent: String,

    // Props
    input: T,
    registry: Registry,
}

impl<T> AgentContext<T> {
    // Public method from Functionality
    pub fn run(self) -> Result<Output> {
        self.execute()
    }

    // Private method from Role Methods
    fn execute(&self) -> Result<Output> {
        // Implementation
    }
}
```

**Why this is correct**:
- ✓ Has exactly 3 fields (1 role player + 2 props)
- ✓ Has exactly 1 public method (from Functionality)
- ✓ Has exactly 1 private method (from Role Methods)
- ✓ Signatures match exactly

### ❌ WRONG Implementation

```rust
pub struct AgentContext<T> {
    agent: String,
    input: T,
    registry: Registry,
    cache: Cache,           // ✗ NOT in spec
}

impl<T> AgentContext<T> {
    pub fn run(self) -> Result<Output> {
        self.execute()
    }

    pub fn quick_run(&self) -> Result<Output> {  // ✗ NOT in Functionality
        self.execute()
    }

    fn execute(&self) -> Result<Output> {
        // Implementation
    }

    fn validate(&self) -> bool {  // ✗ NOT in Role Methods
        true
    }

    fn helper(&self) -> String {  // ✗ NOT in Role Methods
        "helper".to_string()
    }
}
```

**Why this is wrong**:
- ✗ Has 4 fields instead of 3 (extra `cache`)
- ✗ Has 2 public methods instead of 1 (extra `quick_run`)
- ✗ Has 3 private methods instead of 1 (extra `validate` and `helper`)

## Common Violations

### 1. "But I need a helper method!"

**Wrong thinking**: "The specification doesn't say I can't add helper methods."

**Correct thinking**: "The specification doesn't say I CAN add helper methods."

**Solution**: Inline the logic or refactor your role methods to not need helpers.

### 2. "But this would be more convenient!"

**Wrong**: Adding a `quick_run()` method for convenience.

**Correct**: Users call the specified `run()` method. If they want convenience, they can write wrappers.

### 3. "But I need to cache this!"

**Wrong**: Adding a `cache` field because you think you need it.

**Correct**: If caching is needed, it would be in the specification. If it's not specified, don't cache.

### 4. "The spec doesn't mention validation!"

**Wrong**: Adding a `validate()` method.

**Correct**: Validation happens inside the specified methods. If a method needs to validate, it does so inline.

## Enforcement

The `create_implementation` agent has been updated to strictly enforce these rules. It will:

1. Parse the specification to identify all required elements
2. Implement exactly those elements
3. Verify compliance before completing
4. Refuse to add anything not in the specification

## When Specifications Are Incomplete or Wrong

### Agent Behavior

The `create_implementation` agent will **fail immediately** if it determines the specification is insufficient:

**The agent will stop and report an error if**:
- It needs a helper method but none is specified
- The specified methods are insufficient for the requirements
- The specification describes behavior requiring unspecified methods
- The type system won't allow the specified implementation

**Example failure message**:
```
ERROR: Cannot implement specification as written.

Problem: The 'run' method in "Functionality" is documented to perform
input validation, but no 'validate' role method exists in "Role Methods".

Required: Either:
- Add 'validate' role method to handle validation
- Remove validation requirement from 'run' documentation
- Clarify how validation should happen within existing methods

I cannot proceed without specification clarification.
```

### What NOT to Do

**Do NOT**:
- Try to implement anyway with workarounds
- Add helper methods "just this once"
- Compromise on the specification
- Fix it in the implementation

### What TO Do

If a specification is incomplete or wrong:

1. **Stop implementation** (agent will fail)
2. **Update the draft** to include missing methods or clarify behavior
3. **Regenerate the specification**: `reen create specification`
4. **Regenerate the implementation**: `reen create implementation`

The specification is the source of truth. If it's wrong, fix the specification first, not the implementation.

## Benefits of Strict Compliance

1. **Predictability**: Implementations always match specifications
2. **Maintainability**: No hidden complexity or undocumented features
3. **Verifiability**: Easy to check if implementation matches spec
4. **Clarity**: Clear separation between public API and internal logic
5. **Consistency**: All contexts follow the same patterns

## Summary

**Golden Rule**: If it's not in the specification, it doesn't exist in the implementation.

- Role Players + Props → Struct fields
- Functionality → Public methods
- Role Methods → Private methods
- Nothing else

Zero tolerance for deviations. Zero exceptions.
