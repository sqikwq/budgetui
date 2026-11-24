use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

use anyhow::{Result, anyhow, bail};
use chrono::{DateTime, FixedOffset, Local, NaiveDate, NaiveDateTime, TimeZone};
use ratatui::DefaultTerminal;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

use crate::model::{Ledger, MAX_COMMENT_CHARS, Transaction, TransactionRow};
use crate::money::{format_positive_plain, parse_amount};
use crate::service::BudgetService;

use super::ui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Filter {
    All,
    Income,
    Expense,
}

impl Filter {
    pub(crate) fn next(self) -> Self {
        match self {
            Self::All => Self::Income,
            Self::Income => Self::Expense,
            Self::Expense => Self::All,
        }
    }

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::All => "wszystkie",
            Self::Income => "przychody",
            Self::Expense => "wydatki",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Income,
    Expense,
}

impl Kind {
    fn from_amount(amount_cents: i64) -> Self {
        if amount_cents >= 0 {
            Self::Income
        } else {
            Self::Expense
        }
    }

    fn toggled(self) -> Self {
        match self {
            Self::Income => Self::Expense,
            Self::Expense => Self::Income,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormField {
    Amount,
    Timestamp,
    Comment,
    Type,
}

impl FormField {
    fn next(self) -> Self {
        match self {
            Self::Amount => Self::Timestamp,
            Self::Timestamp => Self::Comment,
            Self::Comment => Self::Type,
            Self::Type => Self::Amount,
        }
    }

    fn previous(self) -> Self {
        match self {
            Self::Amount => Self::Type,
            Self::Timestamp => Self::Amount,
            Self::Comment => Self::Timestamp,
            Self::Type => Self::Comment,
        }
    }
}

#[derive(Debug, Clone)]
pub(crate) struct TextField {
    pub(crate) value: String,
    pub(crate) cursor: usize,
}

impl TextField {
    fn new(value: impl Into<String>) -> Self {
        let value = value.into();
        let cursor = value.chars().count();
        Self { value, cursor }
    }

    fn empty() -> Self {
        Self::new("")
    }

    fn char_count(&self) -> usize {
        self.value.chars().count()
    }

    fn byte_index(&self) -> usize {
        self.value
            .char_indices()
            .nth(self.cursor)
            .map(|(index, _)| index)
            .unwrap_or(self.value.len())
    }

    fn insert(&mut self, character: char, max_chars: usize) {
        if self.char_count() >= max_chars {
            return;
        }
        let index = self.byte_index();
        self.value.insert(index, character);
        self.cursor += 1;
    }

    fn backspace(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let index = self
            .value
            .char_indices()
            .nth(self.cursor - 1)
            .map(|(index, _)| index)
            .unwrap_or(0);
        self.value.remove(index);
        self.cursor -= 1;
    }

    fn delete(&mut self) {
        if self.cursor >= self.char_count() {
            return;
        }
        let index = self.byte_index();
        self.value.remove(index);
    }

    fn left(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    fn right(&mut self) {
        self.cursor = (self.cursor + 1).min(self.char_count());
    }

    fn home(&mut self) {
        self.cursor = 0;
    }

    fn end(&mut self) {
        self.cursor = self.char_count();
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum FormMode {
    Add,
    Edit { id: u64 },
}

#[derive(Debug, Clone)]
pub(crate) struct FormState {
    pub(crate) mode: FormMode,
    pub(crate) kind: Kind,
    pub(crate) amount: TextField,
    pub(crate) date: TextField,
    pub(crate) comment: TextField,
    pub(crate) focus: FormField,
    pub(crate) error: Option<String>,
}

impl FormState {
    fn add() -> Self {
        Self {
            mode: FormMode::Add,
            kind: Kind::Income,
            amount: TextField::empty(),
            date: TextField::new(Local::now().format("%Y-%m-%d %H:%M").to_string()),
            comment: TextField::empty(),
            focus: FormField::Amount,
            error: None,
        }
    }

    fn edit(transaction: &Transaction) -> Self {
        Self {
            mode: FormMode::Edit { id: transaction.id },
            kind: Kind::from_amount(transaction.amount_cents),
            amount: TextField::new(format_positive_plain(transaction.amount_cents)),
            date: TextField::new(
                transaction
                    .timestamp
                    .with_timezone(&Local)
                    .format("%Y-%m-%d %H:%M")
                    .to_string(),
            ),
            comment: TextField::new(transaction.comment.clone()),
            focus: FormField::Amount,
            error: None,
        }
    }

    fn set_focus(&mut self, focus: FormField) {
        self.focus = focus;
        match focus {
            FormField::Amount => self.amount.end(),
            FormField::Timestamp => self.date.end(),
            FormField::Comment => self.comment.end(),
            FormField::Type => {}
        }
    }

    fn toggle_kind(&mut self) {
        self.kind = self.kind.toggled();
    }

    fn handle_key(&mut self, key: KeyEvent) -> FormAction {
        match key.code {
            KeyCode::Esc => return FormAction::Cancel,
            KeyCode::Enter => return FormAction::Submit,
            KeyCode::Tab => {
                let next = self.focus.next();
                self.set_focus(next);
                return FormAction::None;
            }
            KeyCode::BackTab => {
                let previous = self.focus.previous();
                self.set_focus(previous);
                return FormAction::None;
            }
            KeyCode::Up => {
                let previous = self.focus.previous();
                self.set_focus(previous);
                return FormAction::None;
            }
            KeyCode::Down => {
                let next = self.focus.next();
                self.set_focus(next);
                return FormAction::None;
            }
            _ => {}
        }

        self.error = None;

        match self.focus {
            FormField::Type => match key.code {
                KeyCode::Char('h') | KeyCode::Char('l') | KeyCode::Char(' ') => {
                    self.toggle_kind();
                }
                KeyCode::Left | KeyCode::Right => self.toggle_kind(),
                _ => {}
            },
            FormField::Amount => match key.code {
                KeyCode::Char(character)
                    if character.is_ascii_digit() || matches!(character, ',' | '.') =>
                {
                    self.amount.insert(character, 20);
                }
                KeyCode::Backspace => self.amount.backspace(),
                KeyCode::Delete => self.amount.delete(),
                KeyCode::Left => self.amount.left(),
                KeyCode::Right => self.amount.right(),
                KeyCode::Home => self.amount.home(),
                KeyCode::End => self.amount.end(),
                _ => {}
            },
            FormField::Timestamp => match key.code {
                KeyCode::Char(character)
                    if character.is_ascii_digit() || matches!(character, '-' | ':' | ' ') =>
                {
                    self.date.insert(character, 16);
                }
                KeyCode::Backspace => self.date.backspace(),
                KeyCode::Delete => self.date.delete(),
                KeyCode::Left => self.date.left(),
                KeyCode::Right => self.date.right(),
                KeyCode::Home => self.date.home(),
                KeyCode::End => self.date.end(),
                _ => {}
            },
            FormField::Comment => match key.code {
                KeyCode::Char(character)
                    if !character.is_control() && self.comment.char_count() < MAX_COMMENT_CHARS =>
                {
                    self.comment.insert(character, MAX_COMMENT_CHARS);
                }
                KeyCode::Backspace => self.comment.backspace(),
                KeyCode::Delete => self.comment.delete(),
                KeyCode::Left => self.comment.left(),
                KeyCode::Right => self.comment.right(),
                KeyCode::Home => self.comment.home(),
                KeyCode::End => self.comment.end(),
                _ => {}
            },
        }

        FormAction::None
    }

    fn signed_amount(&self) -> Result<i64> {
        let cents = parse_amount(&self.amount.value).map_err(|error| anyhow!(error.to_string()))?;
        if cents <= 0 {
            bail!("Kwota w formularzu musi być większa od zera.");
        }
        Ok(match self.kind {
            Kind::Income => cents,
            Kind::Expense => -cents,
        })
    }

    fn parsed_timestamp(&self) -> Result<DateTime<FixedOffset>> {
        let naive = NaiveDateTime::parse_from_str(self.date.value.trim(), "%Y-%m-%d %H:%M")
            .map_err(|_| anyhow!("Nieprawidłowa data. Użyj formatu RRRR-MM-DD GG:MM."))?;
        Local
            .from_local_datetime(&naive)
            .earliest()
            .map(|timestamp| timestamp.fixed_offset())
            .ok_or_else(|| anyhow!("Data nie istnieje w lokalnej strefie czasowej."))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FormAction {
    None,
    Cancel,
    Submit,
}

#[derive(Debug, Clone)]
pub(crate) enum Mode {
    Normal,
    Add(FormState),
    Edit(FormState),
    ConfirmDelete { id: u64 },
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModeKind {
    Normal,
    Form,
    ConfirmDelete,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusKind {
    Info,
    Success,
    Error,
}

#[derive(Debug, Clone)]
pub(crate) struct StatusMessage {
    pub(crate) text: String,
    pub(crate) kind: StatusKind,
    pub(crate) expires_at: Instant,
}

pub(crate) struct App {
    pub(crate) service: BudgetService,
    pub(crate) ledger: Ledger,
    pub(crate) rows: Vec<TransactionRow>,
    pub(crate) selected_id: Option<u64>,
    pub(crate) filter: Filter,
    pub(crate) mode: Mode,
    pub(crate) status: Option<StatusMessage>,
    pub(crate) should_quit: bool,
    pub(crate) last_mtime: Option<SystemTime>,
    pub(crate) last_metadata_check: Instant,
    pub(crate) today: NaiveDate,
    pub(crate) page_size: usize,
    pub(crate) pending_delete: bool,
}

impl App {
    pub(crate) fn new(service: BudgetService, ledger: Ledger) -> Self {
        let rows = ledger.rows_newest_first();
        let selected_id = rows.first().map(|row| row.transaction.id);
        let last_mtime = service.store().last_modified();
        Self {
            service,
            ledger,
            rows,
            selected_id,
            filter: Filter::All,
            mode: Mode::Normal,
            status: None,
            should_quit: false,
            last_mtime,
            last_metadata_check: Instant::now(),
            today: Local::now().date_naive(),
            page_size: 10,
            pending_delete: false,
        }
    }

    pub(crate) fn run(&mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        loop {
            terminal.draw(|frame| ui::render(self, frame))?;
            if event::poll(Duration::from_millis(200))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => self.handle_key(key),
                    _ => {}
                }
            }
            self.tick();
            if self.should_quit {
                break;
            }
        }
        Ok(())
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => self.should_quit = true,
                KeyCode::Char('d') if self.mode_kind() == ModeKind::Normal => self.page_down(),
                KeyCode::Char('u') if self.mode_kind() == ModeKind::Normal => self.page_up(),
                _ => {}
            }
            return;
        }

        match self.mode_kind() {
            ModeKind::Normal => self.handle_normal_key(key),
            ModeKind::Form => self.handle_form_key(key),
            ModeKind::ConfirmDelete => self.handle_confirm_key(key),
            ModeKind::Help => self.handle_help_key(key),
        }
    }

    fn handle_normal_key(&mut self, key: KeyEvent) {
        if self.pending_delete && !matches!(key.code, KeyCode::Char('d')) {
            self.pending_delete = false;
        }

        match key.code {
            KeyCode::Char('q') => self.should_quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.select_next(),
            KeyCode::Char('k') | KeyCode::Up => self.select_previous(),
            KeyCode::Char('g') | KeyCode::Home => self.select_first(),
            KeyCode::Char('G') | KeyCode::End => self.select_last(),
            KeyCode::Char('a') => self.start_add(),
            KeyCode::Char('e') => self.start_edit(),
            KeyCode::Char('d') => self.handle_delete_prefix(),
            KeyCode::Char('f') => self.cycle_filter(),
            KeyCode::Char('r') => self.reload_from_disk(true),
            KeyCode::Char('?') => {
                self.pending_delete = false;
                self.mode = Mode::Help;
            }
            KeyCode::Esc => {
                self.pending_delete = false;
                self.status = None;
            }
            _ => {}
        }
    }

    fn handle_form_key(&mut self, key: KeyEvent) {
        let action = {
            let (Mode::Add(form) | Mode::Edit(form)) = &mut self.mode else {
                return;
            };
            form.handle_key(key)
        };

        match action {
            FormAction::None => {}
            FormAction::Cancel => self.mode = Mode::Normal,
            FormAction::Submit => self.submit_form(),
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => self.confirm_delete(),
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => self.mode = Mode::Normal,
            _ => {}
        }
    }

    fn handle_help_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?') => self.mode = Mode::Normal,
            _ => {}
        }
    }

    pub(crate) fn mode_kind(&self) -> ModeKind {
        match self.mode {
            Mode::Normal => ModeKind::Normal,
            Mode::Add(_) | Mode::Edit(_) => ModeKind::Form,
            Mode::ConfirmDelete { .. } => ModeKind::ConfirmDelete,
            Mode::Help => ModeKind::Help,
        }
    }

    pub(crate) fn visible_rows(&self) -> Vec<&TransactionRow> {
        self.rows
            .iter()
            .filter(|row| match self.filter {
                Filter::All => true,
                Filter::Income => row.transaction.is_income(),
                Filter::Expense => row.transaction.is_expense(),
            })
            .collect()
    }

    fn visible_ids(&self) -> Vec<u64> {
        self.visible_rows()
            .iter()
            .map(|row| row.transaction.id)
            .collect()
    }

    pub(crate) fn visible_selected_index(&self) -> Option<usize> {
        let selected_id = self.selected_id?;
        self.visible_rows()
            .iter()
            .position(|row| row.transaction.id == selected_id)
    }

    fn select_next(&mut self) {
        self.move_selection(1);
    }

    fn select_previous(&mut self) {
        self.move_selection(-1);
    }

    fn move_selection(&mut self, direction: isize) {
        let ids = self.visible_ids();
        if ids.is_empty() {
            self.selected_id = None;
            return;
        }
        let current = self
            .selected_id
            .and_then(|id| ids.iter().position(|candidate| *candidate == id))
            .unwrap_or(0) as isize;
        let next = (current + direction).clamp(0, (ids.len() - 1) as isize) as usize;
        self.selected_id = Some(ids[next]);
    }

    fn select_first(&mut self) {
        self.selected_id = self.visible_ids().first().copied();
    }

    fn select_last(&mut self) {
        self.selected_id = self.visible_ids().last().copied();
    }

    fn page_down(&mut self) {
        self.move_selection(self.page_size.max(1) as isize);
    }

    fn page_up(&mut self) {
        self.move_selection(-(self.page_size.max(1) as isize));
    }

    fn start_add(&mut self) {
        self.pending_delete = false;
        self.mode = Mode::Add(FormState::add());
    }

    fn start_edit(&mut self) {
        let Some(id) = self.selected_id else {
            self.set_status("Brak zaznaczonej transakcji.", StatusKind::Error);
            return;
        };
        let Some(transaction) = self.ledger.find_by_id(id) else {
            self.set_status("Nie znaleziono zaznaczonej transakcji.", StatusKind::Error);
            return;
        };
        self.pending_delete = false;
        self.mode = Mode::Edit(FormState::edit(transaction));
    }

    fn handle_delete_prefix(&mut self) {
        if self.selected_id.is_none() {
            self.pending_delete = false;
            self.set_status("Brak transakcji do usunięcia.", StatusKind::Error);
            return;
        }
        if self.pending_delete {
            self.pending_delete = false;
            if let Some(id) = self.selected_id {
                self.mode = Mode::ConfirmDelete { id };
            }
        } else {
            self.pending_delete = true;
            self.set_status(
                "Naciśnij d ponownie, aby usunąć zaznaczoną transakcję.",
                StatusKind::Info,
            );
        }
    }

    fn submit_form(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        let (mut form, is_add) = match mode {
            Mode::Add(form) => (form, true),
            Mode::Edit(form) => (form, false),
            other => {
                self.mode = other;
                return;
            }
        };

        let result = self.persist_form(&form);
        match result {
            Ok((transaction, ledger, message)) => {
                self.ledger = ledger;
                self.filter = Filter::All;
                self.refresh_rows();
                self.selected_id = Some(transaction.id);
                self.update_mtime();
                self.set_status(message, StatusKind::Success);
            }
            Err(error) => {
                form.error = Some(format!("{error:#}"));
                self.mode = if is_add {
                    Mode::Add(form)
                } else {
                    Mode::Edit(form)
                };
            }
        }
    }

    fn persist_form(&self, form: &FormState) -> Result<(Transaction, Ledger, String)> {
        let amount = form.signed_amount()?;
        let timestamp = form.parsed_timestamp()?;
        let comment = form.comment.value.clone();
        match form.mode {
            FormMode::Add => {
                let (transaction, ledger) = self.service.add(amount, &comment, timestamp)?;
                let message = format!("Dodano transakcję #{}.", transaction.id);
                Ok((transaction, ledger, message))
            }
            FormMode::Edit { id } => {
                let (transaction, ledger) =
                    self.service.update(id, amount, &comment, Some(timestamp))?;
                let message = format!("Zapisano transakcję #{}.", transaction.id);
                Ok((transaction, ledger, message))
            }
        }
    }

    fn confirm_delete(&mut self) {
        let mode = std::mem::replace(&mut self.mode, Mode::Normal);
        let Mode::ConfirmDelete { id } = mode else {
            self.mode = mode;
            return;
        };

        let visible_before = self.visible_ids();
        let position = visible_before
            .iter()
            .position(|candidate| *candidate == id)
            .unwrap_or(0);

        match self.service.remove_by_id(id) {
            Ok((_, ledger)) => {
                self.ledger = ledger;
                self.refresh_rows();
                let after = self.visible_ids();
                if after.is_empty() {
                    self.selected_id = None;
                } else {
                    let index = position.min(after.len() - 1);
                    self.selected_id = Some(after[index]);
                }
                self.update_mtime();
                self.set_status(format!("Usunięto transakcję #{id}."), StatusKind::Success);
            }
            Err(error) => {
                self.set_status(
                    format!("Nie udało się usunąć: {error:#}"),
                    StatusKind::Error,
                );
            }
        }
    }

    fn cycle_filter(&mut self) {
        self.filter = self.filter.next();
        self.selected_id = self.visible_ids().first().copied();
        self.set_status(format!("Filtr: {}.", self.filter.label()), StatusKind::Info);
    }

    fn refresh_rows(&mut self) {
        self.rows = self.ledger.rows_newest_first();
        let visible = self.visible_ids();
        if let Some(selected_id) = self.selected_id {
            if !visible.contains(&selected_id) {
                self.selected_id = visible.first().copied();
            }
        } else {
            self.selected_id = visible.first().copied();
        }
    }

    fn reload_from_disk(&mut self, show_status: bool) {
        match self.service.load() {
            Ok(ledger) => {
                let selected_id = self.selected_id;
                self.ledger = ledger;
                self.refresh_rows();
                if let Some(id) = selected_id
                    && self.visible_ids().contains(&id)
                {
                    self.selected_id = Some(id);
                }
                self.update_mtime();
                if show_status {
                    self.set_status("Odświeżono dane z pliku.", StatusKind::Info);
                }
            }
            Err(error) => {
                self.set_status(
                    format!("Nie udało się odświeżyć danych: {error:#}"),
                    StatusKind::Error,
                );
            }
        }
    }

    fn update_mtime(&mut self) {
        self.last_mtime = self.service.store().last_modified();
    }

    fn tick(&mut self) {
        self.today = Local::now().date_naive();

        if let Some(status) = &self.status
            && Instant::now() >= status.expires_at
        {
            self.status = None;
        }

        if self.last_metadata_check.elapsed() >= Duration::from_secs(1) {
            self.last_metadata_check = Instant::now();
            let current_mtime = self.service.store().last_modified();
            if current_mtime != self.last_mtime {
                match self.mode_kind() {
                    ModeKind::Normal | ModeKind::Help => {
                        self.last_mtime = current_mtime;
                        self.reload_from_disk(true);
                    }
                    ModeKind::Form | ModeKind::ConfirmDelete => {}
                }
            }
        }
    }

    fn set_status(&mut self, text: impl Into<String>, kind: StatusKind) {
        self.status = Some(StatusMessage {
            text: text.into(),
            kind,
            expires_at: Instant::now() + Duration::from_secs(4),
        });
    }

    pub(crate) fn file_path(&self) -> PathBuf {
        self.service.store().path().to_path_buf()
    }

    pub(crate) fn set_page_size(&mut self, size: usize) {
        self.page_size = size.max(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn added_form_has_valid_default_timestamp() {
        let form = FormState::add();
        assert!(form.parsed_timestamp().is_ok());
    }

    #[test]
    fn rejects_invalid_timestamp_text() {
        let mut form = FormState::add();
        form.date = TextField::new("2026-13-40 99:99");
        let error = form.parsed_timestamp().unwrap_err().to_string();
        assert!(error.contains("RRRR-MM-DD"));
    }
}
