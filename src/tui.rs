use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph, Wrap};
use serde_json::{Map, Value, json};

use crate::chat::{
    ChatEntry, ThreadSummary, append_chat_message, delete_mission_thread, list_thread_summaries,
    mark_thread_seen, read_chat_history,
};
use crate::cockpit::{FleetRuntimeStatus, ROLES, capture_pane, focus_machine, restart_pane, runtime_status};
use crate::config::{load_fleet_config, load_models_config};
use crate::mission::plan_mission;
use crate::operator::chat as operator_chat;
use crate::state::{append_event, load_mission, new_mission, save_mission};

pub enum TuiAction {
    Exit,
    OpenCockpit,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FocusZone {
    Composer,
    Threads,
    Cockpit,
    Capture,
}

struct App {
    workspace: String,
    local_session: String,
    machine_session: String,
    focus: FocusZone,
    input: String,
    selected_thread: usize,
    selected_machine: usize,
    selected_role: usize,
    threads: Vec<ThreadSummary>,
    chat_history: Vec<ChatEntry>,
    runtime: Option<FleetRuntimeStatus>,
    capture_title: String,
    capture_lines: Vec<String>,
    capture_scroll: usize,
    flash: String,
    flash_until: Instant,
    last_thread_refresh: Instant,
    last_runtime_refresh: Instant,
}

impl App {
    fn new(workspace: String, local_session: String, machine_session: String) -> Self {
        Self {
            workspace,
            local_session,
            machine_session,
            focus: FocusZone::Composer,
            input: String::new(),
            selected_thread: 0,
            selected_machine: 0,
            selected_role: 1,
            threads: Vec::new(),
            chat_history: Vec::new(),
            runtime: None,
            capture_title: String::new(),
            capture_lines: Vec::new(),
            capture_scroll: 0,
            flash: String::new(),
            flash_until: Instant::now(),
            last_thread_refresh: Instant::now() - Duration::from_secs(10),
            last_runtime_refresh: Instant::now() - Duration::from_secs(10),
        }
    }

    fn refresh(&mut self) {
        if self.last_thread_refresh.elapsed() >= Duration::from_millis(400) {
            self.threads = list_thread_summaries(&self.workspace).unwrap_or_else(|_| {
                vec![ThreadSummary {
                    thread_key: format!("workspace:{}", self.workspace),
                    workspace: self.workspace.clone(),
                    mission_id: None,
                    kind: "workspace".to_string(),
                    title: "Workspace chat".to_string(),
                    mission_status: None,
                    message_count: 0,
                    unread_count: 0,
                    last_role: String::new(),
                    last_preview: "Start a conversation".to_string(),
                    last_timestamp: String::new(),
                }]
            });
            if self.selected_thread >= self.threads.len() {
                self.selected_thread = self.threads.len().saturating_sub(1);
            }
            if let Some(summary) = self.current_thread().cloned() {
                let _ = mark_thread_seen(&self.workspace, &summary.thread_key, summary.message_count);
                self.chat_history =
                    read_chat_history(&self.workspace, summary.mission_id.as_deref(), 80)
                        .unwrap_or_default();
            } else {
                self.chat_history.clear();
            }
            self.last_thread_refresh = Instant::now();
        }

        if self.last_runtime_refresh.elapsed() >= Duration::from_secs(2) {
            self.runtime = runtime_status(&self.local_session, &self.machine_session).ok();
            if let Some(runtime) = &self.runtime {
                if self.selected_machine >= runtime.machines.len() {
                    self.selected_machine = runtime.machines.len().saturating_sub(1);
                }
            } else {
                self.selected_machine = 0;
            }
            self.last_runtime_refresh = Instant::now();
        }
    }

    fn current_thread(&self) -> Option<&ThreadSummary> {
        self.threads.get(self.selected_thread)
    }

    fn current_mission_id(&self) -> Option<&str> {
        self.current_thread().and_then(|summary| summary.mission_id.as_deref())
    }

    fn selected_machine_label(&self) -> Option<&str> {
        self.runtime
            .as_ref()
            .and_then(|runtime| runtime.machines.get(self.selected_machine))
            .map(|machine| machine.label.as_str())
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

        if !event::poll(Duration::from_millis(120))
            .map_err(|err| format!("event poll failed: {err}"))?
        {
            continue;
        }

        let Event::Key(key) =
            event::read().map_err(|err| format!("event read failed: {err}"))?
        else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if matches!(
            (key.code, key.modifiers),
            (KeyCode::Char('q'), _)
                | (KeyCode::Char('c'), KeyModifiers::CONTROL)
        ) {
            return Ok(TuiAction::Exit);
        }

        match app.focus {
            FocusZone::Composer => {
                if let Some(action) = handle_composer_key(app, key)? {
                    return Ok(action);
                }
            }
            FocusZone::Threads => handle_threads_key(app, key)?,
            FocusZone::Cockpit => {
                if let Some(action) = handle_cockpit_key(app, key)? {
                    return Ok(action);
                }
            }
            FocusZone::Capture => handle_capture_key(app, key),
        }
    }
}

fn handle_composer_key(app: &mut App, key: KeyEvent) -> Result<Option<TuiAction>, String> {
    match key.code {
        KeyCode::Enter => {
            submit_message(app)?;
            Ok(None)
        }
        KeyCode::Esc => {
            app.focus = FocusZone::Cockpit;
            app.input.clear();
            Ok(None)
        }
        KeyCode::Left | KeyCode::Tab => {
            app.focus = FocusZone::Threads;
            Ok(None)
        }
        KeyCode::Backspace => {
            app.input.pop();
            Ok(None)
        }
        KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.input.clear();
            Ok(None)
        }
        KeyCode::Char(ch) => {
            app.input.push(ch);
            Ok(None)
        }
        _ => Ok(None),
    }
}

fn handle_threads_key(app: &mut App, key: KeyEvent) -> Result<(), String> {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.selected_thread + 1 < app.threads.len() {
                app.selected_thread += 1;
                app.last_thread_refresh = Instant::now() - Duration::from_secs(10);
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.selected_thread = app.selected_thread.saturating_sub(1);
            app.last_thread_refresh = Instant::now() - Duration::from_secs(10);
        }
        KeyCode::Char('d') => {
            if let Some(summary) = app.current_thread().cloned() {
                if let Some(mission_id) = summary.mission_id {
                    delete_mission_thread(&mission_id)?;
                    app.selected_thread = 0;
                    app.focus = FocusZone::Composer;
                    app.flash(format!("Deleted thread '{}'.", summary.title));
                    app.last_thread_refresh = Instant::now() - Duration::from_secs(10);
                } else {
                    app.flash("Workspace chat cannot be deleted.");
                }
            }
        }
        KeyCode::Right | KeyCode::Enter => app.focus = FocusZone::Composer,
        KeyCode::Left | KeyCode::Esc | KeyCode::Tab => app.focus = FocusZone::Cockpit,
        KeyCode::Char(ch) => {
            app.focus = FocusZone::Composer;
            app.input.clear();
            app.input.push(ch);
        }
        _ => {}
    }
    Ok(())
}

fn handle_cockpit_key(app: &mut App, key: KeyEvent) -> Result<Option<TuiAction>, String> {
    match key.code {
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
            if let Some(machine) = app.selected_machine_label().map(ToString::to_string) {
                let role = ROLES[app.selected_role];
                match focus_machine(&machine, Some(role), &app.local_session, &app.machine_session) {
                    Ok(_) => app.flash(format!("focused {machine}:{role}")),
                    Err(err) => app.flash(format!("focus failed: {err}")),
                }
            }
        }
        KeyCode::Char('r') => {
            if let Some(machine) = app.selected_machine_label().map(ToString::to_string) {
                let role = ROLES[app.selected_role];
                match restart_pane(&machine, role, &app.machine_session) {
                    Ok(result) if result.returncode == 0 => {
                        app.flash(format!("restart sent to {machine}:{role}"))
                    }
                    Ok(result) => app.flash(format!("restart failed: {}", compact_err(&result.stderr))),
                    Err(err) => app.flash(format!("restart failed: {err}")),
                }
            }
        }
        KeyCode::Char('x') => {
            if let Some(machine) = app.selected_machine_label().map(ToString::to_string) {
                let role = ROLES[app.selected_role];
                match capture_pane(&machine, role, 120, &app.machine_session) {
                    Ok(result) if result.returncode == 0 => {
                        app.capture_title = format!("{machine}:{role}");
                        app.capture_lines = result.stdout.lines().map(ToString::to_string).collect();
                        app.capture_scroll = app.capture_lines.len().saturating_sub(20);
                        app.focus = FocusZone::Capture;
                    }
                    Ok(result) => app.flash(format!("capture failed: {}", compact_err(&result.stderr))),
                    Err(err) => app.flash(format!("capture failed: {err}")),
                }
            }
        }
        KeyCode::Char('z') => return Ok(Some(TuiAction::OpenCockpit)),
        KeyCode::Left | KeyCode::Tab => app.focus = FocusZone::Threads,
        KeyCode::Right => app.focus = FocusZone::Composer,
        KeyCode::Char(ch) => {
            app.focus = FocusZone::Composer;
            app.input.clear();
            app.input.push(ch);
        }
        _ => {}
    }
    Ok(None)
}

fn handle_capture_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Char('j') | KeyCode::Down => {
            if app.capture_scroll + 1 < app.capture_lines.len() {
                app.capture_scroll += 1;
            }
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.capture_scroll = app.capture_scroll.saturating_sub(1);
        }
        KeyCode::Esc | KeyCode::Char('x') => {
            app.capture_lines.clear();
            app.capture_title.clear();
            app.capture_scroll = 0;
            app.focus = FocusZone::Cockpit;
        }
        _ => {}
    }
}

fn submit_message(app: &mut App) -> Result<(), String> {
    let message = app.input.trim().to_string();
    if message.is_empty() {
        app.flash("empty prompt");
        return Ok(());
    }
    app.input.clear();

    let mission = app.current_mission_id().map(load_mission).transpose()?;
    let mission_id = mission.as_ref().map(|entry| entry.mission_id.as_str());
    let machine = app.selected_machine_label().map(ToString::to_string);
    let role = ROLES[app.selected_role];

    append_chat_message(
        "user",
        &message,
        &app.workspace,
        mission_id,
        Some("plain_chat"),
        machine.as_deref(),
        Some(role),
        None,
    )?;
    if let Some(mission_id) = mission_id {
        append_event(
            mission_id,
            "chat.user",
            json!({ "content": message, "machine": machine, "pane": role }),
        )?;
    }

    let history_values = app
        .chat_history
        .iter()
        .rev()
        .take(12)
        .rev()
        .map(|entry| serde_json::to_value(entry).unwrap_or(Value::Null))
        .collect::<Vec<_>>();
    let payload = operator_chat(
        &message,
        mission.as_ref(),
        &app.workspace,
        app.selected_machine_label(),
        Some(role),
        &history_values,
    )?;

    if payload.get("intent").and_then(Value::as_str) == Some("mission_create") {
        let models = load_models_config()?;
        let fleet = load_fleet_config()?;
        let routing_overrides = payload
            .get("routing_overrides")
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_else(Map::new);
        let mission_goal = payload
            .get("mission_goal")
            .and_then(Value::as_str)
            .unwrap_or(&message);
        let mut created = new_mission(mission_goal, &app.workspace, Some(routing_overrides), &models);
        save_mission(&mut created)?;
        append_event(
            &created.mission_id,
            "mission.created",
            json!({ "goal": mission_goal, "workspace": app.workspace }),
        )?;
        let buddy_review = plan_mission(&mut created, &fleet)?;
        save_mission(&mut created)?;
        append_event(
            &created.mission_id,
            "mission.planned",
            json!({
                "plan": {
                    "title": created.title,
                    "summary": created.planner_summary,
                    "steps": created.steps,
                },
                "buddy_review": buddy_review,
            }),
        )?;

        append_chat_message(
            "user",
            &message,
            &app.workspace,
            Some(&created.mission_id),
            Some("mission_create"),
            machine.as_deref(),
            Some(role),
            None,
        )?;
        append_chat_message(
            "constant",
            payload.get("reply").and_then(Value::as_str).unwrap_or("No reply."),
            &app.workspace,
            Some(&created.mission_id),
            Some("mission_create"),
            machine.as_deref(),
            Some(role),
            None,
        )?;
        if let Some(answer) = payload
            .get("buddy_note")
            .and_then(Value::as_object)
            .and_then(|note| note.get("answer"))
            .and_then(Value::as_str)
        {
            append_chat_message(
                "buddy",
                answer,
                &app.workspace,
                Some(&created.mission_id),
                Some("buddy_answer"),
                None,
                None,
                None,
            )?;
        }
        app.flash(format!("mission created {}", created.mission_id));
        app.last_thread_refresh = Instant::now() - Duration::from_secs(10);
        app.refresh();
        if let Some(index) = app
            .threads
            .iter()
            .position(|summary| summary.mission_id.as_deref() == Some(created.mission_id.as_str()))
        {
            app.selected_thread = index;
        }
        return Ok(());
    }

    let reply = payload
        .get("reply")
        .and_then(Value::as_str)
        .unwrap_or("No reply.");
    append_chat_message(
        "constant",
        reply,
        &app.workspace,
        mission_id,
        payload.get("intent").and_then(Value::as_str),
        machine.as_deref(),
        Some(role),
        None,
    )?;
    if let Some(mission_id) = mission_id {
        append_event(
            mission_id,
            "chat.constant",
            json!({ "intent": payload.get("intent"), "reply": reply }),
        )?;
    }
    if let Some(answer) = payload
        .get("buddy_note")
        .and_then(Value::as_object)
        .and_then(|note| note.get("answer"))
        .and_then(Value::as_str)
    {
        append_chat_message(
            "buddy",
            answer,
            &app.workspace,
            mission_id,
            Some("buddy_answer"),
            None,
            None,
            None,
        )?;
    }

    if let Some(action) = payload.get("cockpit_action").and_then(Value::as_object) {
        match action.get("type").and_then(Value::as_str) {
            Some("focus") => {
                if let Some(machine_name) = action.get("machine").and_then(Value::as_str) {
                    let pane = action.get("pane").and_then(Value::as_str);
                    match focus_machine(machine_name, pane, &app.local_session, &app.machine_session)
                    {
                        Ok(_) => app.flash(format!(
                            "focused {}:{}",
                            machine_name,
                            pane.unwrap_or(role)
                        )),
                        Err(err) => app.flash(format!("focus failed: {err}")),
                    }
                }
            }
            Some("restart") => {
                if let Some(machine_name) = action.get("machine").and_then(Value::as_str) {
                    let pane = action.get("pane").and_then(Value::as_str).unwrap_or(role);
                    match restart_pane(machine_name, pane, &app.machine_session) {
                        Ok(result) if result.returncode == 0 => {
                            app.flash(format!("restart sent to {machine_name}:{pane}"))
                        }
                        Ok(result) => app.flash(format!("restart failed: {}", compact_err(&result.stderr))),
                        Err(err) => app.flash(format!("restart failed: {err}")),
                    }
                }
            }
            Some("capture") => {
                if let Some(machine_name) = action.get("machine").and_then(Value::as_str) {
                    let pane = action.get("pane").and_then(Value::as_str).unwrap_or(role);
                    match capture_pane(machine_name, pane, 120, &app.machine_session) {
                        Ok(result) if result.returncode == 0 => {
                            app.capture_title = format!("{machine_name}:{pane}");
                            app.capture_lines =
                                result.stdout.lines().map(ToString::to_string).collect();
                            app.capture_scroll = app.capture_lines.len().saturating_sub(20);
                            app.focus = FocusZone::Capture;
                        }
                        Ok(result) => app.flash(format!("capture failed: {}", compact_err(&result.stderr))),
                        Err(err) => app.flash(format!("capture failed: {err}")),
                    }
                }
            }
            Some("open") => {
                app.flash("Use z to open the full cockpit view.");
            }
            _ => {}
        }
    } else {
        app.flash("message routed through Constant");
    }

    app.last_thread_refresh = Instant::now() - Duration::from_secs(10);
    Ok(())
}

fn draw(frame: &mut ratatui::Frame, app: &App) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(12),
            Constraint::Length(4),
        ])
        .split(frame.area());
    draw_header(frame, layout[0], app);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Length(32),
            Constraint::Min(40),
            Constraint::Length(36),
        ])
        .split(layout[1]);
    draw_threads(frame, body[0], app);
    draw_conversation(frame, body[1], app);
    draw_side_panel(frame, body[2], app);
    draw_footer(frame, layout[2], app);
}

fn draw_header(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let workspace_name = std::path::Path::new(&app.workspace)
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("workspace");
    let summary = format!(
        "{}  |  {} threads  |  focus={}  |  selected={}:{}",
        workspace_name,
        app.threads.len().saturating_sub(1),
        focus_label(app.focus),
        app.selected_machine_label().unwrap_or("-"),
        ROLES[app.selected_role]
    );
    let block = Block::default()
        .title("Constant")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan));
    let paragraph = Paragraph::new(summary).block(block);
    frame.render_widget(paragraph, area);
}

fn draw_threads(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let items = app
        .threads
        .iter()
        .enumerate()
        .map(|(index, thread)| {
            let mut style = Style::default().fg(Color::White);
            if index == app.selected_thread {
                style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
            } else if thread.unread_count > 0 {
                style = style.fg(Color::Green);
            }
            let badge = if index == app.selected_thread {
                "ACTIVE".to_string()
            } else if thread.unread_count > 0 {
                format!("NEW {}", thread.unread_count)
            } else {
                format!("{} msgs", thread.message_count)
            };
            ListItem::new(vec![
                Line::from(vec![Span::styled(
                    format!("{} [{}]", thread.title, badge),
                    style,
                )]),
                Line::from(Span::styled(
                    if thread.last_preview.is_empty() {
                        "Start a conversation".to_string()
                    } else {
                        format!("{}: {}", role_label(&thread.last_role), thread.last_preview)
                    },
                    Style::default().fg(Color::Gray),
                )),
            ])
        })
        .collect::<Vec<_>>();
    let title = if app.focus == FocusZone::Threads {
        "Recent Threads *"
    } else {
        "Recent Threads"
    };
    frame.render_widget(
        List::new(items).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(if app.focus == FocusZone::Threads {
                    Color::Yellow
                } else {
                    Color::Cyan
                })),
        ),
        area,
    );
}

fn draw_conversation(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let title = app
        .current_thread()
        .map(|thread| thread.title.clone())
        .unwrap_or_else(|| "Conversation".to_string());
    let lines = if app.chat_history.is_empty() {
        vec![
            Line::from("No conversation yet."),
            Line::from(""),
            Line::from("Type directly to talk to Constant."),
            Line::from("Use /spec-planner or /architecture-brainstorm to route faster."),
        ]
    } else {
        app.chat_history
            .iter()
            .map(|entry| {
                Line::from(vec![
                    Span::styled(
                        format!("{}  ", role_label(&entry.role)),
                        Style::default()
                            .fg(role_color(&entry.role))
                            .add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(entry.content.clone()),
                ])
            })
            .collect::<Vec<_>>()
    };
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Cyan)),
            )
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_side_panel(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(if app.focus == FocusZone::Capture && !app.capture_title.is_empty() {
            format!("Capture {}", app.capture_title)
        } else {
            "Cockpit".to_string()
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if app.focus == FocusZone::Cockpit {
            Color::Yellow
        } else {
            Color::Cyan
        }));

    if app.focus == FocusZone::Capture && !app.capture_title.is_empty() {
        let height = area.height.saturating_sub(2) as usize;
        let start = app.capture_scroll.min(app.capture_lines.len().saturating_sub(height));
        let visible = app
            .capture_lines
            .iter()
            .skip(start)
            .take(height)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n");
        frame.render_widget(Paragraph::new(visible).block(block).wrap(Wrap { trim: false }), area);
        return;
    }

    let lines = if let Some(runtime) = &app.runtime {
        let mut lines = vec![
            Line::from(format!(
                "session={}  focused={}:{}",
                runtime.local_session,
                runtime.focused_machine.as_deref().unwrap_or("-"),
                runtime.focused_role.as_deref().unwrap_or("-")
            )),
            Line::from(""),
        ];
        for (index, machine) in runtime.machines.iter().enumerate() {
            let marker = if index == app.selected_machine { ">" } else { " " };
            lines.push(Line::from(Span::styled(
                format!(
                    "{marker} {} [{}]",
                    machine.label,
                    if machine.session_exists { "up" } else { "down" }
                ),
                Style::default().fg(if index == app.selected_machine {
                    Color::Yellow
                } else {
                    Color::White
                }),
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Controls: [ ] machine | 1..4 pane | o focus | r restart | x capture | z open cockpit"));
        lines
    } else {
        vec![
            Line::from("No runtime snapshot yet."),
            Line::from(""),
            Line::from("Controls: [ ] machine | 1..4 pane | o focus | r restart | x capture | z open cockpit"),
        ]
    };

    frame.render_widget(Paragraph::new(lines).block(block).wrap(Wrap { trim: false }), area);
}

fn draw_footer(frame: &mut ratatui::Frame, area: Rect, app: &App) {
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(area);
    let status = if !app.flash.is_empty() {
        app.flash.clone()
    } else {
        match app.focus {
            FocusZone::Composer => "chat | Enter send | Left threads | Esc cockpit | Ctrl-Q quit".to_string(),
            FocusZone::Threads => "threads | j/k navigate | Enter reply | d delete | Esc cockpit".to_string(),
            FocusZone::Cockpit => "cockpit | [ ] machine | 1..4 pane | o focus | r restart | x capture | Right chat".to_string(),
            FocusZone::Capture => "capture | j/k scroll | x close".to_string(),
        }
    };
    frame.render_widget(Paragraph::new(status), sections[0]);
    frame.render_widget(
        Paragraph::new(format!("› {}", app.input))
            .block(
                Block::default()
                    .title(if app.focus == FocusZone::Composer {
                        "Ask Constant *"
                    } else {
                        "Ask Constant"
                    })
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(if app.focus == FocusZone::Composer {
                        Color::Yellow
                    } else {
                        Color::Cyan
                    })),
            )
            .wrap(Wrap { trim: false }),
        sections[1],
    );
}

fn compact_err(stderr: &str) -> String {
    stderr
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("command failed")
        .to_string()
}

fn role_label(role: &str) -> &'static str {
    match role {
        "user" => "YOU",
        "constant" => "CONSTANT",
        "buddy" => "BUDDY",
        "system" => "SYSTEM",
        _ => "CHAT",
    }
}

fn role_color(role: &str) -> Color {
    match role {
        "user" => Color::Green,
        "constant" => Color::Cyan,
        "buddy" => Color::Magenta,
        "system" => Color::Yellow,
        _ => Color::White,
    }
}

fn focus_label(focus: FocusZone) -> &'static str {
    match focus {
        FocusZone::Composer => "composer",
        FocusZone::Threads => "threads",
        FocusZone::Cockpit => "cockpit",
        FocusZone::Capture => "capture",
    }
}
