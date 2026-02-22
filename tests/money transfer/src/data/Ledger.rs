use std::rc::Rc;

use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use tracing;

use crate::types::amount::Amount;
use crate::types::ledgerentry::LedgerEntry;

/// The Ledger is an immutable chain of ledger entries.
/// The `head` is the current entry; `tail` represents all previous entries.
#[derive(Debug, Clone)]
pub struct Ledger {
    head: LedgerEntry,
    tail: Option<Rc<Ledger>>,
}

impl Ledger {
    /// Creates a new ledger with None as tail and the provided entry as head.
    pub fn new(entry: LedgerEntry) -> Self {
        tracing::info!("[Ledger] new");
        Self { head: entry, tail: None }
    }

    /// Returns all ledger entries where the account is either sink or source,
    /// sorted ascending by the entries’ timestamps.
    pub fn get_entries_for(&self, account: i32) -> Vec<LedgerEntry> {
        tracing::info!("[Ledger] get_entries_for, account={}", account);

        let mut matches: Vec<LedgerEntry> = Vec::new();

        let mut current: Option<&Ledger> = Some(self);
        while let Some(ledger) = current {
            let entry = &ledger.head;

            let is_sink = entry.sink().map(|s| s == account).unwrap_or(false);
            let is_source = entry.source().map(|s| s == account).unwrap_or(false);

            if is_sink || is_source {
                matches.push(entry.clone());
            }

            current = ledger.tail.as_ref().map(|rc| rc.as_ref());
        }

        matches.sort_by_key(|e| e.timestamp().clone());
        matches
    }

    /// Commits an entry to the ledger and returns a new Ledger whose tail is the
    /// previous ledger and whose head is the committed entry.
    ///
    /// Constraints:
    /// - At least one of sink and source must be not None
    /// - The hash of the current head entry must match the prev_hash of the entry being added
    pub fn add_entry(&self, entry: LedgerEntry) -> Result<Ledger> {
        tracing::info!("[Ledger] add_entry");

        let has_party = entry.sink().is_some() || entry.source().is_some();
        if !has_party {
            let msg = "add_entry validation failed: at least one of sink and source must be not None";
            tracing::error!("[Ledger] add_entry, error=\"{}\"", msg);
            return Err(anyhow!(msg));
        }

        let current_head_hash = self.head.hash();
        let prev_ok = match entry.prev_hash() {
            Some(prev) => prev == current_head_hash,
            None => false,
        };
        if !prev_ok {
            let msg = "add_entry validation failed: entry.prev_hash must equal current head hash";
            tracing::error!("[Ledger] add_entry, error=\"{}\"", msg);
            return Err(anyhow!(msg));
        }

        let new_ledger = Ledger {
            head: entry,
            tail: Some(Rc::new(self.clone())),
        };

        tracing::debug!("[Ledger] add_entry, committed");
        Ok(new_ledger)
    }

    /// Creates a new entry based on the input entry, setting sink to the provided account id;
    /// the timestamp of the new entry is equal to the timestamp of the original entry.
    ///
    /// Valid only for an unsettled entry (i.e., one where sink is None)
    pub fn settle(&self, entry: &LedgerEntry, sink_account_id: i32) -> Result<LedgerEntry> {
        tracing::info!(
            "[Ledger] settle, sink_account_id={}, input_prev_hash_present={}",
            sink_account_id,
            entry.prev_hash().is_some()
        );

        if entry.sink().is_some() {
            let msg = "settle validation failed: entry is already settled (sink is Some)";
            tracing::error!("[Ledger] settle, error=\"{}\"", msg);
            return Err(anyhow!(msg));
        }

        let sink = Some(sink_account_id);
        let source = entry.source();
        let amount: Amount = entry.amount().clone();
        let timestamp: DateTime<Utc> = entry.timestamp().clone();
        let prev_hash = Some(self.head.hash().clone());

        tracing::debug!(
            "[Ledger] settle, constructing entry, source={:?}, sink={:?}, timestamp={:?}",
            source,
            sink,
            timestamp
        );

        let settled = LedgerEntry::create(sink, source, amount, timestamp, prev_hash)?;
        tracing::debug!("[Ledger] settle, created settled entry");
        Ok(settled)
    }

    /// Constructs a new ledger entry and returns it.
    ///
    /// Constraints:
    /// - If source is not None, at least one entry for that account must exist on the ledger
    /// - sink is always None for the constructed entry
    /// - timestamp is utc.now
    /// - prev_hash must be set to the hash of the current head entry on the ledger
    /// - At least one of sink and source must be not None (implied business rule)
    pub fn create_entry(&self, source: Option<i32>, amount: Amount) -> Result<LedgerEntry> {
        tracing::info!(
            "[Ledger] create_entry, source={:?}, amount_minor={}, amount_currency={:?}",
            source,
            amount.minor(),
            amount.currency()
        );

        // Enforce that the constructed entry is valid per business rules:
        // since sink is always None, source must be Some(...)
        if source.is_none() {
            let msg = "create_entry validation failed: source must be Some when sink is None";
            tracing::error!("[Ledger] create_entry, error=\"{}\"", msg);
            return Err(anyhow!(msg));
        }

        if let Some(src) = source {
            let existing = self.get_entries_for(src);
            if existing.is_empty() {
                let msg = "create_entry validation failed: at least one entry for the source account must exist on the ledger";
                tracing::error!("[Ledger] create_entry, error=\"{}\"", msg);
                return Err(anyhow!(msg));
            }
            // Use the value again
            let source = Some(src);

            let sink = None;
            let timestamp = Utc::now();
            let prev_hash = Some(self.head.hash().clone());

            tracing::debug!(
                "[Ledger] create_entry, constructing entry, source={:?}, sink={:?}, timestamp={:?}",
                source,
                sink,
                timestamp
            );

            let entry = LedgerEntry::create(sink, source, amount, timestamp, prev_hash)?;
            tracing::debug!("[Ledger] create_entry, entry created");
            Ok(entry)
        } else {
            // Unreachable due to early return; kept for completeness
            let msg = "create_entry validation failed: unreachable None source branch";
            tracing::error!("[Ledger] create_entry, error=\"{}\"", msg);
            Err(anyhow!(msg))
        }
    }
}