use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Paragraph};
use serde_json::Value;

use crate::tui::app::App;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let text = if let Some(ref data) = app.weather_data {
        let mut lines = Vec::new();

        if let Some(station_id) = data.get("stationId").and_then(Value::as_str) {
            lines.push(Line::from(vec![
                Span::styled("Station: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    station_id,
                    Style::default()
                        .fg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if let Some(readings) = data
            .get("readings")
            .or(data.get("sensorData"))
            .and_then(Value::as_array)
        {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Latest Readings:",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )));

            for reading in readings.iter().take(10) {
                let sensor_type = reading
                    .get("type")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let value = reading.get("value").and_then(Value::as_f64).unwrap_or(0.0);
                let unit = reading.get("unit").and_then(Value::as_str).unwrap_or("");

                lines.push(Line::from(vec![
                    Span::styled(
                        format!("  {}: ", sensor_type),
                        Style::default().fg(Color::DarkGray),
                    ),
                    Span::styled(
                        format!("{:.1} {}", value, unit),
                        Style::default().fg(Color::Green),
                    ),
                ]));
            }
        }

        if let Some(quality) = data.get("qualityScore").and_then(Value::as_f64) {
            lines.push(Line::from(""));
            let quality_color = if quality >= 90.0 {
                Color::Green
            } else if quality >= 70.0 {
                Color::Yellow
            } else {
                Color::Red
            };
            lines.push(Line::from(vec![
                Span::styled("Quality Score: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.1}", quality),
                    Style::default()
                        .fg(quality_color)
                        .add_modifier(Modifier::BOLD),
                ),
            ]));
        }

        if let Some(earnings) = data.get("totalEarnings").and_then(Value::as_f64) {
            lines.push(Line::from(vec![
                Span::styled("Total Earnings: ", Style::default().fg(Color::DarkGray)),
                Span::styled(
                    format!("{:.4} WRF", earnings),
                    Style::default().fg(Color::Yellow),
                ),
            ]));
        }

        lines
    } else if app.station_id.is_some() {
        vec![Line::from(Span::styled(
            "Weather data unavailable",
            Style::default().fg(Color::Red),
        ))]
    } else {
        vec![
            Line::from(Span::styled(
                "No station configured",
                Style::default().fg(Color::DarkGray),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "Use --station-id to monitor a weather station",
                Style::default().fg(Color::DarkGray),
            )),
        ]
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Weather Station ");
    let paragraph = Paragraph::new(text).block(block);
    f.render_widget(paragraph, area);
}
