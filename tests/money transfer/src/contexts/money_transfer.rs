use anyhow::{bail, Result};
use tracing;

use crate::contexts::Account;
use crate::types::{Amount, Ledger, LedgerEntry};

#[cfg(feature = "money_transfer")]
pub struct MoneyTransfer {
    // Role Players
    source: Account,
    sink: Account,

    // Props
    amount: Amount,
    ledger: Ledger,
}

#[cfg(feature = "money_transfer")]
impl MoneyTransfer {
    /// Constructs a new MoneyTransfer context.
    /// Business rules:
    /// - The currency of the amount must match the currency of the source.
    pub fn new(sink_account_id: i32, source_account_id: i32, amount: Amount, ledger: Ledger) -> Result<Self> {
        tracing::info!(
            "[MoneyTransfer] new, sink={}, source={}, amount={}",
            sink_account_id,
            source_account_id,
            amount.to_str()
        );

        // Construct accounts
        let source = Account::new(source_account_id, ledger.clone()).map_err(|e| {
            tracing::error!(
                "[MoneyTransfer] new, failed to construct source account, source_id={}, error={}",
                source_account_id,
                e
            );
            e
        })?;

        let sink = Account::new(sink_account_id, ledger.clone()).map_err(|e| {
            tracing::error!(
                "[MoneyTransfer] new, failed to construct sink account, sink_id={}, error={}",
                sink_account_id,
                e
            );
            e
        })?;

        // Validate business rule: currency of amount must match currency of source
        let source_currency = source.currency().ok_or_else(|| {
            let msg = "source account has undefined currency";
            tracing::error!("[MoneyTransfer] new, {}", msg);
            anyhow::anyhow!(msg)
        })?;
        let source_currency_code = source_currency.to_str();

        // Extract currency code from Amount via its string representation "{major}.{minor} CODE"
        let amount_currency_code = match amount.to_str().rsplit_once(' ') {
            Some((_, code)) => code.to_string(),
            None => {
                let msg = "failed to parse currency from amount";
                tracing::error!("[MoneyTransfer] new, {}", msg);
                bail!(msg)
            }
        };

        if source_currency_code != amount_currency_code {
            let msg = format!(
                "currency mismatch: source={}, amount={}",
                source_currency_code, amount_currency_code
            );
            tracing::error!("[MoneyTransfer] new, {}", msg);
            bail!(msg);
        }

        Ok(Self {
            source,
            sink,
            amount,
            ledger,
        })
    }

    /// Executes the transfer: withdraws from source, deposits to sink, and adds the entry to the ledger.
    pub fn Transfer(&self) -> Result<Ledger> {
        tracing::info!(
            "[MoneyTransfer] transfer, source={}, sink={}, amount={}",
            self.source.account_id(),
            self.sink.account_id(),
            self.amount.to_str()
        );

        let entry = self.withdraw()?;
        let settled = self.deposit(entry)?;
        let new_ledger = self.ledger.add_entry(settled).map_err(|e| {
            tracing::error!(
                "[MoneyTransfer] transfer, failed to add entry to ledger, error={}",
                e
            );
            e
        })?;

        tracing::info!(
            "[MoneyTransfer] transfer, completed, source={}, sink={}, amount={}",
            self.source.account_id(),
            self.sink.account_id(),
            self.amount.to_str()
        );

        Ok(new_ledger)
    }

    // Role Methods (private)

    fn withdraw(&self) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] source withdraw, source={}, amount={}",
            self.source.account_id(),
            self.amount.to_str()
        );

        // Business rule: The transferred amount can't exceed the present balance of the source account.
        let balance = self.source.balance();
        if !Self::amount_lte(&self.amount, &balance) {
            let msg = format!(
                "insufficient funds: requested {}, available {}",
                self.amount.to_str(),
                balance.to_str()
            );
            tracing::warn!("[MoneyTransfer] source withdraw, {}", msg);
            bail!(msg);
        }

        // Create withdrawal entry (sink None, source Some)
        let entry = self
            .ledger
            .create_entry(Some(self.source.account_id()), self.amount.clone())
            .map_err(|e| {
                tracing::error!(
                    "[MoneyTransfer] source withdraw, failed to create ledger entry, error={}",
                    e
                );
                e
            })?;

        tracing::debug!(
            "[MoneyTransfer] source withdraw, entry created, source={}, amount={}",
            self.source.account_id(),
            self.amount.to_str()
        );

        Ok(entry)
    }

    fn deposit(&self, entry: LedgerEntry) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] sink deposit, sink={}, amount={}",
            self.sink.account_id(),
            self.amount.to_str()
        );

        let settled = self.ledger.settle(&entry, self.sink.account_id()).map_err(|e| {
            tracing::error!(
                "[MoneyTransfer] sink deposit, failed to settle ledger entry, error={}",
                e
            );
            e
        })?;

        tracing::debug!(
            "[MoneyTransfer] sink deposit, entry settled, sink={}, amount={}",
            self.sink.account_id(),
            self.amount.to_str()
        );

        Ok(settled)
    }

    // Helpers (private, internal-only)

    fn amount_lte(a: &Amount, b: &Amount) -> bool {
        let (a_major, a_minor) = (a.major(), a.minor());
        let (b_major, b_minor) = (b.major(), b.minor());

        if a_major < b_major {
            true
        } else if a_major > b_major {
            false
        } else {
            a_minor <= b_minor
        }
    }
}