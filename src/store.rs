use std::collections::HashSet;
use std::fs::{self, File};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use chrono::{DateTime, SecondsFormat};
use csv::{ReaderBuilder, WriterBuilder};

use crate::model::{Ledger, Transaction, sanitize_comment, validate_amount};
use crate::money::{format_plain, parse_amount};

pub const DEFAULT_FILE: &str = "budgetui.csv";
const HEADERS: [&str; 4] = ["id", "timestamp", "amount_pln", "comment"];

#[derive(Debug, Clone)]
pub struct CsvStore {
    path: PathBuf,
}

impl CsvStore {
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    pub fn default_path() -> PathBuf {
        std::env::var_os("BUDGETUI_FILE")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(DEFAULT_FILE))
    }

    pub fn default_store() -> Self {
        Self::new(Self::default_path())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn last_modified(&self) -> Option<SystemTime> {
        fs::metadata(&self.path)
            .and_then(|metadata| metadata.modified())
            .ok()
    }

    pub fn load(&self) -> Result<Ledger> {
        if !self.path.exists() {
            return Ok(Ledger::default());
        }

        let metadata = fs::metadata(&self.path).with_context(|| {
            format!(
                "Nie można odczytać informacji o pliku {}",
                self.path.display()
            )
        })?;
        if metadata.len() == 0 {
            return Ok(Ledger::default());
        }

        let file = File::open(&self.path)
            .with_context(|| format!("Nie można otworzyć pliku danych {}", self.path.display()))?;
        let mut reader = ReaderBuilder::new()
            .has_headers(true)
            .flexible(false)
            .from_reader(file);

        let headers = reader
            .headers()
            .with_context(|| format!("Nie można odczytać nagłówka pliku {}", self.path.display()))?
            .clone();
        let header_values: Vec<&str> = headers.iter().collect();
        let valid_headers = header_values.len() == HEADERS.len()
            && header_values
                .iter()
                .enumerate()
                .all(|(index, header)| header.trim_start_matches('\u{feff}') == HEADERS[index]);
        if !valid_headers {
            bail!(
                "Nieprawidłowy nagłówek pliku {}. Oczekiwano: {}",
                self.path.display(),
                HEADERS.join(",")
            );
        }

        let mut transactions = Vec::new();
        let mut ids = HashSet::new();
        for (index, record) in reader.records().enumerate() {
            let line = index + 2;
            let record = record.with_context(|| {
                format!(
                    "Błąd składni CSV w pliku {}, linia {line}",
                    self.path.display()
                )
            })?;
            let transaction = parse_record(&record, line, self.path())?;
            if !ids.insert(transaction.id) {
                bail!(
                    "Zduplikowane ID {} w pliku {}, linia {line}",
                    transaction.id,
                    self.path.display()
                );
            }
            transactions.push(transaction);
        }

        Ok(Ledger::new(transactions))
    }

    pub fn save(&self, ledger: &Ledger) -> Result<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            fs::create_dir_all(parent)
                .with_context(|| format!("Nie można utworzyć katalogu {}", parent.display()))?;
        }

        let file_name = self
            .path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("budgetui.csv");
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let temp_path = self
            .path
            .with_file_name(format!(".{file_name}.tmp-{}-{nonce}", std::process::id()));

        let result = self.write_temp_file(&temp_path, ledger);
        if result.is_err() {
            let _ = fs::remove_file(&temp_path);
            return result;
        }

        if cfg!(windows) && self.path.exists() {
            fs::remove_file(&self.path).with_context(|| {
                format!(
                    "Nie można zastąpić istniejącego pliku {}",
                    self.path.display()
                )
            })?;
        }

        if let Err(error) = fs::rename(&temp_path, &self.path) {
            let _ = fs::remove_file(&temp_path);
            return Err(error).with_context(|| {
                format!("Nie można zapisać pliku danych {}", self.path.display())
            });
        }

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600));
        }

        Ok(())
    }

    fn write_temp_file(&self, temp_path: &Path, ledger: &Ledger) -> Result<()> {
        let file = File::create(temp_path).with_context(|| {
            format!(
                "Nie można utworzyć pliku tymczasowego {}",
                temp_path.display()
            )
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }

        let mut writer = WriterBuilder::new().has_headers(false).from_writer(file);
        writer
            .write_record(HEADERS)
            .context("Nie można zapisać nagłówka CSV")?;

        for transaction in ledger.transactions() {
            writer
                .write_record([
                    transaction.id.to_string(),
                    transaction
                        .timestamp
                        .to_rfc3339_opts(SecondsFormat::Secs, false),
                    format_plain(transaction.amount_cents),
                    transaction.comment.clone(),
                ])
                .context("Nie można zapisać transakcji do CSV")?;
        }

        writer.flush().context("Nie można opróżnić bufora CSV")?;
        writer
            .get_ref()
            .sync_all()
            .context("Nie można zsynchronizować pliku CSV")?;
        Ok(())
    }
}

fn parse_record(record: &csv::StringRecord, line: usize, path: &Path) -> Result<Transaction> {
    if record.len() != HEADERS.len() {
        bail!(
            "Nieprawidłowa liczba kolumn w pliku {}, linia {line}",
            path.display()
        );
    }

    let id: u64 =
        record.get(0).unwrap_or_default().parse().with_context(|| {
            format!("Nieprawidłowe ID w pliku {}, linia {line}", path.display())
        })?;
    if id == 0 {
        bail!(
            "ID musi być większe od zera w pliku {}, linia {line}",
            path.display()
        );
    }

    let timestamp =
        DateTime::parse_from_rfc3339(record.get(1).unwrap_or_default()).with_context(|| {
            format!(
                "Nieprawidłowa data w pliku {}, linia {line}",
                path.display()
            )
        })?;

    let amount_cents = parse_amount(record.get(2).unwrap_or_default()).with_context(|| {
        format!(
            "Nieprawidłowa kwota w pliku {}, linia {line}",
            path.display()
        )
    })?;
    validate_amount(amount_cents)
        .map_err(anyhow::Error::msg)
        .with_context(|| {
            format!(
                "Nieprawidłowa kwota w pliku {}, linia {line}",
                path.display()
            )
        })?;

    let comment = sanitize_comment(record.get(3).unwrap_or_default()).with_context(|| {
        format!(
            "Nieprawidłowy komentarz w pliku {}, linia {line}",
            path.display()
        )
    })?;

    Ok(Transaction {
        id,
        timestamp,
        amount_cents,
        comment,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicU64, Ordering};

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
            "budgetui-store-{}-{nonce}-{counter}.csv",
            std::process::id()
        ))
    }

    fn timestamp(hour: u32) -> DateTime<FixedOffset> {
        FixedOffset::east_opt(3600)
            .unwrap()
            .with_ymd_and_hms(2026, 11, 24, hour, 0, 0)
            .unwrap()
    }

    #[test]
    fn missing_file_loads_as_empty_ledger() {
        let path = temp_path();
        let store = CsvStore::new(&path);
        let ledger = store.load().unwrap();
        assert!(ledger.is_empty());
    }

    #[test]
    fn round_trips_csv_with_special_characters() {
        let path = temp_path();
        let store = CsvStore::new(&path);
        let ledger = Ledger::new(vec![
            Transaction {
                id: 1,
                timestamp: timestamp(10),
                amount_cents: 123_456,
                comment: "wynagrodzenie, premia \"świąteczna\"".to_string(),
            },
            Transaction {
                id: 2,
                timestamp: timestamp(12),
                amount_cents: -12_345,
                comment: "zakupy: mleko, chleb".to_string(),
            },
        ]);

        store.save(&ledger).unwrap();
        let loaded = store.load().unwrap();

        assert_eq!(loaded.transactions(), ledger.transactions());
        let content = fs::read_to_string(&path).unwrap();
        assert!(content.contains("amount_pln"));
        assert!(content.contains("1234.56"));
        assert!(content.contains("-123.45"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_duplicate_ids() {
        let path = temp_path();
        fs::write(
            &path,
            "id,timestamp,amount_pln,comment\n1,2026-11-24T10:00:00+01:00,10.00,a\n1,2026-11-24T11:00:00+01:00,20.00,b\n",
        )
        .unwrap();
        let store = CsvStore::new(&path);
        let error = store.load().unwrap_err().to_string();
        assert!(error.contains("Zduplikowane ID"));
        let _ = fs::remove_file(path);
    }

    #[test]
    fn rejects_invalid_header() {
        let path = temp_path();
        fs::write(&path, "id,data,kwota\n1,2026-11-24,100\n").unwrap();
        let store = CsvStore::new(&path);
        let error = store.load().unwrap_err().to_string();
        assert!(error.contains("Nieprawidłowy nagłówek"));
        let _ = fs::remove_file(path);
    }
}
