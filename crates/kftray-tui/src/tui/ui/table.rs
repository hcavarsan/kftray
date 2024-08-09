use std::collections::HashSet;

use kftray_commons::models::config_model::Config;
use kftray_commons::models::config_state_model::ConfigState;
use ratatui::{
    layout::{
        Constraint,
        Direction,
        Layout,
        Rect,
    },
    style::{
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
        Table,
    },
    Frame,
};

use crate::tui::input::{
    ActiveTable,
    App,
};
use crate::tui::ui::{
    BASE,
    GREEN,
    RED,
    SURFACE0,
    SURFACE1,
    SURFACE2,
    TEXT,
    YELLOW,
};

pub struct TableConfig<'a> {
    pub configs: &'a [Config],
    pub config_states: &'a [ConfigState],
    pub selected_row: usize,
    pub selected_rows: &'a HashSet<usize>,
    pub area: Rect,
    pub title: &'a str,
    pub is_active: bool,
}

pub fn draw_main_tab(f: &mut Frame, app: &mut App, config_states: &[ConfigState], area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(100)].as_ref())
        .split(area);

    let tables_chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
        .split(chunks[0]);


    draw_configs_table(
        f,
        TableConfig {
            configs: &app.filtered_stopped_configs,
            config_states,
            selected_row: app.selected_row_stopped,
            selected_rows: &app.selected_rows_stopped,
            area: tables_chunks[0],
            title: "Stopped Configs",
            is_active: app.active_table == ActiveTable::Stopped,
        },
    );

    draw_configs_table(
        f,
        TableConfig {
            configs: &app.filtered_running_configs,
            config_states,
            selected_row: app.selected_row_running,
            selected_rows: &app.selected_rows_running,
            area: tables_chunks[1],
            title: " Running Configs",
            is_active: app.active_table == ActiveTable::Running,
        },
    );
}

pub fn draw_configs_table(f: &mut Frame, table_config: TableConfig) {
    let TableConfig {
        configs,
        config_states,
        selected_row,
        selected_rows,
        area,
        title,
        is_active,
    } = table_config;

    let rows: Vec<Row> = configs
        .iter()
        .enumerate()
        .map(|(row_index, config)| {
            let state = config_states
                .iter()
                .find(|s| s.config_id == config.id.unwrap_or_default())
                .map(|s| s.is_running)
                .unwrap_or(false);

            let style = if selected_rows.contains(&row_index) {
                Style::default().fg(TEXT).bg(SURFACE2)
            } else if is_active && row_index == selected_row {
                Style::default().fg(TEXT).bg(SURFACE1)
            } else {
                Style::default()
                    .fg(if state { GREEN } else { RED })
                    .bg(BASE)
            };

            let remote_target = match config.workload_type.as_str() {
                "proxy" => config.remote_address.clone().unwrap_or_default(),
                "service" => config.service.clone().unwrap_or_default(),
                "pod" => config.target.clone().unwrap_or_default(),
                _ => String::new(),
            };

            Row::new(vec![
                Cell::from(config.alias.clone().unwrap_or_default()),
                Cell::from(config.workload_type.clone()),
                Cell::from(remote_target),
                Cell::from(config.local_address.clone().unwrap_or_default()),
                Cell::from(config.local_port.to_string()),
                Cell::from(config.context.clone()),
            ])
            .style(style)
        })
        .collect();

    let table = Table::new(
        rows,
        &[
            Constraint::Length(30),
            Constraint::Length(30),
            Constraint::Length(50),
            Constraint::Length(30),
            Constraint::Length(15),
            Constraint::Length(30),
        ],
    )
    .header(
        Row::new(vec![
            Cell::from(Span::styled(
                "Alias",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Workload",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Remote Target",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Local Address",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Local Port",
                Style::default().add_modifier(Modifier::BOLD),
            )),
            Cell::from(Span::styled(
                "Context",
                Style::default().add_modifier(Modifier::BOLD),
            )),
        ])
        .style(Style::default().bg(SURFACE0)),
    )
    .block(
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(if is_active {
                Style::default().fg(YELLOW).bg(BASE)
            } else {
                Style::default().bg(BASE)
            }),
    )
    .highlight_style(if is_active {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
    })
    .highlight_symbol(">> ");

    f.render_widget(table, area);
}

pub fn render_details(f: &mut Frame, config: &Config, config_states: &[ConfigState], area: Rect) {
    let state = config_states
        .iter()
        .find(|s| s.config_id == config.id.unwrap_or_default())
        .map(|s| s.is_running)
        .unwrap_or(false);

    let details = vec![
        Line::from(vec![
            Span::styled("Context: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.context),
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
            Span::raw(config.local_port.to_string()),
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
            Span::raw(config.remote_port.to_string()),
        ]),
        Line::from(vec![
            Span::styled("Context: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(&config.context),
        ]),
        Line::from(vec![
            Span::styled(
                "Workload Type: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::raw(&config.workload_type),
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
    ];

    let paragraph = Paragraph::new(Text::from(details))
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .style(Style::default().fg(TEXT).bg(BASE));

    f.render_widget(paragraph, area);
}
