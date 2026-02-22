use anyhow::{anyhow, Result};
use tracing;

use crate::data::currency::Currency;
use crate::data::ledgerentry::LedgerEntry;
use crate::data::Ledger;

/// An immutable account view over a ledger
#[derive(Debug, Clone)]
pub struct Account {
    account_id: i32,
    ledger: Ledger,
}

impl Account {
    /// Creates a new Account bound to the provided ledger and account id.
    ///
    /// Rules:
    /// - account_id must be positive
    /// - At least one entry for the account must exist on the Ledger
    /// - All entries for the account must share the same currency
    pub fn new(account_id: i32, ledger: Ledger) -> Result<Self> {
        tracing::info!("[Account] new, account_id={}", account_id);

        if account_id <= 0 {
            tracing::error!("[Account] new, invalid account_id={}", account_id);
            return Err(anyhow!("account_id must be a positive integer"));
        }

        let entries = ledger.get_entries_for(account_id);
        if entries.is_empty() {
            tracing::error!(
                "[Account] new, no ledger entries found for account_id={}",
                account_id
            );
            return Err(anyhow!(
                "no ledger entries exist on the ledger for account_id={}",
                account_id
            ));
        }

        // Enforce currency immutability/consistency across all entries for this account
        let base_currency = entries[0].currency();
        for e in entries.iter().skip(1) {
            let c = e.currency();
            if c != base_currency {
                tracing::error!(
                    "[Account] new, currency mismatch detected for account_id={}, expected={:?}, found={:?}",
                    account_id,
                    base_currency,
                    c
                );
                return Err(anyhow!(
                    "currency mismatch for account_id={}: expected {:?}, found {:?}",
                    account_id,
                    base_currency,
                    c
                ));
            }
        }

        Ok(Self { account_id, ledger })
    }

    /// Returns the id of the account.
    pub fn account_id(&self) -> i32 {
        tracing::info!("[Account] account_id, account_id={}", self.account_id);
        self.account_id
    }

    /// Returns all ledger entries related to the account (source or sink),
    /// sorted by transaction date descending.
    pub fn transactions(&self) -> Vec<LedgerEntry> {
        tracing::info!(
            "[Account] transactions, account_id={}",
            self.account_id
        );

        let mut entries = self.ledger.get_entries_for(self.account_id);
        // get_entries_for returns ascending by timestamp; reverse to get descending
        entries.reverse();
        entries
    }

    /// The currency of the account. Either None (no entries) or Some(currency).
    /// Since construction requires at least one entry, this will typically be Some(...).
    pub fn currency(&self) -> Option<Currency> {
        tracing::info!(
            "[Account] currency, account_id={}",
            self.account_id
        );

        let entries = self.ledger.get_entries_for(self.account_id);
        entries.first().map(|e| e.currency())
    }

    /// The balance of the account computed as:
    /// sum(amounts where account is sink) - sum(amounts where account is source)
    ///
    /// Returned as a signed integer count of minor units.
    pub fn balance(&self) -> i128 {
        tracing::info!(
            "[Account] balance, account_id={}",
            self.account_id
        );

        let mut sum: i128 = 0;
        let entries = self.ledger.get_entries_for(self.account_id);

        for e in entries.iter() {
            // Reconstruct total minor units from amount.major() and amount.minor()
            let units: i128 = (e.amount.major() as i128) * 100 + (e.amount.minor() as i128);

            // Add if this account is the sink, subtract if it is the source
            if matches!(e.sink, Some(id) if id == self.account_id) {
                sum += units;
            }
            if matches!(e.source, Some(id) if id == self.account_id) {
                sum -= units;
            }
        }

        sum
    }
}