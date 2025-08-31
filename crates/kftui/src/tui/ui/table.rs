use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use ratatui::prelude::Alignment;
use ratatui::widgets::BorderType;
use ratatui::widgets::TableState;
use ratatui::{
    Frame,
    layout::{
        Constraint,
        Rect,
    },
    style::{
        Color,
        Modifier,
        Style,
    },
    text::{
        Line,
        Span,
        Text,
    },
    widgets::{
        Block,
        Borders,
        Cell,
        Paragraph,
        Row,
        Scrollbar,
        ScrollbarOrientation,
        ScrollbarState,
        Table,
    },
};

use crate::tui::input::App;
use crate::tui::ui::{
    BASE,
    GREEN,
    MAUVE,
    RED,
    SUBTEXT0,
    SURFACE0,
    SURFACE1,
    SURFACE2,
    TEXT,
    YELLOW,
};

#[allow(clippy::too_many_arguments)]
pub fn draw_configs_table(
    frame: &mut Frame, area: Rect, configs: &[Config], config_states: &[ConfigState],
    state: &mut TableState, title: &str, has_focus: bool, selected_rows: &HashSet<usize>,
    configs_being_processed: &std::collections::HashMap<i64, (Arc<AtomicBool>, std::time::Instant)>,
    throbber_state: &throbber_widgets_tui::ThrobberState,
) {
    let rows: Vec<Row> = configs
        .iter()
        .enumerate()
        .map(|(i, config)| {
            let state = config_states
                .iter()
                .find(|s| s.config_id == config.id.unwrap_or_default())
                .map(|s| s.is_running)
                .unwrap_or(false);

            let is_processing = config.id.is_some_and(|id| {
                configs_being_processed
                    .get(&id)
                    .is_some_and(|(flag, _)| !flag.load(std::sync::atomic::Ordering::Relaxed))
            });

            let base_style = if is_processing {
                Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
            } else if state {
                Style::default().fg(GREEN)
            } else {
                Style::default().fg(RED)
            };

            let row_style = if selected_rows.contains(&i) {
                base_style.bg(SURFACE0).fg(SUBTEXT0)
            } else {
                base_style
            };

            let alias_text = if let Some(id) = config.id {
                if configs_being_processed
                    .get(&id)
                    .is_some_and(|(flag, _)| !flag.load(std::sync::atomic::Ordering::Relaxed))
                {
                    let index = (throbber_state.index().unsigned_abs() as usize)
                        % throbber_widgets_tui::BRAILLE_SIX.symbols.len();
                    let symbol = throbber_widgets_tui::BRAILLE_SIX.symbols[index];

                    format!("{} {}", symbol, config.alias.clone().unwrap_or_default())
                } else {
                    config.alias.clone().unwrap_or_default()
                }
            } else {
                config.alias.clone().unwrap_or_default()
            };

            Row::new(vec![
                Cell::from(alias_text),
                Cell::from(config.workload_type.clone().unwrap_or_default()),
                Cell::from(
                    config
                        .local_port
                        .map_or_else(|| "".to_string(), |port| port.to_string()),
                ),
                Cell::from(config.context.clone().unwrap_or_default()),
            ])
            .style(row_style)
        })
        .collect();

    let focus_color = if has_focus { YELLOW } else { TEXT };
    let border_modifier = if has_focus {
        Modifier::BOLD
    } else {
        Modifier::empty()
    };

    let table = Table::new(
        rows,
        [
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from("Alias"),
            Cell::from("Workload"),
            Cell::from("Local Port"),
            Cell::from("Context"),
        ])
        .style(style_bold().fg(MAUVE)),
    )
    .block(
        Block::default()
            .border_type(BorderType::Rounded)
            .borders(Borders::ALL)
            .title_alignment(Alignment::Left)
            .border_style(
                Style::default()
                    .fg(focus_color)
                    .add_modifier(border_modifier),
            )
            .title(Span::styled(title, Style::default().fg(MAUVE))),
    )
    .row_highlight_style(Style::default().bg(SURFACE1).fg(TEXT));

    frame.render_stateful_widget(table, area, state);

    let height = area.height.saturating_sub(2);
    let offset_with_last_in_view = configs.len().saturating_sub(height as usize);
    if let Some(selection) = state.selected() {
        if selection >= offset_with_last_in_view {
            *state.offset_mut() = offset_with_last_in_view;
        }
    } else {
        *state.offset_mut() = offset_with_last_in_view;
    }

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .track_symbol(None)
        .end_symbol(None)
        .style(Style::default().fg(SURFACE2).bg(BASE));
    let mut scrollbar_state = ScrollbarState::new(configs.len().saturating_sub(height as usize))
        .position(state.offset())
        .viewport_content_length(height as usize);
    let scrollbar_area = Rect {
        x: area.x + area.width - 1,
        y: area.y.saturating_add(2),
        height: area.height.saturating_sub(2),
        width: 1,
    };
    frame.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
}

pub fn render_details(
    f: &mut Frame, app: &mut App, config: &Config, config_states: &[ConfigState], area: Rect,
    has_focus: bool,
) {
    let state = config_states
        .iter()
        .find(|s| s.config_id == config.id.unwrap_or_default())
        .map(|s| s.is_running)
        .unwrap_or(false);

    let http_logs_enabled = if let Some(config_id) = config.id {
        *app.http_logs_enabled.get(&config_id).unwrap_or(&false)
    } else {
        false
    };

    let mut details = vec![Line::from(vec![
        Span::styled("HTTP Logs: ", Style::default().add_modifier(Modifier::BOLD)),
        Span::styled(
            if http_logs_enabled {
                "Enabled"
            } else {
                "Disabled"
            },
            Style::default().fg(if http_logs_enabled {
                Color::Green
            } else {
                Color::Red
            }),
        ),
    ])];

    if http_logs_enabled
        && let Some(config_id) = config.id
        && let Some(local_port) = config.local_port
    {
        let log_file_name = format!("{}_{}.http", config_id, local_port);
        if let Ok(log_folder_path) = kftray_commons::utils::config_dir::get_log_folder_path() {
            let log_file_path = log_folder_path.join(&log_file_name);
            details.push(Line::from(vec![
                Span::styled(
                    "  Log File: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(log_file_path.display().to_string()),
            ]));
        } else {
            details.push(Line::from(vec![
                Span::styled(
                    "  Log File: ",
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(log_file_name),
            ]));
        }
    }

    if state
        && let Some(config_id) = config.id
        && let Some(Some(active_pod)) = app.active_pods.get(&config_id)
    {
        details.push(Line::from(vec![
            Span::styled(
                "Active Pod: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(active_pod, Style::default().fg(Color::Green)),
        ]));
    }

    details.push(Line::from(""));

    details.extend(vec![
        Line::from(vec![
            Span::styled("Context: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.context.as_deref().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Alias: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.alias.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Service: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.service.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Namespace: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.namespace),
        ]),
        Line::from(vec![
            Span::styled(
                "Local Address: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.local_address.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled(
                "Local Port: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(
                config
                    .local_port
                    .map_or_else(|| "".to_string(), |port| port.to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Remote Address: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.remote_address.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled(
                "Remote Port: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(
                config
                    .remote_port
                    .map_or_else(|| "".to_string(), |port| port.to_string()),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                "Workload Type: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.workload_type.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Protocol: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.protocol),
        ]),
        Line::from(vec![
            Span::styled(
                "Domain Enabled: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.domain_enabled.unwrap_or(false).to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Kubeconfig: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(config.kubeconfig.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Target: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(config.target.clone().unwrap_or_default()),
        ]),
        Line::from(vec![
            Span::styled("Status: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(state.to_string()),
        ]),
    ]);

    let details_clone = details.clone();

    let height = area.height as usize;
    app.details_scroll_max_offset = details_clone.len().saturating_sub(height);

    let visible_details: Vec<Line> = details_clone
        .iter()
        .skip(app.details_scroll_offset)
        .take(height)
        .cloned()
        .collect();

    let paragraph = Paragraph::new(Text::from(visible_details))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled("Details", Style::default().fg(MAUVE)))
                .border_style(if has_focus {
                    Style::default().fg(YELLOW).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(TEXT)
                }),
        )
        .style(Style::default().fg(TEXT).bg(BASE))
        .wrap(ratatui::widgets::Wrap { trim: true });

    f.render_widget(paragraph, area);

    let scrollbar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .track_symbol(None)
        .end_symbol(None)
        .style(Style::default().fg(SURFACE2).bg(BASE));
    let mut scrollbar_state = ScrollbarState::new(app.details_scroll_max_offset)
        .position(app.details_scroll_offset)
        .viewport_content_length(height);
    let scrollbar_area = Rect {
        x: area.x + area.width - 1,
        y: area.y.saturating_add(2),
        height: area.height.saturating_sub(2),
        width: 1,
    };
    f.render_stateful_widget(scrollbar, scrollbar_area, &mut scrollbar_state);
}

pub fn style_bold() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}
