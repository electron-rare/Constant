use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap};
use serde_json::Value;

use crate::cockpit::{ROLES, capture_pane, focus_machine, restart_pane, runtime_status};
use crate::state::{list_missions, mission_summary_value};

pub enum TuiAction {
    Exit,
    OpenCockpit,
}

struct App {
    workspace: String,
    local_session: String,
    machine_session: String,
    selected_mission: usize,
    selected_machine: usize,
    selected_role: usize,
    missions: Vec<Value>,
    runtime: Option<crate::cockpit::FleetRuntimeStatus>,
    flash: String,
    flash_until: Instant,
    capture: Option<String>,
    capture_title: String,
    last_refresh: Instant,
}

const HEXAPUS_FRAMES: &[&[&str]] = &[
    &[
        "      .-====-.",
        "   .-'  .--.  `-.",
        "  /    ( oo )    \\",
        " |      \\/\\/      |",
        " |    .-____-.    |",
        "  \\__/_/ || \\_\\__/",
        "    /_  /||\\  _\\",
        " <_____/ || \\_____>",
    ],
    &[
        "      .-====-.",
        "   .-'  .--.  `-.",
        "  /    ( -- )    \\",
        " |      \\/\\/      |",
        " |    .-____-.    |",
        " _/\\_/ / || \\ \\_/\\_",
        "/_  _\\/  ||  \\/_  _\\",
        "  <_____/||\\_____>",
    ],
    &[
        "      .-====-.",
        "   .-'  .--.  `-.",
        "  /    ( xx )    \\",
        " |      \\/\\/      |",
        " |    .-____-.    |",
        "  \\__/__/||\\__\\__/",
        "    _\\_ /||\\ _/_",
        " <_____/ || \\_____>",
    ],
];

impl App {
    fn new(workspace: String, local_session: String, machine_session: String) -> Self {
        Self {
            workspace,
            local_session,
            machine_session,
            selected_mission: 0,
            selected_machine: 0,
            selected_role: 1,
            missions: Vec::new(),
            runtime: None,
            flash: String::new(),
            flash_until: Instant::now(),
            capture: None,
            capture_title: String::new(),
            last_refresh: Instant::now() - Duration::from_secs(10),
        }
    }

    fn refresh(&mut self) {
        if self.last_refresh.elapsed() < Duration::from_millis(700) {
            return;
        }
        self.missions = list_missions()
            .unwrap_or_default()
            .into_iter()
            .map(|mission| mission_summary_value(&mission))
            .collect();
        self.runtime = runtime_status(&self.local_session, &self.machine_session).ok();
        if self.selected_mission >= self.missions.len() {
            self.selected_mission = self.missions.len().saturating_sub(1);
        }
        if let Some(runtime) = &self.runtime {
            if self.selected_machine >= runtime.machines.len() {
                self.selected_machine = runtime.machines.len().saturating_sub(1);
            }
        } else {
            self.selected_machine = 0;
        }
        self.last_refresh = Instant::now();
    }

    fn flash(&mut self, message: impl Into<String>) {
        self.flash = message.into();
        self.flash_until = Instant::now() + Duration::from_secs(3);
    }

    fn clear_flash_if_needed(&mut self) {
        if Instant::now() >= self.flash_until {
            self.flash.clear();
        }
    }
}

pub fn run_tui(
    workspace: String,
    local_session: String,
    machine_session: String,
) -> Result<TuiAction, String> {
    enable_raw_mode().map_err(|err| format!("cannot enable raw mode: {err}"))?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen)
        .map_err(|err| format!("cannot enter alternate screen: {err}"))?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal =
        Terminal::new(backend).map_err(|err| format!("cannot create terminal: {err}"))?;

    let mut app = App::new(workspace, local_session, machine_session);
    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen).ok();
    terminal.show_cursor().ok();

    result
}

fn run_loop(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    app: &mut App,
) -> Result<TuiAction, String> {
    loop {
        app.refresh();
        app.clear_flash_if_needed();
        terminal
            .draw(|frame| draw(frame, app))
            .map_err(|err| format!("cannot draw TUI: {err}"))?;

        if event::poll(Duration::from_millis(120))
            .map_err(|err| format!("event poll failed: {err}"))?
        {
            let Event::Key(key) =
                event::read().map_err(|err| format!("event read failed: {err}"))?
            else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') => return Ok(TuiAction::Exit),
                KeyCode::Char('j') => {
                    if app.selected_mission + 1 < app.missions.len() {
                        app.selected_mission += 1;
                    }
                }
                KeyCode::Char('k') => {
                    app.selected_mission = app.selected_mission.saturating_sub(1);
                }
                KeyCode::Char(']') => {
                    if let Some(runtime) = &app.runtime {
                        if app.selected_machine + 1 < runtime.machines.len() {
                            app.selected_machine += 1;
                        }
                    }
                }
                KeyCode::Char('[') => {
                    app.selected_machine = app.selected_machine.saturating_sub(1);
                }
                KeyCode::Char('1') => app.selected_role = 0,
                KeyCode::Char('2') => app.selected_role = 1,
                KeyCode::Char('3') => app.selected_role = 2,
                KeyCode::Char('4') => app.selected_role = 3,
                KeyCode::Char('o') => {
                    if let Some(machine) = selected_machine(app) {
                        let role = ROLES[app.selected_role];
                        match focus_machine(
                            &machine.label,
                            Some(role),
                            &app.local_session,
                            &app.machine_session,
                        ) {
                            Ok(_) => app.flash(format!("focused {}:{}", machine.label, role)),
                            Err(err) => app.flash(format!("focus failed: {err}")),
                        }
                    }
                }
                KeyCode::Char('r') => {
                    if let Some(machine) = selected_machine(app) {
                        let role = ROLES[app.selected_role];
                        match restart_pane(&machine.label, role, &app.machine_session) {
                            Ok(result) if result.returncode == 0 => {
                                app.flash(format!("restart sent to {}:{}", machine.label, role))
                            }
                            Ok(result) => app
                                .flash(format!("restart failed: {}", compact_err(&result.stderr))),
                            Err(err) => app.flash(format!("restart failed: {err}")),
                        }
                    }
                }
                KeyCode::Char('x') => {
                    if app.capture.is_some() {
                        app.capture = None;
                        app.capture_title.clear();
                    } else if let Some(machine) = selected_machine(app) {
                        let machine_label = machine.label.clone();
                        let role = ROLES[app.selected_role];
                        match capture_pane(&machine_label, role, 80, &app.machine_session) {
                            Ok(result) if result.returncode == 0 => {
                                app.capture = Some(result.stdout);
                                app.capture_title = format!("{}:{}", machine_label, role);
                            }
                            Ok(result) => app
                                .flash(format!("capture failed: {}", compact_err(&result.stderr))),
                            Err(err) => app.flash(format!("capture failed: {err}")),
                        }
                    }
                }
                KeyCode::Esc => {
                    app.capture = None;
                    app.capture_title.clear();
                }
                KeyCode::Char('z') => return Ok(TuiAction::OpenCockpit),
                _ => {}
            }
        }
    }
}

fn selected_machine(app: &App) -> Option<&crate::cockpit::MachineRuntimeStatus> {
    app.runtime.as_ref()?.machines.get(app.selected_machine)
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(10),
            Constraint::Length(2),
        ])
        .split(frame.area());

    draw_header(frame, chunks[0], app);

    let main = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(28),
            Constraint::Percentage(48),
            Constraint::Percentage(24),
        ])
        .split(chunks[1]);

    draw_missions(frame, main[0], app);
    if app.capture.is_some() {
        draw_capture(frame, main[1], app);
    } else {
        draw_runtime(frame, main[1], app);
    }
    draw_hexapus(frame, main[2], app);
    draw_footer(frame, chunks[2], app);
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let machine_text = if let Some(machine) = selected_machine(app) {
        format!("{} / {}", machine.label, ROLES[app.selected_role])
    } else {
        "no machine".to_string()
    };
    let text = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                " CONSTANT ",
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(" "),
            Span::styled(app.workspace.as_str(), Style::default().fg(Color::Yellow)),
        ]),
        Line::from(vec![
            Span::raw("fleet="),
            Span::styled(
                app.local_session.as_str(),
                Style::default().fg(Color::Green),
            ),
            Span::raw("  machine="),
            Span::styled(machine_text, Style::default().fg(Color::Magenta)),
        ]),
    ])
    .block(Block::default().borders(Borders::ALL).title("Rust Cockpit"));
    frame.render_widget(text, area);
}

fn draw_missions(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let items = app
        .missions
        .iter()
        .enumerate()
        .map(|(index, mission)| {
            let selected = index == app.selected_mission;
            let title = mission
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("mission");
            let status = mission
                .get("status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let style = if selected {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{title} "), style),
                Span::styled(format!("[{status}]"), style.fg(Color::Cyan)),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(items).block(Block::default().borders(Borders::ALL).title("Missions"));
    frame.render_widget(list, area);
}

fn draw_runtime(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let mut lines = Vec::new();
    if let Some(runtime) = &app.runtime {
        for (index, machine) in runtime.machines.iter().enumerate() {
            let selected = index == app.selected_machine;
            let prefix = if selected { ">" } else { " " };
            lines.push(Line::from(Span::styled(
                format!(
                    "{prefix} {} [{}]",
                    machine.label,
                    if machine.session_exists { "up" } else { "down" }
                ),
                if selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            )));
            for (role_index, role) in ROLES.iter().enumerate() {
                let marker = if selected && role_index == app.selected_role {
                    "*"
                } else {
                    " "
                };
                let pane = machine.roles.get(*role).and_then(|pane| pane.as_ref());
                let status = match pane {
                    Some(pane) if pane.dead => "dead",
                    Some(_) => "live",
                    None => "missing",
                };
                lines.push(Line::from(format!("  {marker} {} {}", role, status)));
            }
        }
    } else {
        lines.push(Line::from("No runtime status."));
    }
    let paragraph = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Runtime"))
        .wrap(Wrap { trim: true });
    frame.render_widget(paragraph, area);
}

fn draw_capture(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let capture = app.capture.as_deref().unwrap_or("");
    let paragraph = Paragraph::new(capture)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!("Capture {}", app.capture_title)),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_hexapus(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let tick = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_millis())
        .unwrap_or(0);
    let frame_index = (tick / 220 % HEXAPUS_FRAMES.len() as u128) as usize;
    let frame_lines = HEXAPUS_FRAMES[frame_index];
    let mut lines = frame_lines
        .iter()
        .map(|line| Line::from(Span::styled(*line, Style::default().fg(Color::Cyan))))
        .collect::<Vec<_>>();
    lines.push(Line::from(""));
    if let Some(machine) = selected_machine(app) {
        lines.push(Line::from(format!("selected  {}", machine.label)));
        lines.push(Line::from(format!(
            "pane      {}",
            ROLES[app.selected_role]
        )));
        lines.push(Line::from(format!(
            "focused   {}",
            app.runtime
                .as_ref()
                .and_then(|runtime| runtime.focused_machine.clone())
                .unwrap_or_else(|| "-".to_string())
        )));
    }
    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title("Hexapus Buddy"),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let message = if app.flash.is_empty() {
        "j/k mission | [/] machine | 1..4 pane | o focus | x capture | r restart | z cockpit | q quit".to_string()
    } else {
        app.flash.clone()
    };
    let paragraph =
        Paragraph::new(message).block(Block::default().borders(Borders::ALL).title("Keys"));
    frame.render_widget(paragraph, area);

    if let Some(capture) = &app.capture {
        let popup = centered_rect(80, 70, area);
        frame.render_widget(Clear, popup);
        let block = Paragraph::new(capture.as_str())
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Capture view (Esc/x close)"),
            )
            .wrap(Wrap { trim: false });
        frame.render_widget(block, popup);
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

fn compact_err(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("no details")
        .to_string()
}
