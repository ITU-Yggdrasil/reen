# Rust Module Organization

This document explains how to properly organize Rust modules when implementing specifications.

## The Problem

When you create implementation files in subdirectories like `src/contexts/`, Rust needs to know about them. Without proper module declaration, you'll get:

```
error[E0583]: file not found for module `contexts`
 --> src/lib.rs:2:1
  |
2 | pub mod contexts;
  | ^^^^^^^^^^^^^^^^^
```

## The Solution: mod.rs Files

### Rule

**When you create files in a subdirectory, you MUST create a `mod.rs` file in that directory.**

### Example Structure

```
src/
├── lib.rs
├── contexts/
│   ├── mod.rs              ← REQUIRED!
│   ├── account.rs
│   └── money_transfer.rs
└── types/
    ├── mod.rs              ← REQUIRED!
    └── ledger.rs
```

### Contents of mod.rs

The `mod.rs` file has two jobs:

1. **Declare modules** - Tell Rust about the `.rs` files in this directory
2. **Re-export types** - Make public types available to parent modules

#### Example: src/contexts/mod.rs

```rust
// 1. Declare modules (one for each .rs file)
mod account;
mod money_transfer;

// 2. Re-export public types
pub use account::Account;
pub use money_transfer::{MoneyTransfer, TransferError};
```

#### Example: src/types/mod.rs

```rust
mod ledger;
mod currency;

pub use ledger::{Ledger, LedgerEntry};
pub use currency::Currency;
```

## Step-by-Step Guide

### When Creating First Implementation File

1. **Create the directory**:
   ```bash
   mkdir -p src/contexts
   ```

2. **Create your implementation**:
   ```bash
   # Create src/contexts/account.rs with Account struct
   ```

3. **Create mod.rs**:
   ```rust
   // src/contexts/mod.rs
   mod account;
   pub use account::Account;
   ```

4. **Verify it works**:
   ```bash
   cargo build
   ```

### When Adding More Implementation Files

1. **Create the new file**:
   ```bash
   # Create src/contexts/money_transfer.rs
   ```

2. **Update mod.rs**:
   ```rust
   // src/contexts/mod.rs
   mod account;
   mod money_transfer;  // ← Add this

   pub use account::Account;
   pub use money_transfer::MoneyTransfer;  // ← Add this
   ```

3. **Verify**:
   ```bash
   cargo build
   ```

## Common Patterns

### Pattern 1: Single Export

If a module has one main type:

```rust
// src/contexts/mod.rs
mod account;
pub use account::Account;
```

### Pattern 2: Multiple Exports

If a module exports several types:

```rust
// src/contexts/mod.rs
mod money_transfer;
pub use money_transfer::{MoneyTransfer, TransferError, TransferResult};
```

### Pattern 3: Selective Export

Only export what should be public:

```rust
// src/contexts/mod.rs
mod account;

// Export only Account, not internal types
pub use account::Account;
// account::AccountInternal stays private
```

## What Goes in Each File

### src/lib.rs

```rust
// Declare top-level modules
pub mod contexts;
pub mod types;

// Optional: re-export commonly used items
pub use contexts::{Account, MoneyTransfer};
pub use types::Ledger;
```

### src/contexts/mod.rs

```rust
// Declare all context modules
mod account;
mod money_transfer;

// Re-export public contexts
pub use account::Account;
pub use money_transfer::MoneyTransfer;
```

### src/contexts/account.rs

```rust
// The actual implementation
pub struct Account {
    // ...
}

impl Account {
    pub fn new() -> Self { /* ... */ }
}
```

## Troubleshooting

### Error: "file not found for module"

**Problem**: Missing `mod.rs` file

**Solution**: Create `src/[directory]/mod.rs` and add module declarations

### Error: "unresolved import"

**Problem**: Type not re-exported in `mod.rs`

**Solution**: Add `pub use module_name::TypeName;` to `mod.rs`

### Error: "private type in public interface"

**Problem**: Internal type leaked through public API

**Solution**: Either make the type public or don't expose it in public signatures

## Checklist for Implementation Agent

Before completing implementation:

- [ ] Created `src/contexts/mod.rs` if implementing contexts
- [ ] Added `mod context_name;` for each implemented context
- [ ] Added `pub use context_name::ContextType;` for each public type
- [ ] Verified `cargo build` succeeds
- [ ] No "file not found for module" errors
- [ ] All public types are accessible

## Why This Matters

1. **Compilation**: Code won't compile without proper module organization
2. **Visibility**: Types won't be accessible without re-exports
3. **Convention**: This is standard Rust module organization
4. **Maintenance**: Clear structure makes code easier to navigate

## Summary

**Simple Rule**: Creating files in `src/subdirectory/`? Create `src/subdirectory/mod.rs` first!

**mod.rs Template**:
```rust
// Declare modules
mod my_module;

// Re-export public types
pub use my_module::MyType;
```

Follow this pattern consistently and you'll never have module organization issues.
