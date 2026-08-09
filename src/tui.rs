use bevy::{app::AppExit, prelude::*};
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
    ActiveSession, AssistantMessage, MajinSet, MajinStartupSet, Session, SubmitPrompt,
    TranscriptCamera, TranscriptItem, TranscriptKind, Turn, UserMessage,
    camera::project_transcript_parts,
};

pub struct TuiPlugin;

impl Plugin for TuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppExit>()
            .add_message::<KeyMessage>()
            .add_message::<MouseMessage>()
            .add_systems(Startup, spawn_tui_view.in_set(MajinStartupSet::Tui))
            .add_systems(
                Update,
                (handle_input, handle_mouse_input).in_set(MajinSet::Input),
            )
            .add_systems(Update, sync_active_session.in_set(MajinSet::Project))
            .add_systems(
                Update,
                draw.in_set(MajinSet::Render)
                    .run_if(resource_exists::<RatatuiContext>),
            );
    }
}

#[derive(Component)]
pub struct TuiView {
    pub composer: String,
    pub transcript_camera: Entity,
}

fn spawn_tui_view(world: &mut World) {
    let camera = {
        let mut cameras = world.query_filtered::<Entity, With<TranscriptCamera>>();
        cameras.single(world).expect("one transcript camera")
    };
    world.spawn(TuiView {
        composer: String::new(),
        transcript_camera: camera,
    });
}

fn handle_input(
    mut messages: MessageReader<KeyMessage>,
    mut views: Query<&mut TuiView>,
    mut cameras: Query<&mut TranscriptCamera>,
    active_session: Res<ActiveSession>,
    sessions: Query<(), With<Session>>,
    mut commands: Commands,
    mut exit: MessageWriter<AppExit>,
) {
    let Ok(mut ui) = views.single_mut() else {
        return;
    };

    for message in messages.read() {
        if message.kind == KeyEventKind::Release {
            continue;
        }
        if message
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
        {
            continue;
        }

        match message.code {
            KeyCode::Esc => {
                exit.write_default();
            }
            KeyCode::Enter => {
                let text = ui.composer.trim().to_owned();
                if !text.is_empty() && sessions.get(active_session.0).is_ok() {
                    commands.queue(SubmitPrompt {
                        session: active_session.0,
                        text,
                    });
                    ui.composer.clear();
                    if let Ok(mut camera) = cameras.get_mut(ui.transcript_camera) {
                        camera.scroll_from_bottom = 0;
                    }
                }
            }
            KeyCode::Backspace => {
                ui.composer.pop();
            }
            KeyCode::Char(character) => ui.composer.push(character),
            KeyCode::Up => scroll_up(&mut cameras, ui.transcript_camera, 1),
            KeyCode::Down => scroll_down(&mut cameras, ui.transcript_camera, 1),
            KeyCode::PageUp => scroll_up(&mut cameras, ui.transcript_camera, 10),
            KeyCode::PageDown => scroll_down(&mut cameras, ui.transcript_camera, 10),
            KeyCode::Home => {
                if let Ok(mut camera) = cameras.get_mut(ui.transcript_camera) {
                    camera.scroll_from_bottom = usize::MAX;
                }
            }
            KeyCode::End => {
                if let Ok(mut camera) = cameras.get_mut(ui.transcript_camera) {
                    camera.scroll_from_bottom = 0;
                }
            }
            _ => {}
        }
    }
}

fn handle_mouse_input(
    mut messages: MessageReader<MouseMessage>,
    views: Query<&TuiView>,
    mut cameras: Query<&mut TranscriptCamera>,
) {
    let Ok(ui) = views.single() else {
        return;
    };

    for message in messages.read() {
        match message.kind {
            MouseEventKind::ScrollUp => scroll_up(&mut cameras, ui.transcript_camera, 3),
            MouseEventKind::ScrollDown => scroll_down(&mut cameras, ui.transcript_camera, 3),
            _ => {}
        }
    }
}

fn scroll_up(cameras: &mut Query<&mut TranscriptCamera>, camera: Entity, lines: usize) {
    if let Ok(mut camera) = cameras.get_mut(camera) {
        camera.scroll_from_bottom = camera.scroll_from_bottom.saturating_add(lines);
    }
}

fn scroll_down(cameras: &mut Query<&mut TranscriptCamera>, camera: Entity, lines: usize) {
    if let Ok(mut camera) = cameras.get_mut(camera) {
        camera.scroll_from_bottom = camera.scroll_from_bottom.saturating_sub(lines);
    }
}

fn sync_active_session(
    active_session: Res<ActiveSession>,
    views: Query<&TuiView>,
    mut cameras: Query<&mut TranscriptCamera>,
) {
    if !active_session.is_changed() {
        return;
    }
    let Ok(ui) = views.single() else {
        return;
    };
    if let Ok(mut camera) = cameras.get_mut(ui.transcript_camera) {
        camera.session = active_session.0;
    }
}

fn transcript_lines(items: &[TranscriptItem]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    for item in items {
        let color = match item.kind {
            TranscriptKind::User => Color::Cyan,
            TranscriptKind::Assistant => Color::Green,
            TranscriptKind::Tool => Color::Yellow,
        };
        lines.push(Line::from(Span::styled(
            format!(" {} ", item.title),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        )));
        lines.extend(
            item.body
                .lines()
                .map(|line| Line::from(format!("  {line}"))),
        );
        lines.push(Line::default());
    }

    lines
}

fn draw(
    mut context: ResMut<RatatuiContext>,
    mut views: Query<&mut TuiView>,
    mut cameras: Query<&mut TranscriptCamera>,
    turns: Query<(Entity, &Turn)>,
    users: Query<&UserMessage>,
    assistants: Query<&AssistantMessage>,
) -> Result {
    let Ok(ui) = views.single_mut() else {
        return Ok(());
    };
    let Ok(mut camera) = cameras.get_mut(ui.transcript_camera) else {
        return Ok(());
    };
    let items = project_transcript_parts(*camera, turns.iter(), users.iter(), assistants.iter());

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
            Span::styled("agent harness", Style::default().fg(Color::DarkGray)),
            Span::raw("  "),
            Span::styled("● fake", Style::default().fg(Color::Yellow)),
        ]))
        .block(Block::default().borders(Borders::ALL));
        frame.render_widget(header, areas[0]);

        let lines = transcript_lines(&items);
        let visible_height = usize::from(areas[1].height.saturating_sub(2));
        let max_scroll = lines.len().saturating_sub(visible_height);
        camera.scroll_from_bottom = camera.scroll_from_bottom.min(max_scroll);
        let scroll = max_scroll.saturating_sub(camera.scroll_from_bottom);
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
                .border_style(Style::default().fg(Color::Cyan))
                .title(" Message "),
        );
        frame.render_widget(composer, areas[2]);

        let help = Line::from(" Enter send  Wheel/↑/↓ scroll  PgUp/PgDn page  Esc quit ")
            .style(Style::default().fg(Color::DarkGray));
        frame.render_widget(help, areas[3]);

        let cursor_x = areas[2]
            .x
            .saturating_add(1)
            .saturating_add(ui.composer.chars().count() as u16)
            .min(areas[2].right().saturating_sub(2));
        frame.set_cursor_position((cursor_x, areas[2].y.saturating_add(1)));
    })?;

    Ok(())
}
