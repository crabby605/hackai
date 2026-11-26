//! Offline terminal dashboard for the locally-derived HackAI data.

use crate::{api, processor, read_stored_token, state};
use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};
use serde_json::Value;
use std::{io, path::Path, time::Duration};

struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> io::Result<Self> {
        enable_raw_mode()?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error);
        }
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
    }
}

pub fn run(repo: &Path, online: bool) -> io::Result<()> {
    let root = state::root(repo)?;
    let _guard = TerminalGuard::enter()?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut data = DashboardData::load(&root, online)?;
    let mut selected = 0_usize;
    let mut details_open = false;

    loop {
        terminal.draw(|frame| render(frame, &data, selected, details_open))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            match key.code {
                KeyCode::Char('q') => break,
                KeyCode::Esc | KeyCode::Left => details_open = false,
                KeyCode::Enter | KeyCode::Right => details_open = true,
                KeyCode::Char('r') => data = DashboardData::load(&root, online)?,
                KeyCode::Up | KeyCode::Char('k') => selected = selected.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => {
                    selected = selected
                        .saturating_add(1)
                        .min(data.output.episodes.len().saturating_sub(1));
                }
                _ => {}
            }
        }
    }
    Ok(())
}

struct DashboardData {
    project: String,
    tracked: bool,
    authenticated: bool,
    output: processor::ProcessorOutput,
    server: ServerData,
}

#[derive(Default)]
struct ServerData {
    status: String,
    projects: usize,
    sessions: usize,
    average_pesdo: Option<i32>,
}

impl DashboardData {
    fn load(root: &Path, online: bool) -> io::Result<Self> {
        // Prefer a project-local config for silent dashboard sync. Only the
        // explicit --online mode is allowed to touch macOS Keychain.
        let token = api::read_local_config(root)
            .ok()
            .and_then(|config| config.api_key)
            .or_else(|| online.then(|| read_stored_token(root)).flatten());
        let authenticated = token.is_some();
        let server = token
            .as_deref()
            .map(fetch_server_data)
            .unwrap_or_else(|| ServerData {
                status: if online {
                    "not authenticated".to_owned()
                } else {
                    "local data".to_owned()
                },
                ..ServerData::default()
            });
        Ok(Self {
            project: root
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("project")
                .to_owned(),
            tracked: state::state_dir(root).join("tracked").exists(),
            authenticated,
            output: processor::process(root)?,
            server,
        })
    }

    fn total_human_seconds(&self) -> u64 {
        self.output
            .episodes
            .iter()
            .map(|episode| episode.human_active_millis / 1_000)
            .sum()
    }

    fn total_agent_seconds(&self) -> u64 {
        self.output
            .episodes
            .iter()
            .map(|episode| episode.attended_agent_millis / 1_000)
            .sum()
    }

    fn average_score(&self) -> Option<i32> {
        // Tiny agent-only remnants are useful in a trace but should not
        // dominate the project headline. The summary is duration-weighted
        // across substantive collaboration sessions.
        let scored = self.output.episodes.iter().filter(|episode| {
            episode.dialogue_turn_count > 0 && episode.union_millis >= 5 * 60 * 1_000
        });
        let (weighted, duration) =
            scored.fold((0_u128, 0_u128), |(weighted, duration), episode| {
                (
                    weighted.saturating_add(
                        u128::from(episode.pesdo_score_permille) * u128::from(episode.union_millis),
                    ),
                    duration.saturating_add(u128::from(episode.union_millis)),
                )
            });
        (duration > 0).then(|| i32::try_from(weighted / duration).unwrap_or_default())
    }
}

fn fetch_server_data(token: &str) -> ServerData {
    let server_url = std::env::var("HACKAI_SERVER_URL")
        .unwrap_or_else(|_| "https://hackai.crabby605.hackclub.app".to_owned());
    let stats = api::api_request(&server_url, token, "GET", "/api/v1/dashboard/stats", None);
    let projects = api::api_request(&server_url, token, "GET", "/api/v1/projects", None);
    let sessions = api::api_request(&server_url, token, "GET", "/api/v1/episodes", None);
    match (stats, projects, sessions) {
        (Ok(stats), Ok(projects), Ok(sessions)) => ServerData {
            status: "synced with dashboard".to_owned(),
            projects: array_len(&projects),
            sessions: array_len(&sessions),
            average_pesdo: stats
                .get("mean_pesdo")
                .and_then(Value::as_i64)
                .and_then(|value| i32::try_from(value).ok()),
        },
        _ => ServerData {
            status: "server unavailable · local data shown".to_owned(),
            ..ServerData::default()
        },
    }
}

fn array_len(value: &Value) -> usize {
    value.as_array().map_or(0, Vec::len)
}

fn render(frame: &mut ratatui::Frame, data: &DashboardData, selected: usize, details_open: bool) {
    let outer = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(7),
            Constraint::Min(8),
            Constraint::Length(2),
        ])
        .split(frame.area());
    let sync = if data.authenticated {
        data.server.status.as_str()
    } else {
        "local only · run hackai login"
    };
    let title = Line::from(vec![
        Span::styled(
            " HACKAI ",
            Style::default()
                .bg(Color::Magenta)
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!("  {}  ·  {sync}", data.project)),
    ]);
    frame.render_widget(
        Paragraph::new(title).block(Block::default().borders(Borders::ALL)),
        outer[0],
    );

    let stats = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
            Constraint::Percentage(25),
        ])
        .split(outer[1]);
    stat_card(
        frame,
        stats[0],
        "Human time",
        &format_duration(data.total_human_seconds()),
        Color::Green,
    );
    stat_card(
        frame,
        stats[1],
        "Agent time",
        &format_duration(data.total_agent_seconds()),
        Color::Cyan,
    );
    stat_card(
        frame,
        stats[2],
        "Sessions",
        &data.output.episodes.len().to_string(),
        Color::Yellow,
    );
    let pesdo = data
        .average_score()
        .or(data.server.average_pesdo)
        .map_or_else(
            || "—".to_owned(),
            |score| format!("{score}/1000 · {}", pesdo_band(score)),
        );
    stat_card(frame, stats[3], "PESDO signal", &pesdo, Color::Magenta);

    let content = if details_open {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(43), Constraint::Percentage(57)])
            .split(outer[2])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(100), Constraint::Length(0)])
            .split(outer[2])
    };
    let visible_sessions = data
        .output
        .episodes
        .iter()
        .rev()
        .take(12)
        .collect::<Vec<_>>();
    let rows = visible_sessions
        .iter()
        .copied()
        .enumerate()
        .map(|(index, episode)| {
            let outcome = if episode.resolved {
                "shipped"
            } else {
                "in progress"
            };
            Row::new([
                Cell::from(format_timestamp(episode.started_at_unix_ms)),
                Cell::from(episode.dialogue_turn_count.to_string()),
                Cell::from(format_duration(episode.human_active_millis / 1_000)),
                Cell::from(format_duration(episode.attended_agent_millis / 1_000)),
                Cell::from(format!("{}/1000", episode.pesdo_score_permille)),
                Cell::from(outcome),
            ])
            .style(if index == selected {
                Style::default().bg(Color::DarkGray).fg(Color::White)
            } else {
                Style::default()
            })
        });
    let header = Row::new(["Started", "Turns", "Human", "Agent", "PESDO", "Outcome"]).style(
        Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(
        rows,
        [
            Constraint::Length(12),
            Constraint::Length(6),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Min(12),
        ],
    )
    .header(header)
    .block(Block::default().title(" Sessions ").borders(Borders::ALL));
    frame.render_widget(table, content[0]);
    if details_open {
        render_session_detail(
            frame,
            content[1],
            visible_sessions.get(selected).copied(),
            data,
        );
    }

    let status = if data.tracked {
        "tracking"
    } else {
        "not tracked"
    };
    frame.render_widget(
        Paragraph::new(format!(" {status} · {} local events · {} dashboard projects · {} dashboard sessions · [↑/↓] select  [Enter] open  [Esc] close  [r] refresh  [q] quit", data.output.event_count, data.server.projects, data.server.sessions))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: true }),
        outer[3],
    );
}

fn render_session_detail(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    episode: Option<&processor::EpisodeSummary>,
    data: &DashboardData,
) {
    let Some(episode) = episode else {
        frame.render_widget(
            Paragraph::new("No session selected.").block(
                Block::default()
                    .title(" Session details ")
                    .borders(Borders::ALL),
            ),
            area,
        );
        return;
    };
    let turns = data
        .output
        .dialogue_turns
        .iter()
        .filter(|turn| turn.episode_id == episode.episode_id)
        .take(12)
        .collect::<Vec<_>>();
    let mut lines = vec![
        Line::from(vec![
            Span::styled("PESDO ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                format!("{}/1000", episode.pesdo_score_permille),
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  ·  {} turns", episode.dialogue_turn_count)),
        ]),
        Line::from(format!(
            "Human {}  ·  agent {}  ·  {}",
            format_duration(episode.human_active_millis / 1_000),
            format_duration(episode.attended_agent_millis / 1_000),
            if episode.resolved {
                "shipped"
            } else {
                "in progress"
            }
        )),
        Line::raw(""),
        Line::styled(
            "DERIVED DIALOGUE TRACE · NO PROMPT TEXT",
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        ),
    ];
    for turn in turns {
        lines.push(Line::from(vec![
            Span::styled("You: ", Style::default().fg(Color::Cyan)),
            Span::raw(format!("{:?}", turn.user_act)),
            Span::styled("  Agent: ", Style::default().fg(Color::Yellow)),
            Span::raw(format!("{:?}", turn.agent_state)),
        ]));
        lines.push(Line::styled(
            format!(
                "  {:?} · {:?} · {:?}{}",
                turn.verification,
                turn.new_information,
                turn.context,
                if turn.reason_codes.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", turn.reason_codes.join(", "))
                }
            ),
            Style::default().fg(Color::DarkGray),
        ));
    }
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title(" Session details ")
                .borders(Borders::ALL),
        ),
        area,
    );
}

fn stat_card(
    frame: &mut ratatui::Frame,
    area: ratatui::layout::Rect,
    label: &str,
    value: &str,
    color: Color,
) {
    let body = Paragraph::new(vec![
        Line::from(Span::styled(label, Style::default().fg(Color::DarkGray))),
        Line::from(Span::styled(
            value,
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )),
    ])
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(body, area);
}

fn format_duration(seconds: u64) -> String {
    let hours = seconds / 3_600;
    let minutes = seconds % 3_600 / 60;
    if hours > 0 {
        format!("{hours}h {minutes}m")
    } else {
        format!("{minutes}m")
    }
}

/// PESDO is an evidence signal, not a percent grade. The bands make that
/// explicit while retaining the exact, replayable permille value.
fn pesdo_band(score: i32) -> &'static str {
    match score {
        ..=349 => "limited evidence",
        350..=549 => "developing",
        550..=749 => "strong",
        750..=899 => "very strong",
        _ => "exceptional",
    }
}

fn format_timestamp(milliseconds: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0_i64, |duration| {
            i64::try_from(duration.as_millis()).unwrap_or(0)
        });
    let elapsed = now.saturating_sub(milliseconds).max(0) / 1_000;
    if elapsed < 3_600 {
        format!("{}m ago", elapsed / 60)
    } else if elapsed < 86_400 {
        format!("{}h ago", elapsed / 3_600)
    } else {
        format!("{}d ago", elapsed / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::format_duration;

    #[test]
    fn renders_compact_durations() {
        assert_eq!(format_duration(65), "1m");
        assert_eq!(format_duration(3_725), "1h 2m");
    }
}
