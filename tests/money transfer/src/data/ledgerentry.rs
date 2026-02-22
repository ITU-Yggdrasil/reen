use anyhow::{anyhow, Result};
use base64::{engine::general_purpose::STANDARD, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tracing;

use crate::data::amount::Amount;
use crate::data::currency::Currency;

/// A ledger entry records a single event in the main ledger.
#[derive(Debug, Clone)]
pub struct LedgerEntry {
    sink: Option<i32>,
    source: Option<i32>,
    amount: Amount,
    timestamp: DateTime<Utc>,
    prev_hash: Option<String>,
    hash: String,
}

impl LedgerEntry {
    /// Factory constructor that validates business rules and computes the entry hash.
    ///
    /// Visibility note: This is crate-visible by specification requirement.
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

        // Business rule: At least one of sink and source must be not None.
        if sink.is_none() && source.is_none() {
            tracing::error!(
                "[LedgerEntry] create, validation failed: both sink and source are None"
            );
            return Err(anyhow!(
                "Business rule violation: At least one of sink and source must be not None"
            ));
        }

        // Business rule: amount must be larger than 0.
        // Amount type guarantees positive values, but validate defensively via major/minor.
        if amount.major() == 0 && amount.minor() == 0 {
            tracing::error!("[LedgerEntry] create, validation failed: amount is zero");
            return Err(anyhow!(
                "Business rule violation: amount must be strictly greater than 0"
            ));
        }

        // Validate prev_hash format (if provided): must be RFC 4648 §4 base64 with '=' padding and no whitespace.
        if let Some(ref ph) = prev_hash {
            if let Err(e) = Self::validate_base64_padded(ph) {
                tracing::error!(
                    "[LedgerEntry] create, prev_hash validation failed: {}",
                    e
                );
                return Err(e);
            }
        }

        // Compute hash (SHA256 over concatenation of the values, excluding the hash field)
        let concat = Self::concat_for_hash(&source, &sink, &amount, &timestamp, &prev_hash);
        let digest = Sha256::digest(concat.as_bytes());
        let hash = STANDARD.encode(digest);

        // The hash we generated should already be properly padded and contain no whitespace.
        // Validate to be explicit.
        if let Err(e) = Self::validate_base64_padded(&hash) {
            tracing::error!(
                "[LedgerEntry] create, computed hash validation failed: {}",
                e
            );
            return Err(anyhow!(
                "Internal error: computed hash is not valid padded base64: {}",
                e
            ));
        }

        let entry = LedgerEntry {
            sink,
            source,
            amount,
            timestamp,
            prev_hash,
            hash,
        };

        tracing::info!("[LedgerEntry] create, success");
        Ok(entry)
    }

    /// Returns a formatted string: "{timestamp:?} - {source:?} - {sink:?}:  {amount:?}"
    ///
    /// Note: Returns a borrowed str by leaking the formatted String to 'static.
    /// This adheres to the specified return type while keeping fields immutable.
    pub fn to_str(&self) -> Result<&str> {
        tracing::info!(
            "[LedgerEntry] to_str, timestamp={:?}, source={:?}, sink={:?}",
            self.timestamp,
            self.source,
            self.sink
        );

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

    // Helper: Concatenate field values (excluding hash) into a stable string for hashing.
    fn concat_for_hash(
        source: &Option<i32>,
        sink: &Option<i32>,
        amount: &Amount,
        timestamp: &DateTime<Utc>,
        prev_hash: &Option<String>,
    ) -> String {
        tracing::debug!("[LedgerEntry] concat_for_hash");
        let mut s = String::new();

        // Use a clear, deterministic representation
        s.push_str("ts=");
        s.push_str(&timestamp.timestamp_nanos().to_string());
        s.push('|');

        s.push_str("src=");
        match source {
            Some(v) => s.push_str(&v.to_string()),
            None => s.push_str("None"),
        }
        s.push('|');

        s.push_str("snk=");
        match sink {
            Some(v) => s.push_str(&v.to_string()),
            None => s.push_str("None"),
        }
        s.push('|');

        s.push_str("amt=");
        s.push_str(&amount.to_str());
        s.push('|');

        s.push_str("prev=");
        match prev_hash {
            Some(h) => s.push_str(h),
            None => s.push_str("None"),
        }

        s
    }

    // Helper: Validate that a base64 string is STANDARD with '=' padding, no whitespace.
    fn validate_base64_padded(s: &str) -> Result<()> {
        tracing::debug!("[LedgerEntry] validate_base64_padded");

        // Decode must succeed
        let decoded = STANDARD
            .decode(s.as_bytes())
            .map_err(|e| anyhow!("invalid base64: {}", e))?;

        // Re-encode and compare for canonical padded STANDARD form
        let re = STANDARD.encode(&decoded);
        if re != s {
            return Err(anyhow!(
                "base64 must be canonical STANDARD with '=' padding, no whitespace"
            ));
        }

        Ok(())
    }
}