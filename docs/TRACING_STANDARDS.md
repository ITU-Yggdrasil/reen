# Tracing Standards for Reen Implementations

This document defines the required tracing instrumentation for all context implementations.

## Overview

All generated code MUST include tracing instrumentation for observability and debugging. The tracing format follows a strict convention that reflects the DCI (Data, Context, Interaction) architecture.

## Tracing Format

### Format Rules

1. **Role Methods (private methods)**:
   ```
   "[ContextName] [role_name] [role_method], message"
   ```

2. **Functionality Methods (public methods)**:
   ```
   "[ContextName] [method_name], message"
   ```

### Naming Conventions

- **ContextName**: PascalCase (matches the struct name)
- **role_name**: snake_case (matches the role player field name)
- **role_method**: snake_case (matches the method name)
- **method_name**: snake_case (matches the method name)

## Examples

### Example 1: MoneyTransfer Context

```rust
pub struct MoneyTransfer {
    source: String,      // Role player
    sink: String,        // Role player
    amount: f64,
    ledger: Ledger,
}

impl MoneyTransfer {
    // Public method (from Functionality section)
    pub fn transfer(self) -> Result<Ledger> {
        tracing::info!(
            "[MoneyTransfer] transfer, source={}, sink={}, amount={}",
            self.source, self.sink, self.amount
        );

        let entry = self.withdraw()?;
        let settled = self.deposit(entry)?;
        let new_ledger = self.ledger.add_entry(settled);

        tracing::debug!("[MoneyTransfer] transfer, completed");
        Ok(new_ledger)
    }

    // Private role method (source role)
    fn withdraw(&self) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] source withdraw, account={}, amount={}",
            self.source, self.amount
        );

        // Create unsettled ledger entry
        let entry = LedgerEntry::source(
            self.source.clone(),
            self.amount,
            self.currency,
            Utc::now()
        );

        tracing::debug!("[MoneyTransfer] source withdraw, entry created");
        Ok(entry)
    }

    // Private role method (sink role)
    fn deposit(&self, entry: LedgerEntry) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] sink deposit, account={}",
            self.sink
        );

        // Settle the entry
        let settled = entry.settle(self.sink.clone())
            .ok_or_else(|| {
                tracing::error!("[MoneyTransfer] sink deposit, entry already settled");
                TransferError::EntryAlreadySettled
            })?;

        tracing::debug!("[MoneyTransfer] sink deposit, entry settled");
        Ok(settled)
    }
}
```

### Example 2: Account Context

```rust
pub struct Account {
    account_id: String,  // Role player
    ledger: Ledger,      // Role player
}

impl Account {
    // Public method (from Functionality)
    pub fn balance(&self) -> f64 {
        tracing::info!(
            "[Account] balance, account_id={}",
            self.account_id
        );

        let balance = self.calculate_balance();

        tracing::debug!(
            "[Account] balance, calculated={}",
            balance
        );

        balance
    }

    // Private role method (ledger role)
    fn calculate_balance(&self) -> f64 {
        tracing::debug!(
            "[Account] ledger calculate_balance, account_id={}",
            self.account_id
        );

        let credits: f64 = self.ledger
            .get_entries()
            .iter()
            .filter(|e| e.get_sink_account_id() == &AccountId::Settled(self.account_id.clone()))
            .map(|e| e.get_amount())
            .sum();

        let debits: f64 = self.ledger
            .get_entries()
            .iter()
            .filter(|e| e.get_source_account_id() == &AccountId::Settled(self.account_id.clone()))
            .map(|e| e.get_amount())
            .sum();

        let balance = credits - debits;

        tracing::debug!(
            "[Account] ledger calculate_balance, credits={}, debits={}, balance={}",
            credits, debits, balance
        );

        balance
    }
}
```

## Tracing Levels

### When to Use Each Level

- **`tracing::info!()`**:
  - Entry to all public methods (Functionality)
  - Major state changes
  - Successful completion of operations

- **`tracing::debug!()`**:
  - Entry to all role methods
  - Intermediate steps in calculations
  - Internal state changes
  - Method completion (success path)

- **`tracing::warn!()`**:
  - Recoverable errors
  - Validation failures that don't abort
  - Deprecated code paths
  - Performance issues

- **`tracing::error!()`**:
  - Just before returning `Err(...)`
  - Unrecoverable errors
  - Invariant violations
  - Critical failures

## Message Format

### Good Messages

Include relevant context in the message:

```rust
// Good - includes key values
tracing::info!(
    "[MoneyTransfer] transfer, source={}, sink={}, amount={}",
    self.source, self.sink, self.amount
);

// Good - shows progress
tracing::debug!("[Account] ledger calculate_balance, balance calculated={}", balance);

// Good - error context
tracing::error!(
    "[MoneyTransfer] source withdraw, insufficient funds: available={}, needed={}",
    available, self.amount
);
```

### Bad Messages

Avoid these patterns:

```rust
// Bad - no context
tracing::info!("transfer");

// Bad - missing role/method
tracing::debug!("calculating balance");

// Bad - wrong format
tracing::info!("MoneyTransfer::transfer starting");
```

## Required Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
tracing = "0.1"
```

For applications (not libraries), also add a subscriber:

```toml
[dependencies]
tracing-subscriber = "0.3"
```

## Initialization (for applications)

In `main.rs` or application entry point:

```rust
use tracing_subscriber;

fn main() {
    // Initialize tracing
    tracing_subscriber::fmt::init();

    // Rest of application
}
```

## Verification Checklist

Before submitting implementation:

- [ ] Every public method has `tracing::info!()` at entry
- [ ] Every role method has `tracing::debug!()` at entry
- [ ] Format is `"[ContextName] [role] [method]"` for role methods
- [ ] Format is `"[ContextName] [method]"` for public methods
- [ ] Important values are logged (IDs, amounts, states)
- [ ] Errors have `tracing::error!()` before `Err(...)` return
- [ ] `tracing` dependency is in `Cargo.toml`

## Common Patterns

### Pattern 1: Entry and Exit Logging

```rust
pub fn transfer(self) -> Result<Ledger> {
    tracing::info!("[MoneyTransfer] transfer, entering");

    // Do work
    let result = self.do_transfer()?;

    tracing::info!("[MoneyTransfer] transfer, completed");
    Ok(result)
}
```

### Pattern 2: Error Logging

```rust
fn withdraw(&self) -> Result<LedgerEntry> {
    tracing::debug!("[MoneyTransfer] source withdraw, starting");

    if self.amount <= 0.0 {
        tracing::error!(
            "[MoneyTransfer] source withdraw, invalid amount={}",
            self.amount
        );
        return Err(TransferError::InvalidAmount);
    }

    // Continue...
}
```

### Pattern 3: State Logging

```rust
fn calculate_balance(&self) -> f64 {
    tracing::debug!("[Account] ledger calculate_balance, starting");

    let balance = /* calculation */;

    tracing::debug!(
        "[Account] ledger calculate_balance, result={}",
        balance
    );

    balance
}
```

## Reading the Traces

When running with tracing enabled, you'll see output like:

```
INFO  [MoneyTransfer] transfer, source=acc_123, sink=acc_456, amount=100.50
DEBUG [MoneyTransfer] source withdraw, account=acc_123, amount=100.50
DEBUG [MoneyTransfer] source withdraw, entry created
DEBUG [MoneyTransfer] sink deposit, account=acc_456
DEBUG [MoneyTransfer] sink deposit, entry settled
DEBUG [MoneyTransfer] transfer, completed
```

This clearly shows:
- The context being executed (`MoneyTransfer`)
- The role involved (`source`, `sink`)
- The method being called (`withdraw`, `deposit`, `transfer`)
- Key values at each step

## Benefits

1. **Observability**: See exactly what's happening at runtime
2. **Debugging**: Trace execution flow through role interactions
3. **Performance**: Identify slow operations
4. **Auditing**: Track state changes and operations
5. **Testing**: Verify correct method call sequences

## Summary

**Format Templates**:

- Public method: `tracing::info!("[ContextName] method_name, ...")`
- Role method: `tracing::debug!("[ContextName] role method_name, ...")`
- Error: `tracing::error!("[ContextName] role method_name, error details")`

**Golden Rule**: Every method gets at least one trace statement at entry with the proper format.
