use chrono::{Datelike, NaiveDate};
use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Cell, Clear, HighlightSpacing, Paragraph, Row, Table, TableState,
};

use crate::money::format_cents;

use super::app::{App, FormField, FormMode, FormState, Kind, Mode, StatusKind, TextField};
use super::theme::Palette;

pub(super) fn render(app: &mut App, frame: &mut Frame) {
    let area = frame.area();
    let palette = Palette::classic();
    frame.render_widget(
        Block::new().style(Style::new().bg(palette.background).fg(palette.text)),
        area,
    );

    if area.width < 50 || area.height < 15 {
        let message = Paragraph::new("Terminal jest zbyt mały.\nPotrzebne jest co najmniej 50x14.")
            .alignment(Alignment::Center)
            .block(
                Block::bordered()
                    .border_type(BorderType::Rounded)
                    .border_style(Style::new().fg(palette.muted))
                    .title(Span::styled(" Budget UI ", Style::new().fg(palette.accent)))
                    .style(Style::new().bg(palette.background).fg(palette.text)),
            );
        frame.render_widget(message, area);
        return;
    }

    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(6),
        Constraint::Length(5),
    ])
    .split(area);

    render_header(app, frame, chunks[0], palette);
    render_table(app, frame, chunks[1], palette);
    render_footer(app, frame, chunks[2], palette);

    match &app.mode {
        Mode::Normal => {}
        Mode::Add(form) | Mode::Edit(form) => render_form(form, frame, area, palette),
        Mode::ConfirmDelete { id } => render_confirm(app, *id, frame, area, palette),
        Mode::Help => render_help(frame, area, palette),
    }
}

fn render_header(app: &App, frame: &mut Frame, area: Rect, palette: Palette) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.muted))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(Line::from(vec![
            Span::raw(" "),
            Span::styled(
                "Budget UI",
                Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
        ]));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let columns =
        Layout::horizontal([Constraint::Percentage(60), Constraint::Percentage(40)]).split(inner);

    let left = vec![
        Line::from(Span::styled(
            polish_date(app.today),
            Style::new().fg(palette.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(vec![
            Span::styled("Przychody ", Style::new().fg(palette.muted)),
            Span::styled(
                format_cents(app.ledger.income_total_cents(), true),
                Style::new().fg(palette.positive),
            ),
            Span::styled("   Wydatki ", Style::new().fg(palette.muted)),
            Span::styled(
                format_cents(app.ledger.expense_total_cents(), false),
                Style::new().fg(palette.negative),
            ),
        ]),
    ];
    frame.render_widget(Paragraph::new(left), columns[0]);

    let balance = app.ledger.total_balance_cents();
    let balance_color = if balance >= 0 {
        palette.positive
    } else {
        palette.negative
    };
    let right = vec![
        Line::from(vec![
            Span::styled("Saldo: ", Style::new().fg(palette.muted)),
            Span::styled(
                format_cents(balance, false),
                Style::new().fg(balance_color).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!("Wpisy: {}", app.ledger.len()),
            Style::new().fg(palette.muted),
        )),
    ];
    frame.render_widget(
        Paragraph::new(right).alignment(Alignment::Right),
        columns[1],
    );
}

fn render_table(app: &mut App, frame: &mut Frame, area: Rect, palette: Palette) {
    let title = Line::from(vec![
        Span::raw(" Transakcje: "),
        Span::styled(
            app.visible_rows().len().to_string(),
            Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
        ),
        Span::raw(" • filtr: "),
        Span::styled(app.filter.label(), Style::new().fg(palette.accent)),
        Span::raw(" "),
    ]);
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.muted))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(title);

    let inner = block.inner(area);
    app.set_page_size(inner.height.saturating_sub(1) as usize);

    let visible = app.visible_rows();
    if visible.is_empty() {
        frame.render_widget(block, area);
        render_empty_state(app, frame, inner, palette);
        return;
    }

    let header_style = Style::new().fg(palette.accent).add_modifier(Modifier::BOLD);
    let header = Row::new(vec![
        Cell::from("DATA"),
        Cell::from("TYP"),
        Cell::from(Line::from("KWOTA").right_aligned()),
        Cell::from(Line::from("SALDO PO").right_aligned()),
        Cell::from("KOMENTARZ"),
    ])
    .style(header_style)
    .height(1);

    let rows = visible.iter().map(|row| {
        let amount_style = if row.transaction.is_income() {
            Style::new().fg(palette.positive)
        } else {
            Style::new().fg(palette.negative)
        };
        Row::new(vec![
            Cell::from(Line::from(Span::styled(
                row.transaction.date_display(),
                Style::new().fg(palette.muted),
            ))),
            Cell::from(Line::from(Span::styled(
                row.transaction.kind_label(),
                amount_style,
            ))),
            Cell::from(
                Line::from(Span::styled(
                    row.transaction.amount_display(),
                    amount_style.add_modifier(Modifier::BOLD),
                ))
                .right_aligned(),
            ),
            Cell::from(
                Line::from(Span::styled(
                    format_cents(row.balance_after_cents, false),
                    Style::new().fg(palette.text),
                ))
                .right_aligned(),
            ),
            Cell::from(Line::from(Span::styled(
                row.transaction.comment.clone(),
                Style::new().fg(palette.text),
            ))),
        ])
    });

    let widths = [
        Constraint::Length(16),
        Constraint::Length(9),
        Constraint::Length(16),
        Constraint::Length(16),
        Constraint::Min(10),
    ];
    let table = Table::new(rows, widths)
        .header(header)
        .block(block)
        .column_spacing(1)
        .row_highlight_style(
            Style::new()
                .bg(palette.surface)
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("▸ ")
        .highlight_spacing(HighlightSpacing::Always);

    let mut state = TableState::default().with_selected(app.visible_selected_index());
    frame.render_stateful_widget(table, area, &mut state);
}

fn render_empty_state(app: &App, frame: &mut Frame, inner: Rect, palette: Palette) {
    let lines = if app.ledger.is_empty() {
        vec![
            Line::from(Span::styled(
                "Brak transakcji.",
                Style::new().fg(palette.text).add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled(
                "Naciśnij [a], aby dodać pierwszy przychód lub wydatek.",
                Style::new().fg(palette.muted),
            )),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                format!("Brak transakcji dla filtra: {}.", app.filter.label()),
                Style::new().fg(palette.text),
            )),
            Line::from(Span::styled(
                "Naciśnij [f], aby zmienić filtr.",
                Style::new().fg(palette.muted),
            )),
        ]
    };

    let height = 2.min(inner.height);
    let y = inner.y + inner.height.saturating_sub(height) / 2;
    let area = Rect::new(inner.x, y, inner.width, height);
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), area);
}

fn render_footer(app: &App, frame: &mut Frame, area: Rect, palette: Palette) {
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.muted))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(Span::styled(" Skróty ", Style::new().fg(palette.muted)));
    let inner = block.inner(area);
    frame.render_widget(block, area);

    let navigation = Line::from(vec![
        key("j/k", palette),
        hint(" ruch  ", palette),
        key("g/G", palette),
        hint(" skraj  ", palette),
        key("Ctrl+d/u", palette),
        hint(" strona  ", palette),
        key("a", palette),
        hint(" dodaj  ", palette),
        key("e", palette),
        hint(" edytuj  ", palette),
        key("dd", palette),
        hint(" usuń", palette),
    ]);
    let actions = Line::from(vec![
        key("f", palette),
        hint(" filtr  ", palette),
        key("r", palette),
        hint(" odśwież  ", palette),
        key("?", palette),
        hint(" pomoc  ", palette),
        key("q", palette),
        hint(" wyjście  ", palette),
        key("Esc", palette),
        hint(" anuluj", palette),
    ]);

    let (status_text, status_style) = match &app.status {
        Some(status) => (
            status.text.clone(),
            match status.kind {
                StatusKind::Info => Style::new().fg(palette.accent),
                StatusKind::Success => Style::new().fg(palette.positive),
                StatusKind::Error => Style::new().fg(palette.negative),
            },
        ),
        None => (
            format!("Plik: {}", app.file_path().display()),
            Style::new().fg(palette.muted),
        ),
    };

    let lines = vec![
        navigation,
        actions,
        Line::from(Span::styled(status_text, status_style)),
    ];
    frame.render_widget(Paragraph::new(lines), inner);
}

fn render_form(form: &FormState, frame: &mut Frame, area: Rect, palette: Palette) {
    let popup = popup_area(area, 78, 17);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::new().style(Style::new().bg(palette.background).fg(palette.text)),
        popup,
    );

    let title = match form.mode {
        FormMode::Add => " Nowa transakcja ".to_string(),
        FormMode::Edit { id } => format!(" Edycja transakcji #{id} "),
    };
    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.accent))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(Span::styled(
            title,
            Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = vec![
        type_line(form, palette),
        Line::default(),
        field_line(
            "Kwota",
            &form.amount,
            form.focus == FormField::Amount,
            48,
            true,
            palette,
        ),
        Line::default(),
        field_line(
            "Data",
            &form.date,
            form.focus == FormField::Timestamp,
            16,
            false,
            palette,
        ),
        Line::default(),
        field_line(
            "Komentarz",
            &form.comment,
            form.focus == FormField::Comment,
            48,
            false,
            palette,
        ),
        Line::default(),
    ];

    lines.push(match &form.error {
        Some(error) => Line::from(Span::styled(
            error.clone(),
            Style::new()
                .fg(palette.negative)
                .add_modifier(Modifier::BOLD),
        )),
        None => Line::from(Span::styled(
            "Data: RRRR-MM-DD GG:MM. Typ: h/l lub strzałki.",
            Style::new().fg(palette.muted),
        )),
    });
    lines.push(Line::from(Span::styled(
        "Tab/Shift+Tab pole  Enter zapisz  Esc anuluj",
        Style::new().fg(palette.muted),
    )));

    frame.render_widget(Paragraph::new(lines), inner);
}

fn type_line(form: &FormState, palette: Palette) -> Line<'static> {
    let income_style = if form.kind == Kind::Income {
        Style::new()
            .fg(palette.contrast)
            .bg(palette.positive)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(palette.muted)
    };
    let expense_style = if form.kind == Kind::Expense {
        Style::new()
            .fg(palette.contrast)
            .bg(palette.negative)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(palette.muted)
    };
    Line::from(vec![
        Span::styled(
            format!("{:<12}", "Typ"),
            field_label_style(form.focus == FormField::Type, palette),
        ),
        Span::styled(" Przychód ", income_style),
        Span::raw("  "),
        Span::styled(" Wydatek ", expense_style),
    ])
}

fn field_line(
    label: &str,
    field: &TextField,
    focused: bool,
    max_chars: usize,
    currency_suffix: bool,
    palette: Palette,
) -> Line<'static> {
    let mut spans = vec![Span::styled(
        format!("{label:<12}"),
        field_label_style(focused, palette),
    )];
    spans.extend(field_value_spans(field, focused, max_chars, palette));
    if currency_suffix {
        spans.push(Span::styled(" zł", Style::new().fg(palette.muted)));
    }
    Line::from(spans)
}

fn field_value_spans(
    field: &TextField,
    focused: bool,
    max_chars: usize,
    palette: Palette,
) -> Vec<Span<'static>> {
    let characters: Vec<char> = field.value.chars().collect();
    if !focused {
        let mut text: String = characters.iter().take(max_chars).collect();
        if characters.len() > max_chars {
            text.push('…');
        }
        return vec![Span::styled(text, Style::new().fg(palette.text))];
    }

    let visible = max_chars.max(1);
    let start = if field.cursor >= visible {
        field.cursor + 1 - visible
    } else {
        0
    };
    let end = (start + visible).min(characters.len());
    let before: String = characters[start..field.cursor.min(characters.len())]
        .iter()
        .collect();
    let cursor_character = if field.cursor < characters.len() {
        characters[field.cursor].to_string()
    } else {
        " ".to_string()
    };
    let after: String = if field.cursor < characters.len() {
        characters[(field.cursor + 1).min(end)..end]
            .iter()
            .collect()
    } else {
        String::new()
    };

    let mut spans = Vec::new();
    if start > 0 {
        spans.push(Span::styled("…", Style::new().fg(palette.muted)));
    }
    if !before.is_empty() {
        spans.push(Span::styled(before, Style::new().fg(palette.text)));
    }
    spans.push(Span::styled(
        cursor_character,
        Style::new().fg(palette.contrast).bg(palette.accent),
    ));
    if !after.is_empty() {
        spans.push(Span::styled(after, Style::new().fg(palette.text)));
    }
    spans
}

fn field_label_style(focused: bool, palette: Palette) -> Style {
    if focused {
        Style::new().fg(palette.accent).add_modifier(Modifier::BOLD)
    } else {
        Style::new().fg(palette.muted)
    }
}

fn render_confirm(app: &App, id: u64, frame: &mut Frame, area: Rect, palette: Palette) {
    let popup = popup_area(area, 62, 9);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::new().style(Style::new().bg(palette.background).fg(palette.text)),
        popup,
    );

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.negative))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(Span::styled(
            " Potwierdzenie usunięcia ",
            Style::new()
                .fg(palette.negative)
                .add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let transaction_text = app
        .ledger
        .find_by_id(id)
        .map(|transaction| {
            if transaction.comment.is_empty() {
                transaction.amount_display()
            } else {
                format!(
                    "{}  — {}",
                    transaction.amount_display(),
                    transaction.comment
                )
            }
        })
        .unwrap_or_else(|| format!("transakcja #{id}"));

    let lines = vec![
        Line::from(Span::styled(
            format!("Czy usunąć transakcję #{id}?"),
            Style::new().fg(palette.text).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            transaction_text,
            Style::new().fg(palette.muted),
        )),
        Line::default(),
        Line::from(vec![
            key("y", palette),
            hint(" / Enter — usuń    ", palette),
            key("n", palette),
            hint(" / Esc — anuluj", palette),
        ]),
    ];
    frame.render_widget(Paragraph::new(lines).alignment(Alignment::Center), inner);
}

fn render_help(frame: &mut Frame, area: Rect, palette: Palette) {
    let popup = popup_area(area, 74, 22);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Block::new().style(Style::new().bg(palette.background).fg(palette.text)),
        popup,
    );

    let block = Block::bordered()
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(palette.accent))
        .style(Style::new().bg(palette.background).fg(palette.text))
        .title(Span::styled(
            " Pomoc ",
            Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
        ));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);

    let mut lines = vec![
        help_section("Nawigacja", palette),
        help_line("j / k", "następny / poprzedni wpis", palette),
        help_line("g / G", "pierwszy / ostatni wpis", palette),
        help_line("Ctrl+d / Ctrl+u", "pół strony w dół / w górę", palette),
        Line::default(),
        help_section("Akcje", palette),
        help_line("a", " dodaj transakcję", palette),
        help_line("e", "edytuj zaznaczoną transakcję", palette),
        help_line("dd", "usuń zaznaczoną (potem y/n)", palette),
        help_line("f", "zmień filtr: wszystkie/przychody/wydatki", palette),
        help_line("r", "odśwież dane z pliku", palette),
        Line::default(),
        help_section("Formularz", palette),
        help_line("Tab / Shift+Tab", "zmiana pola", palette),
        help_line("h / l", "zmiana typu transakcji", palette),
        help_line("Data", "format RRRR-MM-DD GG:MM", palette),
        help_line("Enter / Esc", "zapisz / anuluj", palette),
        Line::default(),
        help_line("? / q / Esc", "zamknij pomoc", palette),
    ];
    lines.truncate(inner.height as usize);
    frame.render_widget(Paragraph::new(lines), inner);
}

fn help_section(title: &'static str, palette: Palette) -> Line<'static> {
    Line::from(Span::styled(
        title,
        Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
    ))
}

fn help_line(keys: &'static str, description: &'static str, palette: Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{keys:<18}"), Style::new().fg(palette.accent)),
        Span::styled(description, Style::new().fg(palette.text)),
    ])
}

fn key(label: &'static str, palette: Palette) -> Span<'static> {
    Span::styled(
        format!("[{label}]"),
        Style::new().fg(palette.accent).add_modifier(Modifier::BOLD),
    )
}

fn hint(text: &'static str, palette: Palette) -> Span<'static> {
    Span::styled(text, Style::new().fg(palette.muted))
}

fn popup_area(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2)).max(10);
    let height = height.min(area.height.saturating_sub(2)).max(5);
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width, height)
}

fn polish_date(date: NaiveDate) -> String {
    const WEEKDAYS: [&str; 7] = [
        "poniedziałek",
        "wtorek",
        "środa",
        "czwartek",
        "piątek",
        "sobota",
        "niedziela",
    ];
    const MONTHS: [&str; 12] = [
        "stycznia",
        "lutego",
        "marca",
        "kwietnia",
        "maja",
        "czerwca",
        "lipca",
        "sierpnia",
        "września",
        "października",
        "listopada",
        "grudnia",
    ];
    let weekday = WEEKDAYS[date.weekday().num_days_from_monday() as usize];
    let month = MONTHS[date.month0() as usize];
    format!("{weekday}, {} {month} {}", date.day(), date.year())
}
