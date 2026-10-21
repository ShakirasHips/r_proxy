use std::collections::VecDeque;
use std::io::{self, Stdout};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Style};
use ratatui::widgets::{Block, Borders, Paragraph, Row, Sparkline, Table};
use ratatui::{Frame, Terminal};

use tui_logger::{TuiLoggerWidget, TuiWidgetState};
use proxy::HealthProber;

const THROUGHPUT_SAMPLES: usize = 240;

pub struct UI {
    started_at: Instant,
    health_prober: Arc<HealthProber>,
    log_state: TuiWidgetState,
    last_sample: (Instant, u64),
    throughput: VecDeque<u64>,
}

impl UI {
    fn new(health_prober: Arc<HealthProber>) -> Self {
        let now = Instant::now();
        let total = health_prober.total_bytes_sent();
        Self {
            started_at: now,
            health_prober,
            log_state: TuiWidgetState::new().set_default_display_level(log::LevelFilter::Trace),
            last_sample: (now, total),
            throughput: VecDeque::with_capacity(THROUGHPUT_SAMPLES),
        }
    }

    fn on_tick(&mut self) {
        let now = Instant::now();
        let total = self.health_prober.total_bytes_sent();
        let (then, last_total) = self.last_sample;
        let secs = now.duration_since(then).as_secs_f64();
        if secs > 0.0 {
            if self.throughput.len() == THROUGHPUT_SAMPLES {
                self.throughput.pop_front();
            }
            self.throughput
                .push_back((total.saturating_sub(last_total) as f64 / secs) as u64);
        }
        self.last_sample = (now, total);
    }

    pub fn init_logging() {
        tui_logger::init_logger(log::LevelFilter::Trace)
            .expect("failed to init tui_logger");
        tui_logger::set_default_level(log::LevelFilter::Trace);
        for noisy in ["mio::poll", "tracing::span", "tracing::span::active"] {
            tui_logger::set_level_for_target(noisy, log::LevelFilter::Info);
        }
        tui_logger::set_log_file(tui_logger::TuiLoggerFile::new("app.log"));
    }

    pub fn flush_logs() {
        tui_logger::move_events();
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

    pub fn run(health_prober: Arc<HealthProber>) -> io::Result<()> {
        let mut app = UI::new(health_prober);
        Self::init_panic_hook();
        let mut terminal = Self::setup_terminal()?;
        let result = app.run_app(&mut terminal);
        Self::restore_terminal(&mut terminal)?;
        result
    }

    fn run_app(&mut self, terminal: &mut Terminal<CrosstermBackend<Stdout>>) -> io::Result<()> {
        let tick_rate = Duration::from_millis(250);
        let mut last_tick = Instant::now();

        loop {
            terminal.draw(|frame| self.draw_ui(frame))?;

            // Wait for either a key press or the next tick, whichever comes first.
            let timeout = tick_rate.saturating_sub(last_tick.elapsed());
            if event::poll(timeout)?
                && let Event::Key(key) = event::read()?
                && key.kind == KeyEventKind::Press
            {
                match key.code {
                    KeyCode::Char('q') | KeyCode::Esc => return Ok(()),
                    // raw mode swallows SIGINT
                    KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                        return Ok(());
                    }
                    _ => {}
                }
            }

            if last_tick.elapsed() >= tick_rate {
                self.on_tick();
                last_tick = Instant::now();
            }
        }
    }

    fn draw_ui(&self, frame: &mut Frame) {
        let vertical_chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Length(3),
                Constraint::Length(5),
                Constraint::Min(0),
            ])
            .split(frame.area());

        let horizontal_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Ratio(1, 2), Constraint::Ratio(1, 2)])
            .split(vertical_chunks[2]);

        let uptime = self.started_at.elapsed().as_secs();
        let title = Paragraph::new(format!(
            "uptime {:02}:{:02}:{:02}   ·   q / Esc / Ctrl+C to quit",
            uptime / 3600,
            uptime / 60 % 60,
            uptime % 60
        ))
        .block(Block::default().borders(Borders::ALL).title("r_proxy"));
        frame.render_widget(title, vertical_chunks[0]);

        self.render_throughput(frame, vertical_chunks[1]);
        self.render_data_table(frame, horizontal_chunks[0]);

        let logs = TuiLoggerWidget::default()
            .block(Block::bordered().title("Logs"))
            .state(&self.log_state)
            .output_separator('|');
        frame.render_widget(logs, horizontal_chunks[1]);
    }

    fn render_throughput(&self, frame: &mut Frame, area: Rect) {
        let current = self.throughput.back().copied().unwrap_or(0);
        let visible = area.width.saturating_sub(2) as usize;
        let data: Vec<u64> = self
            .throughput
            .iter()
            .skip(self.throughput.len().saturating_sub(visible))
            .copied()
            .collect();

        let sparkline = Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("Throughput {}/s", format_bytes(current))),
            )
            .style(Style::default().fg(Color::LightBlue))
            .data(&data);
        frame.render_widget(sparkline, area);
    }

    fn render_data_table(&self, frame: &mut Frame, area: Rect) {
        let rows: Vec<Row> = self
            .health_prober
            .summary()
            .into_iter()
            .map(|s| {
                Row::new(vec![
                    s.id,
                    s.active_connections.to_string(),
                    s.total_connections.to_string(),
                    format_bytes(s.total_bytes_sent),
                ])
            })
            .collect();

        let widths = [
            Constraint::Min(10),
            Constraint::Length(8),
            Constraint::Length(12),
            Constraint::Length(12),
        ];
        let table = Table::new(rows, widths)
            .header(Row::new(vec!["ID", "Active", "Connections", "Bytes Sent"]))
            .block(Block::default().borders(Borders::ALL).title("Data"));

        frame.render_widget(table, area);
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

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::format_bytes;

    #[test]
    fn test_format_bytes() {
        assert_eq!(format_bytes(0), "0 B");
        assert_eq!(format_bytes(1023), "1023 B");
        assert_eq!(format_bytes(1536), "1.5 KiB");
        assert_eq!(format_bytes(5 * 1024 * 1024), "5.0 MiB");
    }
}
