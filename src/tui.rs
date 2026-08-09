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

use crate::MajinSet;

pub struct TuiPlugin;

impl Plugin for TuiPlugin {
    fn build(&self, app: &mut App) {
        app.add_message::<AppExit>()
            .add_message::<KeyMessage>()
            .add_message::<MouseMessage>()
            .init_resource::<TuiView>()
            .add_systems(
                Update,
                (handle_input, handle_mouse_input).in_set(MajinSet::Input),
            )
            .add_systems(
                Update,
                draw.in_set(MajinSet::Render)
                    .run_if(resource_exists::<RatatuiContext>),
            );
    }
}

#[derive(Clone, Copy)]
pub enum TranscriptKind {
    User,
    Assistant,
    Tool,
}

pub struct TranscriptItem {
    pub kind: TranscriptKind,
    pub title: &'static str,
    pub body: String,
}

#[derive(Resource)]
pub struct TuiView {
    pub composer: String,
    pub transcript: Vec<TranscriptItem>,
    pub scroll_from_bottom: usize,
}

impl Default for TuiView {
    fn default() -> Self {
        Self {
            composer: String::new(),
            transcript: vec![
                TranscriptItem {
                    kind: TranscriptKind::User,
                    title: "YOU",
                    body: "Show me the current project structure.".into(),
                },
                TranscriptItem {
                    kind: TranscriptKind::Assistant,
                    title: "MAJIN",
                    body: "I will inspect the source tree first.".into(),
                },
                TranscriptItem {
                    kind: TranscriptKind::Tool,
                    title: "TOOL CALL",
                    body: "find src/**/*".into(),
                },
                TranscriptItem {
                    kind: TranscriptKind::Tool,
                    title: "TOOL RESULT",
                    body: "src/main.rs".into(),
                },
                TranscriptItem {
                    kind: TranscriptKind::Assistant,
                    title: "MAJIN",
                    body: "The project currently has one application source file.".into(),
                },
            ],
            scroll_from_bottom: 0,
        }
    }
}

impl TuiView {
    fn handle_key(&mut self, code: KeyCode) -> bool {
        match code {
            KeyCode::Esc => return true,
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.composer.pop();
            }
            KeyCode::Char(character) => self.composer.push(character),
            KeyCode::Up => self.scroll_up(1),
            KeyCode::Down => self.scroll_down(1),
            KeyCode::PageUp => self.scroll_up(10),
            KeyCode::PageDown => self.scroll_down(10),
            KeyCode::Home => self.scroll_from_bottom = usize::MAX,
            KeyCode::End => self.scroll_from_bottom = 0,
            _ => {}
        }

        false
    }

    fn scroll_up(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_add(lines);
    }

    fn scroll_down(&mut self, lines: usize) {
        self.scroll_from_bottom = self.scroll_from_bottom.saturating_sub(lines);
    }

    fn submit(&mut self) {
        let message = self.composer.trim();
        if message.is_empty() {
            return;
        }

        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::User,
            title: "YOU",
            body: message.to_owned(),
        });
        self.transcript.push(TranscriptItem {
            kind: TranscriptKind::Assistant,
            title: "MAJIN",
            body: "Fake harness received the message. No agent is connected yet.".into(),
        });
        self.composer.clear();
        self.scroll_from_bottom = 0;
    }
}

fn handle_input(
    mut messages: MessageReader<KeyMessage>,
    mut ui: ResMut<TuiView>,
    mut exit: MessageWriter<AppExit>,
) {
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
        if ui.handle_key(message.code) {
            exit.write_default();
        }
    }
}

fn handle_mouse_input(mut messages: MessageReader<MouseMessage>, mut ui: ResMut<TuiView>) {
    for message in messages.read() {
        match message.kind {
            MouseEventKind::ScrollUp => ui.scroll_up(3),
            MouseEventKind::ScrollDown => ui.scroll_down(3),
            _ => {}
        }
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

fn draw(mut context: ResMut<RatatuiContext>, mut ui: ResMut<TuiView>) -> Result {
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

        let lines = transcript_lines(&ui.transcript);
        let visible_height = usize::from(areas[1].height.saturating_sub(2));
        let max_scroll = lines.len().saturating_sub(visible_height);
        ui.scroll_from_bottom = ui.scroll_from_bottom.min(max_scroll);
        let scroll = max_scroll.saturating_sub(ui.scroll_from_bottom);
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
