use anyhow::{bail, Result};
use base64::{engine::general_purpose, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::sync::RwLock;
use tracing;

use crate::data::amount::Amount;
use crate::data::currency::Currency;

/// A ledger entry records a single event in the main ledger.
///
/// Immutable data type; all fields are private with no setters.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    pub sink: Option<i32>,
    pub source: Option<i32>,
    pub amount: Amount,
    pub timestamp: DateTime<Utc>,
    pub prev_hash: Option<String>,
    pub hash: String,
}

impl LedgerEntry {
    /// Create a new LedgerEntry instance.
    ///
    /// Business rules enforced:
    /// - At least one of sink and source must be not None.
    /// - The amount must be larger than 0 (assumed enforced by Amount type).
    ///
    /// Notes:
    /// - The `hash` is calculated internally from the other fields.
    /// - This constructor is crate-visible as per specification notes.
    pub(crate) fn create(
        source: Option<i32>,
        sink: Option<i32>,
        amount: Amount,
        timestamp: DateTime<Utc>,
        prev_hash: Option<String>,
    ) -> Result<LedgerEntry> {
        tracing::info!(
            "[LedgerEntry] create, source={:?}, sink={:?}, timestamp={:?}, prev_hash_present={}",
            source,
            sink,
            timestamp,
            prev_hash.is_some()
        );

        if source.is_none() && sink.is_none() {
            bail!("Business rule violated: at least one of sink or source must be provided (not None).");
        }

        let hash = Self::compute_hash(&source, &sink, &amount, &timestamp, &prev_hash);

        Ok(LedgerEntry {
            sink,
            source,
            amount,
            timestamp,
            prev_hash,
            hash,
        })
    }

    /// Returns a formatted string view of the ledger entry.
    ///
    /// Format: "{timestamp:?} - {source:?} - {sink:?}:  {amount:?}"
    pub fn to_str(&self) -> Result<&str> {
        tracing::info!(
            "[LedgerEntry] to_str, timestamp={:?}, source={:?}, sink={:?}",
            self.timestamp,
            self.source,
            self.sink
        );

        // Build new string and store a leaked reference for stable &'static str
        let s = format!(
            "{:?} - {:?} - {:?}:  {:?}",
            self.timestamp, self.source, self.sink, self.amount
        );
        let leaked: &'static str = Box::leak(s.into_boxed_str());

        Ok(leaked)
    }

    /// The currency of the amount.
    pub fn currency(&self) -> Currency {
        tracing::info!("[LedgerEntry] currency");
        self.amount.get_currency()
    }

    fn compute_hash(
        source: &Option<i32>,
        sink: &Option<i32>,
        amount: &Amount,
        timestamp: &DateTime<Utc>,
        prev_hash: &Option<String>,
    ) -> String {
        tracing::debug!("[LedgerEntry] compute_hash");

        // Concatenation strategy (implementation-defined per specification).
        // Use stable, explicit components and delimiters.
        let amount_str = amount.to_str();
        let ts_str = timestamp.to_rfc3339();
        let prev_str = prev_hash.as_deref().unwrap_or("None");

        let input = format!(
            "ts={}|source={:?}|sink={:?}|amount={}|prev={}",
            ts_str, source, sink, amount_str, prev_str
        );

        let digest = Sha256::digest(input.as_bytes());
        general_purpose::STANDARD.encode(digest)
    }
}