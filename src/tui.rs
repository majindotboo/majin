use bevy::{app::AppExit, ecs::system::SystemParam, prelude::*};
use bevy_ratatui::{
    RatatuiContext,
    event::{KeyMessage, MouseMessage},
};
use ratatui::{
    crossterm::event::{KeyCode, KeyEventKind, KeyModifiers, MouseEventKind},
    layout::{Constraint, Layout, Margin},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
};

use crate::{
    ActiveSession, HarnessReady, InterruptTurn, MajinSet, MajinStartupSet, ModelRequest,
    SelectBranch, SelectSession, Session, SubmitPrompt, ToolUse, TranscriptCamera, TranscriptRow,
    TranscriptWork, Turn, WorkStatus, camera::TranscriptProjector,
};

pub struct TuiPlugin;

impl Plugin for TuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppExit>()
            .add_message::<KeyMessage>()
            .add_message::<MouseMessage>()
            .add_systems(
                Startup,
                draw_startup_loading
                    .in_set(MajinStartupSet::Loading)
                    .run_if(resource_exists::<RatatuiContext>),
            )
            .add_systems(Startup, initialize_tui_view.in_set(MajinStartupSet::Tui))
            .add_systems(
                Update,
                (handle_input, handle_mouse_input).in_set(MajinSet::Input),
            )
            .add_systems(Update, sync_transcript_camera.in_set(MajinSet::Project))
            .add_systems(
                Update,
                draw.in_set(MajinSet::Render)
                    .run_if(resource_exists::<RatatuiContext>),
            );
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum TuiFocus {
    #[default]
    Composer,
    Transcript,
}

#[derive(Component)]
pub struct TuiView {
    pub composer: String,
    pub focus: TuiFocus,
    pub transcript_camera: Entity,
}

#[derive(Component, Default)]
pub struct TerminalTranscriptViewport {
    pub scroll_from_bottom: usize,
}

#[derive(SystemParam)]
struct TuiInputState<'w, 's> {
    views: Query<'w, 's, &'static mut TuiView>,
    viewports: Query<'w, 's, &'static mut TerminalTranscriptViewport>,
    ready: Option<Res<'w, HarnessReady>>,
    active_session: Option<Res<'w, ActiveSession>>,
    sessions: Query<'w, 's, (Entity, &'static Session)>,
    turns: Query<'w, 's, (Entity, &'static Turn)>,
    requests: Query<'w, 's, &'static ModelRequest>,
    tool_uses: Query<'w, 's, &'static ToolUse>,
}

#[derive(SystemParam)]
struct TuiDrawState<'w, 's> {
    ready: Option<Res<'w, HarnessReady>>,
    active_session: Option<Res<'w, ActiveSession>>,
    sessions: Query<'w, 's, &'static Session>,
    turns: Query<'w, 's, &'static Turn>,
    views: Query<'w, 's, &'static mut TuiView>,
    cameras: Query<
        'w,
        's,
        (
            &'static TranscriptCamera,
            &'static mut TerminalTranscriptViewport,
        ),
    >,
    projector: TranscriptProjector<'w, 's>,
}

fn draw_startup_loading(mut context: ResMut<RatatuiContext>) -> Result {
    draw_loading(&mut context)
}

fn initialize_tui_view(world: &mut World) {
    let session = world.resource::<ActiveSession>().0;
    let head = world
        .get::<Session>(session)
        .expect("active session")
        .active_head;
    let camera = world
        .spawn((
            TranscriptCamera { session, head },
            TerminalTranscriptViewport::default(),
        ))
        .id();
    world.spawn(TuiView {
        composer: String::new(),
        focus: TuiFocus::Composer,
        transcript_camera: camera,
    });
}

fn handle_input(
    mut messages: MessageReader<KeyMessage>,
    state: TuiInputState,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let TuiInputState {
        mut views,
        mut viewports,
        ready,
        active_session,
        sessions,
        turns,
        requests,
        tool_uses,
    } = state;
    let Ok(mut ui) = views.single_mut() else {
        return;
    };
    if ready.is_none() {
        return;
    }
    let mut selected_session = active_session.as_deref().map(|active| active.0);
    let mut selected_head = selected_session.and_then(|session| session_head(&sessions, session));
    let mut submission_queued = false;

    for message in messages.read() {
        if message.kind == KeyEventKind::Release {
            continue;
        }
        if message.code == KeyCode::Esc {
            exit.write_default();
            continue;
        }
        let Some(session) = selected_session else {
            continue;
        };
        let camera = ui.transcript_camera;

        match (message.code, message.modifiers) {
            (KeyCode::Tab, _) => {
                if let Some(next) = adjacent_session(&sessions, session, 1) {
                    selected_session = Some(next);
                    selected_head = session_head(&sessions, next);
                    commands.queue(SelectSession { session: next });
                }
            }
            (KeyCode::BackTab, _) => {
                if let Some(previous) = adjacent_session(&sessions, session, -1) {
                    selected_session = Some(previous);
                    selected_head = session_head(&sessions, previous);
                    commands.queue(SelectSession { session: previous });
                }
            }
            (KeyCode::Up, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                if !session_has_active_work(&requests, &tool_uses, &turns, session)
                    && let Some(head) = adjacent_branch(&turns, session, selected_head, -1)
                {
                    selected_head = Some(head);
                    commands.queue(SelectBranch { session, head });
                }
            }
            (KeyCode::Down, modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                if !session_has_active_work(&requests, &tool_uses, &turns, session)
                    && let Some(head) = adjacent_branch(&turns, session, selected_head, 1)
                {
                    selected_head = Some(head);
                    commands.queue(SelectBranch { session, head });
                }
            }
            (KeyCode::Char('x'), modifiers) if modifiers.contains(KeyModifiers::CONTROL) => {
                if let Some(turn) = interruptible_turn(&requests, &tool_uses, selected_head) {
                    commands.queue(InterruptTurn { turn });
                }
            }
            (KeyCode::F(2), _) => {
                ui.focus = match ui.focus {
                    TuiFocus::Composer => TuiFocus::Transcript,
                    TuiFocus::Transcript => TuiFocus::Composer,
                };
            }
            _ if message
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {}
            (KeyCode::Enter, _) if ui.focus == TuiFocus::Composer => {
                let text = ui.composer.trim().to_owned();
                if !submission_queued
                    && !text.is_empty()
                    && sessions.get(session).is_ok()
                    && !session_has_active_work(&requests, &tool_uses, &turns, session)
                {
                    commands.queue(SubmitPrompt { session, text });
                    ui.composer.clear();
                    if let Ok(mut viewport) = viewports.get_mut(camera) {
                        viewport.scroll_from_bottom = 0;
                    }
                    submission_queued = true;
                }
            }
            (KeyCode::Backspace, _) if ui.focus == TuiFocus::Composer => {
                ui.composer.pop();
            }
            (KeyCode::Char(character), _) if ui.focus == TuiFocus::Composer => {
                ui.composer.push(character);
            }
            (KeyCode::Up, _) => scroll_up(&mut viewports, camera, 1),
            (KeyCode::Down, _) => scroll_down(&mut viewports, camera, 1),
            (KeyCode::PageUp, _) => scroll_up(&mut viewports, camera, 10),
            (KeyCode::PageDown, _) => scroll_down(&mut viewports, camera, 10),
            (KeyCode::Home, _) => {
                if let Ok(mut viewport) = viewports.get_mut(camera) {
                    viewport.scroll_from_bottom = usize::MAX;
                }
            }
            (KeyCode::End, _) => {
                if let Ok(mut viewport) = viewports.get_mut(camera) {
                    viewport.scroll_from_bottom = 0;
                }
            }
            _ => {}
        }
    }
}

fn session_head(sessions: &Query<(Entity, &Session)>, session: Entity) -> Option<Entity> {
    sessions
        .get(session)
        .ok()
        .and_then(|(_, session)| session.active_head)
}

fn adjacent_session(
    sessions: &Query<(Entity, &Session)>,
    current: Entity,
    direction: isize,
) -> Option<Entity> {
    let mut sessions: Vec<_> = sessions.iter().collect();
    sessions.sort_by_key(|(entity, session)| (session.id.0, entity.to_bits()));
    adjacent(
        &sessions
            .into_iter()
            .map(|(entity, _)| entity)
            .collect::<Vec<_>>(),
        current,
        direction,
    )
}

fn adjacent_branch(
    turns: &Query<(Entity, &Turn)>,
    session: Entity,
    current: Option<Entity>,
    direction: isize,
) -> Option<Entity> {
    let mut turns: Vec<_> = turns
        .iter()
        .filter(|(_, turn)| turn.session == session)
        .collect();
    turns.sort_by_key(|(entity, turn)| (turn.sequence, turn.id.0, entity.to_bits()));
    let turns: Vec<_> = turns.into_iter().map(|(entity, _)| entity).collect();
    match current {
        Some(current) => adjacent(&turns, current, direction),
        None if direction < 0 => turns.last().copied(),
        None => turns.first().copied(),
    }
}

fn adjacent(items: &[Entity], current: Entity, direction: isize) -> Option<Entity> {
    if items.is_empty() {
        return None;
    }
    if items.len() == 1 {
        return (items[0] != current).then_some(items[0]);
    }
    let current = items.iter().position(|item| *item == current);
    let index = match (current, direction < 0) {
        (Some(0), true) | (None, true) => items.len() - 1,
        (Some(index), true) => index - 1,
        (Some(index), false) => (index + 1) % items.len(),
        (None, false) => 0,
    };
    Some(items[index])
}

fn interruptible_turn(
    requests: &Query<&ModelRequest>,
    tool_uses: &Query<&ToolUse>,
    head: Option<Entity>,
) -> Option<Entity> {
    let head = head?;
    requests
        .iter()
        .any(|request| request.turn == head && is_active(request.status))
        .then_some(head)
        .or_else(|| {
            tool_uses
                .iter()
                .any(|tool_use| tool_use.turn == head && is_active(tool_use.status))
                .then_some(head)
        })
}

fn session_has_active_work(
    requests: &Query<&ModelRequest>,
    tool_uses: &Query<&ToolUse>,
    turns: &Query<(Entity, &Turn)>,
    session: Entity,
) -> bool {
    requests.iter().any(|request| {
        is_active(request.status)
            && turns
                .get(request.turn)
                .is_ok_and(|(_, turn)| turn.session == session)
    }) || tool_uses.iter().any(|tool_use| {
        is_active(tool_use.status)
            && turns
                .get(tool_use.turn)
                .is_ok_and(|(_, turn)| turn.session == session)
    })
}

fn is_active(status: WorkStatus) -> bool {
    matches!(status, WorkStatus::Pending | WorkStatus::Running)
}

fn handle_mouse_input(
    mut messages: MessageReader<MouseMessage>,
    views: Query<&TuiView>,
    mut viewports: Query<&mut TerminalTranscriptViewport>,
) {
    let Ok(ui) = views.single() else {
        return;
    };

    for message in messages.read() {
        match message.kind {
            MouseEventKind::ScrollUp => scroll_up(&mut viewports, ui.transcript_camera, 3),
            MouseEventKind::ScrollDown => scroll_down(&mut viewports, ui.transcript_camera, 3),
            _ => {}
        }
    }
}

fn scroll_up(viewports: &mut Query<&mut TerminalTranscriptViewport>, camera: Entity, lines: usize) {
    if let Ok(mut viewport) = viewports.get_mut(camera) {
        viewport.scroll_from_bottom = viewport.scroll_from_bottom.saturating_add(lines);
    }
}

fn scroll_down(
    viewports: &mut Query<&mut TerminalTranscriptViewport>,
    camera: Entity,
    lines: usize,
) {
    if let Ok(mut viewport) = viewports.get_mut(camera) {
        viewport.scroll_from_bottom = viewport.scroll_from_bottom.saturating_sub(lines);
    }
}

fn sync_transcript_camera(
    active_session: Res<ActiveSession>,
    sessions: Query<&Session>,
    views: Query<&TuiView>,
    mut cameras: Query<&mut TranscriptCamera>,
) {
    let Ok(ui) = views.single() else {
        return;
    };
    let Ok(session) = sessions.get(active_session.0) else {
        return;
    };
    if let Ok(mut camera) = cameras.get_mut(ui.transcript_camera)
        && (camera.session != active_session.0 || camera.head != session.active_head)
    {
        camera.session = active_session.0;
        camera.head = session.active_head;
    }
}

fn transcript_lines(items: &[TranscriptRow]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for item in items {
        let (title, color, body) = match item {
            TranscriptRow::User(body) => ("YOU", Color::Cyan, body.clone()),
            TranscriptRow::Assistant(body) => ("MAJIN", Color::Green, body.clone()),
            TranscriptRow::ToolUse { tool, input } => {
                ("TOOL", Color::Yellow, format!("{tool}: {input}"))
            }
            TranscriptRow::ToolOutcome { tool, output } => {
                ("TOOL", Color::Yellow, format!("{tool}: {output}"))
            }
            TranscriptRow::Work { work, status } => {
                let work = match work {
                    TranscriptWork::Model => "model".into(),
                    TranscriptWork::Tool(tool) => format!("tool {tool}"),
                };
                ("ACTIVE", Color::Blue, format!("{work}: {status:?}"))
            }
            TranscriptRow::System(body) => ("SYSTEM", Color::Magenta, body.clone()),
            TranscriptRow::Error(body) => ("ERROR", Color::Red, body.clone()),
        };
        lines.push(Line::from(Span::styled(
            format!(" {title} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        lines.extend(body.lines().map(|line| Line::from(format!("  {line}"))));
        lines.push(Line::default());
    }

    lines
}

fn draw_loading(context: &mut RatatuiContext) -> Result {
    context.draw(|frame| {
        frame.render_widget(
            Paragraph::new("Loading harness...")
                .block(Block::default().borders(Borders::ALL).title(" MAJIN ")),
            frame.area(),
        );
    })?;
    Ok(())
}

fn draw(mut context: ResMut<RatatuiContext>, state: TuiDrawState) -> Result {
    let TuiDrawState {
        ready,
        active_session,
        sessions,
        turns,
        mut views,
        mut cameras,
        projector,
    } = state;
    let Ok(ui) = views.single_mut() else {
        return Ok(());
    };
    if ready.is_none() {
        return draw_loading(&mut context);
    }
    let Ok((camera, mut viewport)) = cameras.get_mut(ui.transcript_camera) else {
        return Ok(());
    };
    let items = projector.project(camera);
    let selection = active_session
        .as_deref()
        .and_then(|active| sessions.get(active.0).ok())
        .map(|session| {
            let turn = session
                .active_head
                .and_then(|head| turns.get(head).ok())
                .map(|turn| format!("turn {}", turn.id.0))
                .unwrap_or_else(|| "empty".into());
            format!("session {} · {turn}", session.id.0)
        })
        .unwrap_or_else(|| "no session".into());
    let composer_color = if ui.focus == TuiFocus::Composer {
        Color::Cyan
    } else {
        Color::DarkGray
    };

    context.draw(|frame| {
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Fill(1),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(frame.area());

        let header = Paragraph::new(Line::from(vec![
            Span::styled(
                " MAJIN ",
                Style::default()
                    .fg(Color::Magenta)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(selection.as_str(), Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("● fake", Style::default().fg(Color::Yellow)),
        ]))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(header, areas[0]);

        let lines = transcript_lines(&items);
        let visible_height = usize::from(areas[1].height.saturating_sub(2));
        let max_scroll = lines.len().saturating_sub(visible_height);
        viewport.scroll_from_bottom = viewport.scroll_from_bottom.min(max_scroll);
        let scroll = max_scroll.saturating_sub(viewport.scroll_from_bottom);
        let transcript = Paragraph::new(Text::from(lines))
            .block(Block::default().borders(Borders::ALL).title(" Transcript "))
            .scroll((scroll as u16, 0));
        frame.render_widget(transcript, areas[1]);

        let mut scrollbar_state = ScrollbarState::new(max_scroll).position(scroll);
        frame.render_stateful_widget(
            Scrollbar::new(ScrollbarOrientation::VerticalRight),
            areas[1].inner(Margin {
                vertical: 1,
                horizontal: 0,
            }),
            &mut scrollbar_state,
        );

        let composer = Paragraph::new(ui.composer.as_str()).block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(composer_color))
                .title(" Message "),
        );
        frame.render_widget(composer, areas[2]);

        let help = Line::from(
            " Enter send  Tab/Shift-Tab session  Ctrl+↑/↓ branch  Ctrl+X interrupt  F2 focus  Esc quit ",
        )
        .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(help, areas[3]);

        if ui.focus == TuiFocus::Composer {
            let cursor_x = areas[2]
                .x
                .saturating_add(1)
                .saturating_add(ui.composer.chars().count() as u16)
                .min(areas[2].right().saturating_sub(2));
            frame.set_cursor_position((cursor_x, areas[2].y.saturating_add(1)));
        }
    })?;

    Ok(())
}
