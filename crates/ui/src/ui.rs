use std::io::{self, Stdout};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph};
use ratatui::Terminal;


pub struct UI {
    started_at: Instant,
    tick_count: u64,
    load: u16, // 0-100, pretend "CPU load" style value
}

impl UI {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            tick_count: 0,
            load: 10,
        }
    }

    fn on_tick(&mut self) {
        self.tick_count += 1;

        // Fake a wandering load value just so something visibly moves.
        let wave = ((self.tick_count as f64 * 0.3).sin() * 40.0 + 50.0) as i32;
        self.load = wave.clamp(0, 100) as u16;
    }

    pub fn run() -> io::Result<()> {
        let mut terminal = Self::setup_terminal()?;
        let result = Self::run_app(&mut terminal);
        Self::restore_terminal(&mut terminal)?;

        if let Err(err) = result {
            eprintln!("Error: {err}");
        }
        Ok(())
    }

    fn run_app(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        let mut app = UI::new();
        let tick_rate = Duration::from_millis(250);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|frame| Self::draw_ui(frame, &app))?;

            // Wait for either a key press or the next tick, whichever comes first.
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                        _ => {}
                    }
                }
            }

            if last_tick.elapsed() >= tick_rate {
                app.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    fn draw_ui(frame: &mut ratatui::Frame, app: &UI) {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3), // title
                Constraint::Length(3), // gauge
                Constraint::Min(0),    // info block
            ])
            .split(frame.size());

        // Title
        let title = Paragraph::new("ratatui live demo — press 'q' to quit")
            .block(Block::default().borders(Borders::ALL).title("Demo"));
        frame.render_widget(title, chunks[0]);

        // A gauge that moves on its own each tick
        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Load"))
            .gauge_style(Style::default().fg(Color::Cyan))
            .percent(app.load);
        frame.render_widget(gauge, chunks[1]);

        // Some changing text values
        let uptime = app.started_at.elapsed().as_secs();
        let lines = vec![
            Line::from(vec![
                Span::styled("Uptime: ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{uptime}s")),
            ]),
            Line::from(vec![
                Span::styled("Ticks: ", Style::default().fg(Color::Gray)),
                Span::raw(app.tick_count.to_string()),
            ]),
            Line::from(vec![
                Span::styled("Load: ", Style::default().fg(Color::Gray)),
                Span::raw(format!("{}%", app.load)),
            ]),
        ];
        let info = Paragraph::new(lines).block(Block::default().borders(Borders::ALL).title("Info"));
        frame.render_widget(info, chunks[2]);
    }

    fn setup_terminal() -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;
        Terminal::new(CrosstermBackend::new(stdout))
    }

    fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
        terminal.show_cursor()
    }

}