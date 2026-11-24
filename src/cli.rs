use std::error::Error;
use std::fmt;
use std::io::{self, IsTerminal};
use std::process::ExitCode;

use anyhow::anyhow;

use crate::APP_NAME;
use crate::model::Ledger;
use crate::money::{format_cents, parse_amount};
use crate::service::{BudgetService, RemoveOutcome, RemoveSelector, now};

#[derive(Debug)]
pub enum CliError {
    Usage(String),
    Runtime(anyhow::Error),
}

impl CliError {
    pub fn code(&self) -> u8 {
        match self {
            Self::Usage(_) => 2,
            Self::Runtime(_) => 1,
        }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Usage(message) => write!(f, "Błąd: {message}\n\n{}", usage_text()),
            Self::Runtime(error) => write!(f, "Błąd: {error:#}"),
        }
    }
}

impl Error for CliError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Usage(_) => None,
            Self::Runtime(error) => Some(error.as_ref()),
        }
    }
}

impl From<anyhow::Error> for CliError {
    fn from(value: anyhow::Error) -> Self {
        Self::Runtime(value)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Tui,
    Add {
        amount_cents: i64,
        comment: String,
    },
    RemoveMatch {
        amount_cents: i64,
        signed: bool,
        comment: String,
    },
    RemoveId {
        id: u64,
    },
    List,
    Help,
    Version,
}

pub fn main() -> ExitCode {
    let result = collect_args().and_then(|args| run(&args));
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::from(error.code())
        }
    }
}

fn collect_args() -> Result<Vec<String>, CliError> {
    std::env::args_os()
        .skip(1)
        .map(|argument| {
            argument.into_string().map_err(|_| {
                CliError::Usage("Argumenty muszą być poprawnym tekstem UTF-8.".to_string())
            })
        })
        .collect()
}

pub fn parse_args(args: &[String]) -> Result<Command, CliError> {
    let Some(operation) = args.first().map(String::as_str) else {
        return Ok(Command::Tui);
    };

    match operation {
        "-h" | "--help" | "help" => Ok(Command::Help),
        "-V" | "--version" => Ok(Command::Version),
        "add" => {
            let amount_text = args
                .get(1)
                .ok_or_else(|| CliError::Usage("Brak kwoty dla operacji add.".to_string()))?;
            let amount_cents =
                parse_amount(amount_text).map_err(|error| CliError::Usage(error.to_string()))?;
            Ok(Command::Add {
                amount_cents,
                comment: args[2..].join(" "),
            })
        }
        "rm" => {
            if args.get(1).map(String::as_str) == Some("--id") {
                let id_text = args
                    .get(2)
                    .ok_or_else(|| CliError::Usage("Brak ID po --id.".to_string()))?;
                let id = id_text.parse::<u64>().map_err(|_| {
                    CliError::Usage(format!("Nieprawidłowe ID transakcji: {id_text}."))
                })?;
                if id == 0 {
                    return Err(CliError::Usage("ID musi być większe od zera.".to_string()));
                }
                if args.len() > 3 {
                    return Err(CliError::Usage(
                        "Operacja rm --id przyjmuje tylko jeden identyfikator.".to_string(),
                    ));
                }
                return Ok(Command::RemoveId { id });
            }

            let amount_text = args
                .get(1)
                .ok_or_else(|| CliError::Usage("Brak kwoty dla operacji rm.".to_string()))?;
            let trimmed_amount = amount_text.trim_start();
            let signed = trimmed_amount.starts_with('+') || trimmed_amount.starts_with('-');
            let amount_cents =
                parse_amount(amount_text).map_err(|error| CliError::Usage(error.to_string()))?;
            Ok(Command::RemoveMatch {
                amount_cents,
                signed,
                comment: args[2..].join(" "),
            })
        }
        "ls" | "list" => {
            if args.len() > 1 {
                return Err(CliError::Usage(
                    "Operacja ls nie przyjmuje dodatkowych argumentów.".to_string(),
                ));
            }
            Ok(Command::List)
        }
        other => Err(CliError::Usage(format!("Nieznana operacja: {other}."))),
    }
}

pub fn run(args: &[String]) -> Result<u8, CliError> {
    match parse_args(args)? {
        Command::Help => {
            println!("{}", usage_text());
            Ok(0)
        }
        Command::Version => {
            println!("{APP_NAME} {}", env!("CARGO_PKG_VERSION"));
            Ok(0)
        }
        Command::Tui => {
            if !io::stdout().is_terminal() {
                return Err(CliError::Usage(
                    "TUI wymaga interaktywnego terminala. Użyj add, rm lub ls.".to_string(),
                ));
            }
            crate::tui::run()?;
            Ok(0)
        }
        Command::List => {
            let service = BudgetService::from_default();
            let ledger = service.load()?;
            print_ledger(&ledger);
            Ok(0)
        }
        Command::Add {
            amount_cents,
            comment,
        } => {
            let service = BudgetService::from_default();
            let (transaction, ledger) = service.add(amount_cents, &comment, now())?;
            println!(
                "Dodano #{}: {}{}",
                transaction.id,
                transaction.amount_display(),
                comment_suffix(&transaction.comment)
            );
            println!(
                "Saldo: {}",
                format_cents(ledger.total_balance_cents(), false)
            );
            Ok(0)
        }
        Command::RemoveId { id } => {
            let service = BudgetService::from_default();
            let (removed, ledger) = service.remove_by_id(id)?;
            println!(
                "Usunięto #{}: {}{}",
                removed.id,
                removed.amount_display(),
                comment_suffix(&removed.comment)
            );
            println!(
                "Saldo: {}",
                format_cents(ledger.total_balance_cents(), false)
            );
            Ok(0)
        }
        Command::RemoveMatch {
            amount_cents,
            signed,
            comment,
        } => {
            let service = BudgetService::from_default();
            let outcome = service.remove_matching(&RemoveSelector {
                amount_cents,
                signed,
                comment,
            })?;
            match outcome {
                RemoveOutcome::Removed { removed, ledger } => {
                    println!(
                        "Usunięto #{}: {}{}",
                        removed.id,
                        removed.amount_display(),
                        comment_suffix(&removed.comment)
                    );
                    println!(
                        "Saldo: {}",
                        format_cents(ledger.total_balance_cents(), false)
                    );
                    Ok(0)
                }
                RemoveOutcome::NotFound => Err(CliError::Runtime(anyhow!(
                    "Nie znaleziono transakcji pasującej do podanej kwoty i komentarza."
                ))),
                RemoveOutcome::Ambiguous { candidates } => Err(CliError::Runtime(anyhow!(
                    "Nie można jednoznacznie usunąć transakcji. Pasujące wpisy:\n{}\nUżyj: {APP_NAME} rm --id <id>",
                    format_candidates(&candidates)
                ))),
            }
        }
    }
}

fn print_ledger(ledger: &Ledger) {
    if ledger.is_empty() {
        println!("Brak transakcji.");
        println!("Dodaj pierwszą komendą: {APP_NAME} add <kwota> [komentarz]");
        return;
    }

    println!(
        "{:<5} {:<16} {:<9} {:>16} {:>16}  KOMENTARZ",
        "ID", "DATA", "TYP", "KWOTA", "SALDO PO"
    );
    for row in ledger.rows_newest_first() {
        println!(
            "{:<5} {:<16} {:<9} {:>16} {:>16}  {}",
            row.transaction.id,
            row.transaction.date_display(),
            row.transaction.kind_label(),
            row.transaction.amount_display(),
            format_cents(row.balance_after_cents, false),
            row.transaction.comment
        );
    }
    println!();
    println!(
        "Przychody: {} | Wydatki: {} | Balans: {}",
        format_cents(ledger.income_total_cents(), true),
        format_cents(ledger.expense_total_cents(), false),
        format_cents(ledger.total_balance_cents(), false)
    );
}

fn format_candidates(candidates: &[crate::model::Transaction]) -> String {
    candidates
        .iter()
        .map(|transaction| {
            format!(
                "  #{} {} {}{}",
                transaction.id,
                transaction.date_display(),
                transaction.amount_display(),
                comment_suffix(&transaction.comment)
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn comment_suffix(comment: &str) -> String {
    if comment.is_empty() {
        String::new()
    } else {
        format!(" — {comment}")
    }
}

pub fn usage_text() -> String {
    format!(
        "Budget UI — terminalowy budżet przychodów i wydatków\n\
\n\
Użycie:\n\
  {APP_NAME}                            uruchom TUI\n\
  {APP_NAME} add <kwota> [komentarz...]  dodaj transakcję\n\
  {APP_NAME} rm <kwota> [komentarz...]   usuń jednoznaczną transakcję\n\
  {APP_NAME} rm --id <id>                usuń transakcję po ID\n\
  {APP_NAME} ls                          pokaż transakcje\n\
  {APP_NAME} --help                      pokaż tę pomoc\n\
  {APP_NAME} --version                   pokaż wersję\n\
\n\
Kwota dodatnia oznacza przychód, a ujemna wydatek.\n\
Przykłady:\n\
  {APP_NAME} add 5000 wynagrodzenie\n\
  {APP_NAME} add -129,99 zakupy\n\
  {APP_NAME} rm 5000 wynagrodzenie\n\
\n\
Dane są przechowywane w pliku {} (zmienna BUDGETUI_FILE może go zmienić).",
        crate::store::DEFAULT_FILE
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn parses_no_arguments_as_tui() {
        assert_eq!(parse_args(&[]).unwrap(), Command::Tui);
    }

    #[test]
    fn parses_add_with_comment() {
        assert_eq!(
            parse_args(&args(&["add", "100", "dodatek", "miesięczny"])).unwrap(),
            Command::Add {
                amount_cents: 10_000,
                comment: "dodatek miesięczny".to_string(),
            }
        );
    }

    #[test]
    fn parses_remove_with_sign_and_without() {
        assert_eq!(
            parse_args(&args(&["rm", "-100", "wydatek"])).unwrap(),
            Command::RemoveMatch {
                amount_cents: -10_000,
                signed: true,
                comment: "wydatek".to_string(),
            }
        );
        assert_eq!(
            parse_args(&args(&["rm", "100", "wydatek"])).unwrap(),
            Command::RemoveMatch {
                amount_cents: 10_000,
                signed: false,
                comment: "wydatek".to_string(),
            }
        );
    }

    #[test]
    fn parses_remove_by_id() {
        assert_eq!(
            parse_args(&args(&["rm", "--id", "42"])).unwrap(),
            Command::RemoveId { id: 42 }
        );
    }

    #[test]
    fn rejects_bad_arguments() {
        assert!(parse_args(&args(&["add"])).is_err());
        assert!(parse_args(&args(&["add", "abc"])).is_err());
        assert!(parse_args(&args(&["rm", "--id", "0"])).is_err());
        assert!(parse_args(&args(&["ls", "extra"])).is_err());
        assert!(parse_args(&args(&["unknown"])).is_err());
    }
}
