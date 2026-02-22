use crate::data::{Currency, Ledger, LedgerEntry};
use anyhow::{anyhow, Result};

/// Account represents a view over a Ledger for a specific account id.
#[derive(Debug, Clone)]
pub struct Account {
    account_id: i32,
    ledger: Ledger,
}

impl Account {
    /// new
    /// - Accepts an account id and the Ledger.
    /// - At least one entry for the account must exist on the Ledger; if not, an Error is returned.
    /// - If at least one ledger entry exists, returns an account object.
    pub fn new(account_id: i32, ledger: Ledger) -> Result<Self> {
        tracing::info!("[Account] new, account_id={}", account_id);

        if account_id <= 0 {
            tracing::warn!("[Account] new, invalid account_id (must be positive): {}", account_id);
            return Err(anyhow!("account_id must be a positive integer"));
        }

        let entries = ledger.get_entries_for(account_id);
        if entries.is_empty() {
            tracing::error!(
                "[Account] new, no entries found for account_id={}",
                account_id
            );
            return Err(anyhow!(
                "no ledger entries exist for account_id={}",
                account_id
            ));
        }

        Ok(Self { account_id, ledger })
    }

    /// account_id
    /// - Returns the id of the account.
    pub fn account_id(&self) -> i32 {
        tracing::info!("[Account] account_id");
        self.account_id
    }

    /// transactions
    /// - Returns all ledger entries related to the account (entries where the account_id is sink or source),
    ///   sorted by transaction date, descending.
    pub fn transactions(&self) -> Vec<LedgerEntry> {
        tracing::info!(
            "[Account] transactions, account_id={}",
            self.account_id
        );

        let mut entries = self.ledger.get_entries_for(self.account_id);
        // get_entries_for returns ascending by timestamp; reverse to get descending.
        entries.reverse();
        entries
    }

    /// currency
    /// - All ledger entries for an account must be in the same currency.
    /// - The value is either None or the currency of previous ledger entries where the account is source or sink.
    pub fn currency(&self) -> Option<Currency> {
        tracing::info!(
            "[Account] currency, account_id={}",
            self.account_id
        );

        let entries = self.ledger.get_entries_for(self.account_id);
        // new() guarantees at least one entry exists for valid Account; returning Option to comply with spec wording.
        entries.first().map(|e| e.currency())
    }

    /// balance
    /// - The balance of an account is a calculated property.
    /// - Definition: sum of the amounts of all transactions on the Ledger where the account_id is the sink
    ///   minus the sum of the amounts of all transactions on the Ledger where the account_id is the source.
    ///
    /// NOTE: This implementation cannot complete as specified because the LedgerEntry's amount value
    /// is not externally accessible by the current public API of LedgerEntry/amount. Without an accessor
    /// to retrieve the numeric value in minor units, we cannot perform the required arithmetic.
    /// As such, this method returns an error indicating the missing dependency in the specification.
    pub fn balance(&self) -> Result<i128> {
        tracing::info!(
            "[Account] balance, account_id={}",
            self.account_id
        );

        tracing::error!("[Account] balance, cannot compute: missing public accessor to retrieve numeric amount from LedgerEntry/amount");
        Err(anyhow!(
            "Cannot compute balance: missing public accessor to retrieve numeric amount from LedgerEntry/amount"
        ))
    }
}