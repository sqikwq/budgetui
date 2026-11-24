use std::error::Error;
use std::fmt;

use chrono::{DateTime, FixedOffset, Local};

use crate::money::MAX_AMOUNT_CENTS;

pub const MAX_COMMENT_CHARS: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Transaction {
    pub id: u64,
    pub timestamp: DateTime<FixedOffset>,
    pub amount_cents: i64,
    pub comment: String,
}

impl Transaction {
    pub fn is_income(&self) -> bool {
        self.amount_cents > 0
    }

    pub fn is_expense(&self) -> bool {
        self.amount_cents < 0
    }

    pub fn kind_label(&self) -> &'static str {
        if self.is_income() {
            "PRZYCHÓD"
        } else {
            "WYDATEK"
        }
    }

    pub fn amount_display(&self) -> String {
        crate::money::format_cents(self.amount_cents, true)
    }

    pub fn balance_display(&self) -> String {
        crate::money::format_cents(self.amount_cents, false)
    }

    pub fn date_display(&self) -> String {
        self.timestamp
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M")
            .to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionRow {
    pub transaction: Transaction,
    pub balance_after_cents: i64,
}

#[derive(Debug, Clone, Default)]
pub struct Ledger {
    transactions: Vec<Transaction>,
}

impl Ledger {
    pub fn new(mut transactions: Vec<Transaction>) -> Self {
        sort_transactions(&mut transactions);
        Self { transactions }
    }

    pub fn transactions(&self) -> &[Transaction] {
        &self.transactions
    }

    pub fn is_empty(&self) -> bool {
        self.transactions.is_empty()
    }

    pub fn len(&self) -> usize {
        self.transactions.len()
    }

    pub fn total_balance_cents(&self) -> i64 {
        self.transactions.iter().fold(0_i64, |total, transaction| {
            total.saturating_add(transaction.amount_cents)
        })
    }

    pub fn income_total_cents(&self) -> i64 {
        self.transactions
            .iter()
            .filter(|transaction| transaction.is_income())
            .fold(0_i64, |total, transaction| {
                total.saturating_add(transaction.amount_cents)
            })
    }

    pub fn expense_total_cents(&self) -> i64 {
        self.transactions
            .iter()
            .filter(|transaction| transaction.is_expense())
            .fold(0_i64, |total, transaction| {
                total.saturating_add(transaction.amount_cents)
            })
    }

    pub fn next_id(&self) -> Option<u64> {
        self.transactions
            .iter()
            .map(|transaction| transaction.id)
            .max()
            .unwrap_or(0)
            .checked_add(1)
    }

    pub fn insert(&mut self, transaction: Transaction) {
        self.transactions.push(transaction);
        sort_transactions(&mut self.transactions);
    }

    pub fn replace(&mut self, transaction: Transaction) -> bool {
        if let Some(existing) = self
            .transactions
            .iter_mut()
            .find(|existing| existing.id == transaction.id)
        {
            *existing = transaction;
            sort_transactions(&mut self.transactions);
            true
        } else {
            false
        }
    }

    pub fn remove_by_id(&mut self, id: u64) -> Option<Transaction> {
        let index = self
            .transactions
            .iter()
            .position(|transaction| transaction.id == id)?;
        Some(self.transactions.remove(index))
    }

    pub fn find_by_id(&self, id: u64) -> Option<&Transaction> {
        self.transactions
            .iter()
            .find(|transaction| transaction.id == id)
    }

    pub fn rows_newest_first(&self) -> Vec<TransactionRow> {
        let mut balance = 0_i64;
        let mut rows = Vec::with_capacity(self.transactions.len());
        for transaction in &self.transactions {
            balance = balance.saturating_add(transaction.amount_cents);
            rows.push(TransactionRow {
                transaction: transaction.clone(),
                balance_after_cents: balance,
            });
        }
        rows.reverse();
        rows
    }
}

fn sort_transactions(transactions: &mut [Transaction]) {
    transactions.sort_by(|left, right| {
        left.timestamp
            .cmp(&right.timestamp)
            .then_with(|| left.id.cmp(&right.id))
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommentError {
    TooLong,
    ControlCharacter,
}

impl fmt::Display for CommentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLong => write!(
                f,
                "Komentarz może mieć maksymalnie {MAX_COMMENT_CHARS} znaków."
            ),
            Self::ControlCharacter => {
                write!(
                    f,
                    "Komentarz nie może zawierać znaków kontrolnych ani nowej linii."
                )
            }
        }
    }
}

impl Error for CommentError {}

pub fn sanitize_comment(input: &str) -> Result<String, CommentError> {
    let trimmed = input.trim();
    if trimmed.chars().count() > MAX_COMMENT_CHARS {
        return Err(CommentError::TooLong);
    }
    if trimmed.chars().any(char::is_control) {
        return Err(CommentError::ControlCharacter);
    }
    Ok(trimmed.to_string())
}

pub fn validate_amount(amount_cents: i64) -> Result<(), &'static str> {
    if amount_cents == 0 {
        return Err("Kwota nie może wynosić zero.");
    }
    if amount_cents.unsigned_abs() > MAX_AMOUNT_CENTS as u64 {
        return Err("Kwota jest zbyt duża.");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use chrono::{FixedOffset, TimeZone};

    use super::*;

    fn offset() -> FixedOffset {
        FixedOffset::east_opt(3600).unwrap()
    }

    fn transaction(id: u64, hour: u32, amount_cents: i64) -> Transaction {
        Transaction {
            id,
            timestamp: offset().with_ymd_and_hms(2026, 11, 24, hour, 0, 0).unwrap(),
            amount_cents,
            comment: String::new(),
        }
    }

    #[test]
    fn computes_totals_and_running_balances() {
        let ledger = Ledger::new(vec![
            transaction(1, 10, 200_000),
            transaction(2, 12, -10_000),
            transaction(3, 14, 5_000),
        ]);

        assert_eq!(ledger.total_balance_cents(), 195_000);
        assert_eq!(ledger.income_total_cents(), 205_000);
        assert_eq!(ledger.expense_total_cents(), -10_000);

        let rows = ledger.rows_newest_first();
        assert_eq!(rows[0].transaction.id, 3);
        assert_eq!(rows[0].balance_after_cents, 195_000);
        assert_eq!(rows[1].transaction.id, 2);
        assert_eq!(rows[1].balance_after_cents, 190_000);
        assert_eq!(rows[2].transaction.id, 1);
        assert_eq!(rows[2].balance_after_cents, 200_000);
    }

    #[test]
    fn sorts_same_timestamp_by_id() {
        let ledger = Ledger::new(vec![transaction(2, 10, 2), transaction(1, 10, 1)]);
        let ids: Vec<u64> = ledger
            .transactions()
            .iter()
            .map(|transaction| transaction.id)
            .collect();
        assert_eq!(ids, vec![1, 2]);
    }

    #[test]
    fn sanitizes_comments() {
        assert_eq!(sanitize_comment("  zakupy  ").unwrap(), "zakupy");
        assert!(sanitize_comment("linia\nnowa").is_err());
        assert!(sanitize_comment(&"x".repeat(MAX_COMMENT_CHARS + 1)).is_err());
    }
}
