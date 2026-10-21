use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table};
use serde_json::Value;

use crate::tui::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(8), // Balance
            Constraint::Min(5),    // Transactions
        ])
        .split(area);

    render_balance(f, chunks[0], app);
    render_transactions(f, chunks[1], app);
}

fn render_balance(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default().borders(Borders::ALL).title(" Wallet ");

    if let Some(ref wallet) = app.wallet_data {
        let address = wallet
            .get("address")
            .and_then(Value::as_str)
            .unwrap_or("unknown");
        let balance = wallet.get("balance").and_then(Value::as_f64).unwrap_or(0.0);
        let staking = wallet
            .get("staking")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let staking_amount = wallet.get("staking_amount").and_then(Value::as_f64);

        let mut lines = vec![
            Line::from(vec![
                Span::styled("Address: ", Style::default().fg(Color::DarkGray)),
                Span::styled(address, Style::default().fg(Color::Cyan)),
            ]),
            Line::from(vec![
                Span::styled("Balance: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.4} WRF", balance),
                    Style::default()
                        .fg(Color::Green)
                        .add_modifier(Modifier::BOLD),
                ),
            ]),
        ];

        if staking {
            let stake_str = staking_amount
                .map(|a| format!("{:.4} WRF", a))
                .unwrap_or_else(|| "active".to_string());
            lines.push(Line::from(vec![
                Span::styled("Staking: ", Style::default().fg(Color::DarkGray)),
                Span::styled(stake_str, Style::default().fg(Color::Yellow)),
            ]));
        }

        let paragraph = Paragraph::new(lines).block(block);
        f.render_widget(paragraph, area);
    } else if app.address.is_some() {
        let paragraph = Paragraph::new(Span::styled(
            "Wallet data unavailable",
            Style::default().fg(Color::Red),
        ))
        .block(block);
        f.render_widget(paragraph, area);
    } else {
        let paragraph = Paragraph::new(vec![
            Line::from(Span::styled(
                "No wallet configured",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Use --address to monitor a wallet",
                Style::default().fg(Color::DarkGray),
            )),
        ])
        .block(block);
        f.render_widget(paragraph, area);
    }
}

fn render_transactions(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Recent Transactions ");

    if let Some(ref txs) = app.wallet_transactions {
        let tx_list = txs.get("data").and_then(Value::as_array);
        if let Some(transactions) = tx_list {
            if transactions.is_empty() {
                let paragraph = Paragraph::new("No transactions found").block(block);
                f.render_widget(paragraph, area);
                return;
            }

            let header = Row::new(vec!["Type", "Amount", "Status", "From/To"]).style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            );

            let rows: Vec<Row> = transactions
                .iter()
                .take(10)
                .map(|tx| {
                    let tx_type = tx
                        .get("tx_type")
                        .and_then(Value::as_str)
                        .unwrap_or("transfer");
                    let amount = tx.get("amount").and_then(Value::as_f64).unwrap_or(0.0);
                    let status = tx
                        .get("status")
                        .and_then(Value::as_str)
                        .unwrap_or("pending");
                    let counterparty = tx.get("to").and_then(Value::as_str).unwrap_or("unknown");

                    let status_style = match status {
                        "confirmed" | "completed" => Style::default().fg(Color::Green),
                        "pending" => Style::default().fg(Color::Yellow),
                        _ => Style::default().fg(Color::Red),
                    };

                    Row::new(vec![
                        Cell::from(tx_type.to_string()),
                        Cell::from(format!("{:.4} WRF", amount)),
                        Cell::from(Span::styled(status, status_style)),
                        Cell::from(truncate_address(counterparty)),
                    ])
                })
                .collect();

            let widths = [
                Constraint::Percentage(15),
                Constraint::Percentage(25),
                Constraint::Percentage(20),
                Constraint::Percentage(40),
            ];
            let table = Table::new(rows, widths).header(header).block(block);
            f.render_widget(table, area);
        } else {
            let paragraph = Paragraph::new("No transaction data").block(block);
            f.render_widget(paragraph, area);
        }
    } else {
        let paragraph = Paragraph::new(Span::styled(
            "Transaction data unavailable",
            Style::default().fg(Color::DarkGray),
        ))
        .block(block);
        f.render_widget(paragraph, area);
    }
}

fn truncate_address(addr: &str) -> String {
    if addr.len() > 16 {
        format!("{}...{}", &addr[..8], &addr[addr.len() - 6..])
    } else {
        addr.to_string()
    }
}
