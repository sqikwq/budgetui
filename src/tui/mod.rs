mod app;
mod ui;

use std::io::{self, IsTerminal};

use anyhow::{Result, bail};

use crate::service::BudgetService;

use app::App;

pub fn run() -> Result<()> {
    if !io::stdout().is_terminal() {
        bail!("TUI wymaga interaktywnego terminala.");
    }

    let service = BudgetService::from_default();
    let ledger = service.load()?;
    let mut app = App::new(service, ledger);

    ratatui::run(|terminal| app.run(terminal))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::{FixedOffset, TimeZone};
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;

    use super::app::{App, Filter};
    use crate::model::Transaction;
    use crate::service::BudgetService;
    use crate::store::CsvStore;

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    fn temp_path() -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!(
            "budgetui-tui-{}-{nonce}-{counter}.csv",
            std::process::id()
        ))
    }

    fn sample_app() -> (App, PathBuf) {
        let path = temp_path();
        let service = BudgetService::new(CsvStore::new(&path));
        let offset = FixedOffset::east_opt(3600).unwrap();
        let ledger = crate::model::Ledger::new(vec![
            Transaction {
                id: 1,
                timestamp: offset.with_ymd_and_hms(2026, 11, 24, 10, 0, 0).unwrap(),
                amount_cents: 200_000,
                comment: "wynagrodzenie".to_string(),
            },
            Transaction {
                id: 2,
                timestamp: offset.with_ymd_and_hms(2026, 11, 24, 12, 0, 0).unwrap(),
                amount_cents: -10_000,
                comment: "zakupy".to_string(),
            },
        ]);
        (App::new(service, ledger), path)
    }

    fn render_text(app: &mut App) -> String {
        let backend = TestBackend::new(100, 30);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal
            .draw(|frame| super::ui::render(app, frame))
            .unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn renders_header_ledger_and_shortcuts() {
        let (mut app, path) = sample_app();
        let text = render_text(&mut app);
        assert!(text.contains("Budget UI"));
        assert!(text.contains("Saldo"));
        assert!(text.contains("PRZYCHÓD"));
        assert!(text.contains("WYDATEK"));
        assert!(text.contains("dd"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn filter_changes_visible_rows() {
        let (mut app, path) = sample_app();
        app.filter = Filter::Income;
        let text = render_text(&mut app);
        assert!(text.contains("PRZYCHÓD"));
        assert!(!text.contains("WYDATEK"));
        let _ = std::fs::remove_file(path);
    }
}
