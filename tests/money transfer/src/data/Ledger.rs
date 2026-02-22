use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use tracing;

use crate::data::amount::Amount;
use crate::data::ledgerentry::LedgerEntry;

/// The Ledger is an immutable chain of ledger entries.
/// Each new entry produces a new Ledger with the new head and the previous ledger as tail.
#[derive(Debug, Clone)]
pub struct Ledger {
    head: LedgerEntry,
    // Immutable reference-equivalent to the previous ledger state
    tail: Option<Box<Ledger>>,
}

impl Ledger {
    /// Creates a new ledger with None as tail and the provided entry as head.
    pub fn new(entry: LedgerEntry) -> Self {
        tracing::info!("[Ledger] new");
        Self { head: entry, tail: None }
    }

    /// Returns all ledger entries where the account is either sink or source,
    /// sorted ascending by the entries' timestamps.
    pub fn get_entries_for(&self, account: i32) -> Vec<LedgerEntry> {
        tracing::info!("[Ledger] get_entries_for, account={}", account);
        let mut all_entries = Vec::new();
        self.collect_entries(&mut all_entries);

        let mut filtered: Vec<LedgerEntry> = all_entries
            .into_iter()
            .filter(|e| e.sink == Some(account) || e.source == Some(account))
            .collect();

        filtered.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
        filtered
    }

    /// Commits an entry to the ledger and returns a new Ledger whose tail is the previous ledger
    /// and whose head is the committed entry.
    ///
    /// Constraints:
    /// - At least one of sink and source must be not None
    /// - The hash of the current head entry must match the prev_hash of the entry being added
    pub fn add_entry(&self, entry: LedgerEntry) -> Result<Ledger> {
        tracing::info!("[Ledger] add_entry");

        // Validate at least one of sink/source present
        if entry.sink.is_none() && entry.source.is_none() {
            let msg = "add_entry constraint violated: at least one of sink and source must be not None";
            tracing::error!("[Ledger] add_entry, error={}", msg);
            return Err(anyhow!(msg));
        }

        // Validate hash chain linkage
        match &entry.prev_hash {
            Some(prev_hash) if *prev_hash == self.head.hash => {
                // OK
            }
            _ => {
                let msg = "add_entry constraint violated: head.hash does not match entry.prev_hash";
                tracing::error!("[Ledger] add_entry, error={}", msg);
                return Err(anyhow!(msg));
            }
        }

        let new_ledger = Ledger {
            head: entry,
            tail: Some(Box::new(self.clone())),
        };

        Ok(new_ledger)
    }

    /// Creates a new entry based on the input entry, setting sink to the provided account id;
    /// the timestamp of the new entry is equal to the timestamp of the original entry.
    ///
    /// Valid only for an unsettled entry (i.e., one where sink is None).
    pub fn settle(&self, entry: LedgerEntry, sink_account_id: i32) -> Result<LedgerEntry> {
        tracing::info!("[Ledger] settle, sink_account_id={}", sink_account_id);

        if entry.sink.is_some() {
            let msg = "settle is only valid for an unsettled entry (sink must be None)";
            tracing::error!("[Ledger] settle, error={}", msg);
            return Err(anyhow!(msg));
        }

        let sink = Some(sink_account_id);
        let source = entry.source;
        let amount: Amount = entry.amount.clone();
        // Timestamp must equal the original entry's timestamp
        let timestamp: DateTime<Utc> = entry.timestamp;
        // Ensure chain linkage to current head
        let prev_hash = Some(self.head.hash.clone());

        // Create the settled entry
        let settled = LedgerEntry::create(sink, source, amount, timestamp, prev_hash)?;
        Ok(settled)
    }

    /// Constructs a new ledger entry and returns it.
    ///
    /// Constraints:
    /// - If source is not None, at least one entry for that account must exist on the ledger
    /// - sink is always None for the constructed entry
    /// - timestamp is utc.now
    /// - prev_hash must be set to the hash of the current head entry on the ledger
    /// - The entry’s own hash is computed by the entry itself
    pub fn create_entry(&self, source: Option<i32>, amount: Amount) -> Result<LedgerEntry> {
        tracing::info!("[Ledger] create_entry, source={:?}", source);

        if let Some(src) = source {
            let existing = self.get_entries_for(src);
            if existing.is_empty() {
                let msg = "create_entry constraint violated: source account has no prior entries on this ledger";
                tracing::error!("[Ledger] create_entry, error={}", msg);
                return Err(anyhow!(msg));
            }
        }

        let sink: Option<i32> = None;
        let timestamp = Utc::now();
        let prev_hash = Some(self.head.hash.clone());

        let entry = LedgerEntry::create(sink, source, amount, timestamp, prev_hash)?;
        Ok(entry)
    }

    // Private helper to collect all entries from head to tail recursively.
    fn collect_entries(&self, acc: &mut Vec<LedgerEntry>) {
        tracing::debug!("[Ledger] collect_entries");
        acc.push(self.head.clone());
        if let Some(ref tail) = self.tail {
            tail.collect_entries(acc);
        }
    }
}