mod app;
mod theme;
mod ui;

use anyhow::Result;
use app::{App, Screen};
use clap::Parser;
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::io::{self, stdout};
use std::time::Duration;
use sui_clmm_manager::store::expand_tilde;

#[derive(Debug, Parser)]
#[command(
    name = "sui-clmm-tui",
    about = "Sui CLMM terminal — wallet positions (Bluefin · Momentum · Cetus · Turbos)"
)]
struct Cli {
    /// SQLite database path
    #[arg(long, env = "SUI_CLMM_DB")]
    db: Option<String>,

    /// Sui GraphQL RPC URL
    #[arg(long, env = "SUI_GRAPHQL_URL")]
    graphql_url: Option<String>,

    /// Wallet address (read-only position list)
    #[arg(long, env = "SUI_ADDRESS")]
    address: Option<String>,
}

fn main() -> Result<()> {
    color_eyre::install().map_err(|e| anyhow::anyhow!(e))?;
    let cli = Cli::parse();
    let db = cli.db.as_deref().map(expand_tilde);

    let mut app = App::new(db, cli.graphql_url, cli.address);

    enable_raw_mode()?;
    let mut stdout = stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_loop(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    result
}

fn run_loop(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>, app: &mut App) -> Result<()> {
    loop {
        app.poll_work();
        terminal.draw(|f| ui::draw(f, app))?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
        {
            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                break;
            }
            if key.code == KeyCode::Char('q')
                && matches!(
                    app.screen,
                    Screen::Positions | Screen::DbSetup | Screen::Setup | Screen::Settings
                )
                && !app.filter_mode
                && !app.editing
                && !app.busy
            {
                break;
            }
            app.handle_key(key);
        }
    }
    Ok(())
}
