use anyhow::{anyhow, Result};
use tracing;

use crate::contexts::Account;
use crate::data::{Amount, Ledger, LedgerEntry};


pub struct MoneyTransfer {
    // Role players
    source: Account,
    sink: Account,

    // Props
    amount: Amount,
    ledger: Ledger,
}


impl MoneyTransfer {
    pub fn new(sink_account_id: i32, source_account_id: i32, amount: Amount, ledger: Ledger) -> Result<Self> {
        tracing::info!(
            "[MoneyTransfer] new, sink_account_id={}, source_account_id={}, amount={}",
            sink_account_id,
            source_account_id,
            amount.to_str()
        );

        // Construct accounts from provided ids and ledger
        let source = Account::new(source_account_id, &ledger)?;
        let sink = Account::new(sink_account_id, &ledger)?;

        // Business rule: The currency of the amount must match the currency of the source.
        let source_currency = source
            .currency()
            .ok_or_else(|| anyhow!("source account has no defined currency"))?;
        let amount_currency = amount.get_currency();
        if source_currency != amount_currency {
            tracing::error!(
                "[MoneyTransfer] new, currency_mismatch, source_currency={:?}, amount_currency={:?}",
                source_currency,
                amount_currency
            );
            return Err(anyhow!(
                "currency mismatch between source account and transfer amount"
            ));
        }

        // Business rule: The transferred amount can't exceed the present balance of the source account.
        let source_balance = source.balance();
        if !Self::amount_leq(&amount, &source_balance) {
            tracing::error!(
                "[MoneyTransfer] new, insufficient_funds, amount={}, balance={}",
                amount.to_str(),
                source_balance.to_str()
            );
            return Err(anyhow!("insufficient funds on source account"));
        }

        Ok(Self {
            source,
            sink,
            amount,
            ledger,
        })
    }

    pub fn transfer(&self) -> Result<Ledger> {
        tracing::info!(
            "[MoneyTransfer] transfer, source_account_id={}, sink_account_id={}, amount={}",
            self.source.account_id(),
            self.sink.account_id(),
            self.amount.to_str()
        );

        // 1. Call source.withdraw
        let withdrawal_entry = self.withdraw()?;

        // 2. Call sink.deposit with result of withdraw
        let settled_entry = self.deposit(withdrawal_entry)?;

        // 3. Add the returned entry to the ledger
        let updated_ledger = self
            .ledger
            .add_entry(settled_entry)
            .map_err(|e| {
                tracing::error!("[MoneyTransfer] transfer, add_entry_failed, error={}", e);
                e
            })?;

        // 4. Return resulting ledger
        tracing::info!("[MoneyTransfer] transfer, completed_successfully");
        Ok(updated_ledger)
    }

    // Role method: source.withdraw
    fn withdraw(&self) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] source withdraw, source_account_id={}, amount={}",
            self.source.account_id(),
            self.amount.to_str()
        );

        // Enforce business rule here as well for role method boundary
        let balance = self.source.balance();
        if !Self::amount_leq(&self.amount, &balance) {
            tracing::error!(
                "[MoneyTransfer] source withdraw, insufficient_funds, amount={}, balance={}",
                self.amount.to_str(),
                balance.to_str()
            );
            return Err(anyhow!("insufficient funds on source account"));
        }

        // Create a withdrawal entry (sink None, source Some(id))
        let entry = self
            .ledger
            .create_entry(Some(self.source.account_id()), self.amount.clone())
            .map_err(|e| {
                tracing::error!(
                    "[MoneyTransfer] source withdraw, create_entry_failed, error={}",
                    e
                );
                e
            })?;

        tracing::debug!("[MoneyTransfer] source withdraw, entry_created");
        Ok(entry)
    }

    // Role method: sink.deposit
    fn deposit(&self, entry: LedgerEntry) -> Result<LedgerEntry> {
        tracing::debug!(
            "[MoneyTransfer] sink deposit, sink_account_id={}",
            self.sink.account_id()
        );

        // Settle the previously created withdrawal entry by setting sink to sink account id
        let settled = self
            .ledger
            .settle(&entry, self.sink.account_id())
            .map_err(|e| {
                tracing::error!("[MoneyTransfer] sink deposit, settle_failed, error={}", e);
                e
            })?;

        tracing::debug!("[MoneyTransfer] sink deposit, entry_settled");
        Ok(settled)
    }

    // Helper: compare two Amounts without exposing internal representation
    fn amount_leq(left: &Amount, right: &Amount) -> bool {
        let (lmj, lmn) = (left.major(), left.minor());
        let (rmj, rmn) = (right.major(), right.minor());
        lmj < rmj || (lmj == rmj && lmn <= rmn)
    }
}