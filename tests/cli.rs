use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_path() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let counter = COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!(
        "budgetui-cli-{}-{nonce}-{counter}.csv",
        std::process::id()
    ))
}

fn run(path: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_budgetui"))
        .env("BUDGETUI_FILE", path)
        .args(args)
        .output()
        .unwrap()
}

#[test]
fn add_list_and_remove_transaction() {
    let path = temp_path();
    let add = run(&path, &["add", "100", "dodatek"]);
    assert!(add.status.success());
    let stdout = String::from_utf8_lossy(&add.stdout);
    assert!(stdout.contains("Dodano #1"));
    assert!(stdout.contains("+100,00 zł"));

    let expense = run(&path, &["add", "-100", "wydatek"]);
    assert!(expense.status.success());

    let list = run(&path, &["ls"]);
    assert!(list.status.success());
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(stdout.contains("PRZYCHÓD"));
    assert!(stdout.contains("WYDATEK"));
    assert!(stdout.contains("Balans: 0,00 zł"));

    let remove = run(&path, &["rm", "100", "dodatek"]);
    assert!(remove.status.success());
    let stdout = String::from_utf8_lossy(&remove.stdout);
    assert!(stdout.contains("Usunięto #1"));

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("wydatek"));
    assert!(!content.contains("dodatek"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn ambiguous_remove_fails_without_changing_file() {
    let path = temp_path();
    assert!(run(&path, &["add", "100", "dodatek"]).status.success());
    assert!(run(&path, &["add", "-100", "wydatek"]).status.success());
    let before = std::fs::read_to_string(&path).unwrap();

    let remove = run(&path, &["rm", "100"]);
    assert_eq!(remove.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&remove.stderr);
    assert!(stderr.contains("Nie można jednoznacznie"));
    assert!(stderr.contains("--id"));

    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(before, after);
    let _ = std::fs::remove_file(path);
}

#[test]
fn remove_by_id_works() {
    let path = temp_path();
    assert!(run(&path, &["add", "100", "pierwszy"]).status.success());
    assert!(run(&path, &["add", "200", "drugi"]).status.success());

    let remove = run(&path, &["rm", "--id", "1"]);
    assert!(remove.status.success());
    let list = run(&path, &["ls"]);
    let stdout = String::from_utf8_lossy(&list.stdout);
    assert!(!stdout.contains("pierwszy"));
    assert!(stdout.contains("drugi"));
    let _ = std::fs::remove_file(path);
}

#[test]
fn usage_errors_exit_with_code_two() {
    let path = temp_path();
    let output = run(&path, &["add", "abc"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Nieprawidłowa kwota"));
    let _ = std::fs::remove_file(path);
}
