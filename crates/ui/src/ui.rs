use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Gauge, Paragraph, Row, Table};
use ratatui::{Frame, Terminal};

use tracing_subscriber::EnvFilter;
use tui_logger::{TuiLoggerWidget, TuiWidgetState};
use proxy::HealthProber;

pub struct UI {
    started_at: Instant,
    tick_count: u64,
    load: u16,
    log_guard : Option<tracing_appender::non_blocking::WorkerGuard>,
    health_prober: Option<Arc<HealthProber>>,
}

impl UI {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
            tick_count: 0,
            load: 10,
            log_guard : None,
            health_prober : None
        }
    }

    fn on_tick(&mut self) {
        self.tick_count += 1;
        let wave = ((self.tick_count as f64 * 0.3).sin() * 40.0 + 50.0) as i32;
        self.load = wave.clamp(0, 100) as u16;
    }

    fn init_logging() {
        tui_logger::init_logger(log::LevelFilter::Trace)
            .expect("failed to init tui_logger");
        tui_logger::set_default_level(log::LevelFilter::Trace);
        tui_logger::set_log_file(tui_logger::TuiLoggerFile::new("app.log"));
    }
    fn init_panic_hook() {
        let original_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |panic_info| {
            let _ = disable_raw_mode();
            let _ = execute!(io::stdout(), LeaveAlternateScreen);
            log::error!("panic: {panic_info}");
            original_hook(panic_info);
        }));
    }

    pub fn run(health_prober : Arc<HealthProber>) -> io::Result<()> {
        let mut app = UI::new();
        app.health_prober = Some(health_prober);
        let mut terminal = Self::setup_terminal(&mut app)?;
        let result = Self::run_app(&mut app, &mut terminal);
        Self::restore_terminal(&mut terminal)?;

        if let Err(err) = result {
            eprintln!("Error: {err}");
        }
        Ok(())
    }

    fn run_app(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        let tick_rate = Duration::from_millis(250);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|frame| Self::draw_ui(frame, self))?;

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
                self.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    fn draw_ui(frame: &mut ratatui::Frame, app: &UI) {
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(3),
                Constraint::Ratio(1, 2),
            ])
            .split(frame.size());

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(vertical_chunks[2]);

        // Title
        let title = Paragraph::new("Proxy stuff")
            .block(Block::default().borders(Borders::ALL).title("Demo"));
        frame.render_widget(title, vertical_chunks[0]);

        let gauge = Gauge::default()
            .block(Block::default().borders(Borders::ALL).title("Load"))
            .gauge_style(Style::default().fg(Color::LightBlue))
            .percent(app.load);
        frame.render_widget(gauge, vertical_chunks[1]);

        let state = TuiWidgetState::new().set_default_display_level(log::LevelFilter::Trace);

        let logs = TuiLoggerWidget::default()
            .block(Block::bordered().title("Logs"))
            .state(&state)
            .output_separator('|');
        frame.render_widget(logs, horizontal_chunks[1]);
        Self::render_data_table(frame, horizontal_chunks[0], app);
    }

    fn render_data_table(frame: &mut Frame, area: Rect, app: &UI) {
        let rows: Vec<Row> = app.health_prober.as_ref().unwrap().summary()
            .iter()
            .map(|(id, total)| {
                let id = id.to_string();
                let total = total.to_string();
                Row::new(vec![id, total])
            })
            .collect();

        let widths = [Constraint::Length(10), Constraint::Length(20)];
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["ID", "Total Bytes Sent"]))
            .block(Block::default().borders(Borders::ALL).title("Data"));

        frame.render_widget(table, area);
    }

    fn setup_terminal(&mut self) -> io::Result<Terminal<CrosstermBackend<Stdout>>> {
        Self::init_logging();
        Self::init_panic_hook();
        tui_logger::set_log_file(tui_logger::TuiLoggerFile::new("app.log"));
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