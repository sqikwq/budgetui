use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, FixedOffset, Local};

use crate::model::{Ledger, Transaction, sanitize_comment, validate_amount};
use crate::store::CsvStore;

#[derive(Debug, Clone)]
pub struct RemoveSelector {
    pub amount_cents: i64,
    pub signed: bool,
    pub comment: String,
}

#[derive(Debug)]
pub enum RemoveOutcome {
    Removed {
        removed: Transaction,
        ledger: Ledger,
    },
    Ambiguous {
        candidates: Vec<Transaction>,
    },
    NotFound,
}

#[derive(Debug, Clone)]
pub struct BudgetService {
    store: CsvStore,
}

impl BudgetService {
    pub fn new(store: CsvStore) -> Self {
        Self { store }
    }

    pub fn from_default() -> Self {
        Self::new(CsvStore::default_store())
    }

    pub fn store(&self) -> &CsvStore {
        &self.store
    }

    pub fn load(&self) -> Result<Ledger> {
        self.store.load()
    }

    pub fn add(
        &self,
        amount_cents: i64,
        comment: &str,
        timestamp: DateTime<FixedOffset>,
    ) -> Result<(Transaction, Ledger)> {
        validate_amount(amount_cents).map_err(anyhow::Error::msg)?;
        let comment = sanitize_comment(comment)?;
        let mut ledger = self.store.load()?;
        let id = ledger
            .next_id()
            .ok_or_else(|| anyhow!("Brak wolnych identyfikatorów transakcji."))?;
        let transaction = Transaction {
            id,
            timestamp,
            amount_cents,
            comment,
        };
        ledger.insert(transaction.clone());
        self.store.save(&ledger)?;
        Ok((transaction, ledger))
    }

    pub fn update(
        &self,
        id: u64,
        amount_cents: i64,
        comment: &str,
        timestamp: Option<DateTime<FixedOffset>>,
    ) -> Result<(Transaction, Ledger)> {
        validate_amount(amount_cents).map_err(anyhow::Error::msg)?;
        let comment = sanitize_comment(comment)?;
        let mut ledger = self.store.load()?;
        let mut transaction = ledger
            .find_by_id(id)
            .cloned()
            .with_context(|| format!("Nie znaleziono transakcji #{id}."))?;
        transaction.amount_cents = amount_cents;
        transaction.comment = comment;
        if let Some(timestamp) = timestamp {
            transaction.timestamp = timestamp;
        }
        ledger.replace(transaction.clone());
        self.store.save(&ledger)?;
        Ok((transaction, ledger))
    }

    pub fn remove_by_id(&self, id: u64) -> Result<(Transaction, Ledger)> {
        let mut ledger = self.store.load()?;
        let removed = ledger
            .remove_by_id(id)
            .with_context(|| format!("Nie znaleziono transakcji #{id}."))?;
        self.store.save(&ledger)?;
        Ok((removed, ledger))
    }

    pub fn remove_matching(&self, selector: &RemoveSelector) -> Result<RemoveOutcome> {
        validate_amount(selector.amount_cents).map_err(anyhow::Error::msg)?;
        let comment = sanitize_comment(&selector.comment)?;
        let mut ledger = self.store.load()?;

        let mut candidates: Vec<Transaction> = ledger
            .transactions()
            .iter()
            .filter(|transaction| {
                let amount_matches = if selector.signed {
                    transaction.amount_cents == selector.amount_cents
                } else {
                    transaction.amount_cents.unsigned_abs() == selector.amount_cents.unsigned_abs()
                };
                let comment_matches = comment.is_empty() || transaction.comment == comment;
                amount_matches && comment_matches
            })
            .cloned()
            .collect();
        candidates.reverse();

        match candidates.len() {
            0 => Ok(RemoveOutcome::NotFound),
            1 => {
                let removed = candidates.remove(0);
                ledger.remove_by_id(removed.id);
                self.store.save(&ledger)?;
                Ok(RemoveOutcome::Removed { removed, ledger })
            }
            _ => Ok(RemoveOutcome::Ambiguous { candidates }),
        }
    }
}

pub fn now() -> DateTime<FixedOffset> {
    Local::now().fixed_offset()
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{FixedOffset, TimeZone};

    use super::*;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "budgetui-service-{}-{nonce}-{counter}.csv",
            std::process::id()
        ))
    }

    fn timestamp(hour: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 24, hour, 0, 0)
            .unwrap()
    }

    fn service() -> (BudgetService, PathBuf) {
        let path = temp_path();
        (BudgetService::new(CsvStore::new(&path)), path)
    }

    #[test]
    fn adds_updates_and_removes_by_id() {
        let (service, path) = service();
        let (added, _) = service.add(10_000, "pensja", timestamp(10)).unwrap();
        assert_eq!(added.id, 1);

        let (updated, ledger) = service
            .update(1, -2_500, "korekta", Some(timestamp(12)))
            .unwrap();
        assert_eq!(updated.timestamp, timestamp(12));
        assert_eq!(updated.comment, "korekta");
        assert_eq!(ledger.total_balance_cents(), -2_500);

        let (removed, ledger) = service.remove_by_id(1).unwrap();
        assert_eq!(removed.id, 1);
        assert!(ledger.is_empty());
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn matches_unsigned_amount_by_absolute_value() {
        let (service, path) = service();
        service.add(-10_000, "wydatek", timestamp(10)).unwrap();

        let outcome = service
            .remove_matching(&RemoveSelector {
                amount_cents: 10_000,
                signed: false,
                comment: "wydatek".to_string(),
            })
            .unwrap();
        assert!(matches!(
            outcome,
            RemoveOutcome::Removed { ref removed, .. } if removed.id == 1
        ));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn refuses_ambiguous_removal_without_mutating_data() {
        let (service, path) = service();
        service.add(10_000, "dodatek", timestamp(10)).unwrap();
        service.add(-10_000, "wydatek", timestamp(11)).unwrap();

        let outcome = service
            .remove_matching(&RemoveSelector {
                amount_cents: 10_000,
                signed: false,
                comment: String::new(),
            })
            .unwrap();
        assert!(matches!(outcome, RemoveOutcome::Ambiguous { .. }));
        assert_eq!(service.load().unwrap().len(), 2);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn returns_not_found_for_missing_match() {
        let (service, path) = service();
        let outcome = service
            .remove_matching(&RemoveSelector {
                amount_cents: 10_000,
                signed: false,
                comment: String::new(),
            })
            .unwrap();
        assert!(matches!(outcome, RemoveOutcome::NotFound));
        let _ = std::fs::remove_file(path);
    }
}
