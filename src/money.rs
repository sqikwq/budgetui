use std::error::Error;
use std::fmt;

pub const MAX_AMOUNT_CENTS: i64 = 999_999_999_999;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoneyError {
    Empty,
    Invalid,
    Zero,
    TooLarge,
    TooManyDecimals,
}

impl fmt::Display for MoneyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "Kwota nie może być pusta."),
            Self::Invalid => {
                write!(f, "Nieprawidłowa kwota. Użyj np. 100, 100,50 lub -100.")
            }
            Self::Zero => write!(f, "Kwota nie może wynosić zero."),
            Self::TooLarge => write!(f, "Kwota jest zbyt duża."),
            Self::TooManyDecimals => write!(f, "Maksymalnie dwa miejsca po przecinku."),
        }
    }
}

impl Error for MoneyError {}

pub fn parse_amount(input: &str) -> Result<i64, MoneyError> {
    let compact: String = input
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '\u{00a0}')
        .collect();

    if compact.is_empty() {
        return Err(MoneyError::Empty);
    }

    let lower = compact.to_lowercase();
    let mut text = compact.as_str();
    for suffix in ["zł", "zl", "pln"] {
        if lower.ends_with(suffix) {
            let byte_len = text
                .len()
                .checked_sub(suffix.len())
                .ok_or(MoneyError::Invalid)?;
            text = &text[..byte_len];
            break;
        }
    }

    if text.is_empty() {
        return Err(MoneyError::Invalid);
    }

    let (sign, digits) = if let Some(rest) = text.strip_prefix('-') {
        (-1_i64, rest)
    } else if let Some(rest) = text.strip_prefix('+') {
        (1_i64, rest)
    } else {
        (1_i64, text)
    };

    if digits.is_empty() {
        return Err(MoneyError::Invalid);
    }

    let normalized = digits.replace(',', ".");
    let mut parts = normalized.split('.');
    let whole = parts.next().unwrap_or_default();
    let fractional = parts.next();
    if parts.next().is_some() {
        return Err(MoneyError::Invalid);
    }

    if !whole.chars().all(|c| c.is_ascii_digit()) {
        return Err(MoneyError::Invalid);
    }
    if whole.is_empty() && fractional.is_none() {
        return Err(MoneyError::Invalid);
    }

    let whole_value: i128 = if whole.is_empty() {
        0
    } else {
        whole.parse().map_err(|_| MoneyError::TooLarge)?
    };

    let fractional_value = match fractional {
        Some(value) => {
            if value.is_empty() || !value.chars().all(|c| c.is_ascii_digit()) {
                return Err(MoneyError::Invalid);
            }
            if value.len() > 2 {
                return Err(MoneyError::TooManyDecimals);
            }
            let padded = format!("{value:0<2}");
            padded.parse::<i128>().map_err(|_| MoneyError::Invalid)?
        }
        None => 0,
    };

    let total = whole_value
        .checked_mul(100)
        .and_then(|value| value.checked_add(fractional_value))
        .ok_or(MoneyError::TooLarge)?;

    if total == 0 {
        return Err(MoneyError::Zero);
    }
    if total > i128::from(MAX_AMOUNT_CENTS) {
        return Err(MoneyError::TooLarge);
    }

    let signed = total
        .checked_mul(i128::from(sign))
        .ok_or(MoneyError::TooLarge)?;
    i64::try_from(signed).map_err(|_| MoneyError::TooLarge)
}

pub fn format_cents(cents: i64, show_plus: bool) -> String {
    let negative = cents < 0;
    let absolute = (i128::from(cents)).abs();
    let zloty = absolute / 100;
    let grosze = absolute % 100;
    let sign = if negative {
        "-"
    } else if show_plus && absolute > 0 {
        "+"
    } else {
        ""
    };
    format!(
        "{sign}{},{} zł",
        group_thousands(zloty),
        format_args!("{grosze:02}")
    )
}

pub fn format_plain(cents: i64) -> String {
    let negative = cents < 0;
    let absolute = (i128::from(cents)).abs();
    let sign = if negative { "-" } else { "" };
    format!("{sign}{}.{:02}", absolute / 100, absolute % 100)
}

pub fn format_positive_plain(cents: i64) -> String {
    let absolute = (i128::from(cents)).abs();
    format!("{}.{:02}", absolute / 100, absolute % 100)
}

fn group_thousands(value: i128) -> String {
    let raw = value.to_string();
    let mut reversed = String::with_capacity(raw.len() + raw.len() / 3);
    for (index, character) in raw.chars().rev().enumerate() {
        if index > 0 && index % 3 == 0 {
            reversed.push(' ');
        }
        reversed.push(character);
    }
    reversed.chars().rev().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_supported_amounts() {
        assert_eq!(parse_amount("100"), Ok(10_000));
        assert_eq!(parse_amount("+100"), Ok(10_000));
        assert_eq!(parse_amount("-100"), Ok(-10_000));
        assert_eq!(parse_amount("100,50"), Ok(10_050));
        assert_eq!(parse_amount("100.5"), Ok(10_050));
        assert_eq!(parse_amount(".99"), Ok(99));
        assert_eq!(parse_amount("1 234,56 zł"), Ok(123_456));
        assert_eq!(parse_amount("100PLN"), Ok(10_000));
    }

    #[test]
    fn rejects_invalid_amounts() {
        assert_eq!(parse_amount(""), Err(MoneyError::Empty));
        assert_eq!(parse_amount("abc"), Err(MoneyError::Invalid));
        assert_eq!(parse_amount("0"), Err(MoneyError::Zero));
        assert_eq!(parse_amount("-0,00"), Err(MoneyError::Zero));
        assert_eq!(parse_amount("1.234"), Err(MoneyError::TooManyDecimals));
        assert_eq!(parse_amount("1,2,3"), Err(MoneyError::Invalid));
        assert_eq!(parse_amount("100."), Err(MoneyError::Invalid));
    }

    #[test]
    fn formats_money_with_polish_separators() {
        assert_eq!(format_cents(123_456, true), "+1 234,56 zł");
        assert_eq!(format_cents(-123_456, true), "-1 234,56 zł");
        assert_eq!(format_cents(123_456, false), "1 234,56 zł");
        assert_eq!(format_cents(0, false), "0,00 zł");
        assert_eq!(format_cents(99, true), "+0,99 zł");
    }

    #[test]
    fn formats_plain_values_for_csv_and_editing() {
        assert_eq!(format_plain(123_456), "1234.56");
        assert_eq!(format_plain(-100), "-1.00");
        assert_eq!(format_positive_plain(-123_456), "1234.56");
    }
}
