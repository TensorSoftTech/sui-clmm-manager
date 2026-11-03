use anyhow::Result;
use ratatui::widgets::TableState;
use sui_clmm_client::Client;
use sui_clmm_manager::graphql::{self, GraphQlMeter};
use sui_clmm_manager::store::{
    self, AppSettings, Store, default_db_path, display_path, expand_tilde, write_active_pointer,
};
use sui_clmm_manager::{
    CHART_HALF_SPACINGS_DEFAULT, CHART_HALF_SPACINGS_MIN, PositionDetail, PositionRow,
    PositionSnapshotRow, VenueFilter, short_id,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::Instant;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    DbSetup,
    Setup,
    Positions,
    Detail,
    History,
    Settings,
    SettingsGraphql,
    SettingsWallet,
    Help,
}

#[derive(Debug, Clone, Default)]
pub enum HistoryFilter {
    #[default]
    All,
    Position {
        object_id: String,
        label: String,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DbPathChoice {
    UseDefault,
    CustomPath,
}

impl DbPathChoice {
    pub const ALL: [DbPathChoice; 2] = [DbPathChoice::UseDefault, DbPathChoice::CustomPath];

    pub fn label(self, default_path: &str) -> String {
        match self {
            Self::UseDefault => format!("default · {default_path}"),
            Self::CustomPath => "custom path".into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SetupField {
    Wallet,
    Graphql,
    Done,
}

impl SetupField {
    pub const ALL: [SetupField; 3] = [SetupField::Wallet, SetupField::Graphql, SetupField::Done];

    pub fn label(self) -> &'static str {
        match self {
            Self::Wallet => "Wallet address",
            Self::Graphql => "GraphQL URL",
            Self::Done => "Continue → load positions",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Wallet => Self::Graphql,
            Self::Graphql => Self::Done,
            Self::Done => Self::Wallet,
        }
    }
}

enum WorkResult {
    List {
        result: Result<Vec<PositionRow>>,
        meter: GraphQlMeter,
    },
    Detail {
        result: Result<PositionDetail>,
        meter: GraphQlMeter,
    },
}

pub struct App {
    pub screen: Screen,
    pub store: Option<Store>,
    pub settings: AppSettings,
    pub meter: GraphQlMeter,
    pub positions: Vec<PositionRow>,
    pub table: TableState,
    pub venue_filter: VenueFilter,
    pub symbol_query: String,
    pub filter_mode: bool,
    pub detail: Option<PositionDetail>,
    pub status: String,
    pub error: Option<String>,
    pub input: String,
    pub db_choice_idx: usize,
    pub setup_field: SetupField,
    pub editing: bool,
    rx: Option<Receiver<WorkResult>>,
    pub busy: bool,
    /// When the current bg fetch started (animates the Sol-style load bar).
    pub load_started: Option<Instant>,
    /// What the bar is loading (label).
    pub load_label: String,
    pub show_full_id: bool,
    /// Detail chart zoom (± tick spacings around current).
    pub detail_chart_half_spacings: i32,
    pub history: Vec<PositionSnapshotRow>,
    pub history_table: TableState,
    pub history_filter: HistoryFilter,
    pub history_return: Screen,
}

impl App {
    pub fn new(
        cli_db: Option<PathBuf>,
        graphql_url: Option<String>,
        wallet: Option<String>,
    ) -> Self {
        let mut app = Self {
            screen: Screen::DbSetup,
            store: None,
            settings: AppSettings {
                graphql_url: graphql_url.or_else(|| Some(graphql::DEFAULT_GRAPHQL_URL.into())),
                wallet_address: wallet,
                ..Default::default()
            },
            meter: GraphQlMeter::default(),
            positions: Vec::new(),
            table: TableState::default(),
            venue_filter: VenueFilter::All,
            symbol_query: String::new(),
            filter_mode: false,
            detail: None,
            status: "pick a database".into(),
            error: None,
            input: String::new(),
            db_choice_idx: 0,
            setup_field: SetupField::Wallet,
            editing: false,
            rx: None,
            busy: false,
            load_started: None,
            load_label: String::new(),
            show_full_id: false,
            detail_chart_half_spacings: CHART_HALF_SPACINGS_DEFAULT,
            history: Vec::new(),
            history_table: TableState::default(),
            history_filter: HistoryFilter::All,
            history_return: Screen::Positions,
        };

        if let Some(path) = cli_db.or_else(|| store::resolve_existing_db(None)) {
            if let Err(e) = app.open_db(path) {
                app.error = Some(e.to_string());
            } else if app.wallet().is_some() {
                app.screen = Screen::Positions;
                app.start_list();
            } else {
                app.screen = Screen::Setup;
                app.status = "set wallet address".into();
            }
        }
        app
    }

    pub fn default_db_display() -> String {
        display_path(&default_db_path())
    }

    pub fn open_db(&mut self, path: PathBuf) -> Result<()> {
        let store = Store::open(&path)?;
        write_active_pointer(&path)?;
        let loaded = store.load_settings()?;
        if self.settings.graphql_url.is_none() {
            self.settings.graphql_url = loaded.graphql_url;
        }
        if self.settings.wallet_address.is_none() {
            self.settings.wallet_address = loaded.wallet_address;
        }
        self.settings.privacy_trim = loaded.privacy_trim;
        self.settings.pool_limit = loaded.pool_limit;
        if !loaded.price_mode.is_empty() {
            self.settings.price_mode = loaded.price_mode;
        } else if self.settings.price_mode.is_empty() {
            self.settings.price_mode = "aftermath".into();
        }
        self.store = Some(store);
        Ok(())
    }

    pub fn graphql_url(&self) -> String {
        self.settings
            .graphql_url
            .clone()
            .unwrap_or_else(|| graphql::DEFAULT_GRAPHQL_URL.to_string())
    }

    pub fn wallet(&self) -> Option<&str> {
        self.settings
            .wallet_address
            .as_deref()
            .map(str::trim)
            .filter(|s| !s.is_empty())
    }

    pub fn wallet_label(&self) -> String {
        match self.wallet() {
            Some(w) if self.settings.privacy_trim || !self.show_full_id => short_id(w),
            Some(w) => w.to_string(),
            None => "(none)".into(),
        }
    }

    pub fn filtered_positions(&self) -> Vec<&PositionRow> {
        let q = self.symbol_query.trim().to_ascii_lowercase();
        self.positions
            .iter()
            .filter(|r| self.venue_filter.matches(r.venue))
            .filter(|r| {
                if q.is_empty() {
                    return true;
                }
                r.pair_label().to_ascii_lowercase().contains(&q)
                    || r.object_id.to_ascii_lowercase().contains(&q)
                    || r.pool_id.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected_position(&self) -> Option<PositionRow> {
        let rows = self.filtered_positions();
        self.table
            .selected()
            .and_then(|i| rows.get(i).map(|r| (*r).clone()))
    }

    pub fn save_settings(&mut self) -> Result<()> {
        if let Some(store) = self.store.as_ref() {
            store.save_settings(&self.settings)?;
        }
        Ok(())
    }

    pub fn start_list(&mut self) {
        if self.busy {
            return;
        }
        let Some(owner) = self.wallet().map(str::to_string) else {
            self.error = Some("set a wallet address first".into());
            self.screen = Screen::Setup;
            return;
        };
        let Some(store_path) = self.store.as_ref().map(|s| s.path.clone()) else {
            self.error = Some("no database open".into());
            return;
        };
        let url = self.graphql_url();
        let filter = self.venue_filter;
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.load_started = Some(Instant::now());
        self.load_label = format!("loading positions · {}", short_id(&owner));
        self.status = self.load_label.clone();
        self.error = None;

        thread::spawn(move || {
            let meter = GraphQlMeter::default();
            let result = (|| {
                let store = Store::open(&store_path)?;
                let gql = Client::connect(&url, &meter)?;
                Client::default().list_positions(&gql, Some(&store), &owner, filter)
            })();
            let _ = tx.send(WorkResult::List { result, meter });
        });
    }

    pub fn start_detail(&mut self) {
        if self.busy {
            return;
        }
        let Some(row) = self.selected_position() else {
            return;
        };
        let Some(store_path) = self.store.as_ref().map(|s| s.path.clone()) else {
            return;
        };
        let url = self.graphql_url();
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.busy = true;
        self.load_started = Some(Instant::now());
        self.load_label = format!(
            "loading {} · ticks · size…",
            row.pair_label()
        );
        self.status = self.load_label.clone();
        self.error = None;

        thread::spawn(move || {
            let meter = GraphQlMeter::default();
            let result = (|| {
                let store = Store::open(&store_path)?;
                let gql = Client::connect(&url, &meter)?;
                Client::default().position_detail(&gql, Some(&store), &row)
            })();
            let _ = tx.send(WorkResult::Detail { result, meter });
        });
    }

    /// 0..1 looping fill for the in-panel load bar (Sol twin: ~2.4s period).
    pub fn load_bar_ratio(&self) -> f64 {
        let Some(started) = self.load_started else {
            return 0.0;
        };
        const PERIOD_SECS: f64 = 2.4;
        (started.elapsed().as_secs_f64() % PERIOD_SECS) / PERIOD_SECS
    }

    pub fn poll_work(&mut self) {
        let Some(rx) = self.rx.as_ref() else {
            return;
        };
        match rx.try_recv() {
            Ok(WorkResult::List { result, meter }) => {
                self.busy = false;
                self.load_started = None;
                self.rx = None;
                self.meter.merge(&meter);
                match result {
                    Ok(rows) => {
                        if let Some(path) = self.store.as_ref().map(|s| s.path.clone())
                            && let Ok(s) = Store::open(path)
                        {
                            self.store = Some(s);
                        }
                        self.positions = rows;
                        if self.positions.is_empty() {
                            self.table.select(None);
                        } else {
                            self.table.select(Some(0));
                        }
                        self.status = format!("{} positions", self.filtered_positions().len());
                        self.screen = Screen::Positions;
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.status = "list failed".into();
                    }
                }
            }
            Ok(WorkResult::Detail { result, meter }) => {
                self.busy = false;
                self.load_started = None;
                self.rx = None;
                self.meter.merge(&meter);
                match result {
                    Ok(detail) => {
                        let step = detail.tick_spacing.max(1) as i32;
                        let span = (detail.series_max_tick - detail.series_min_tick).max(step);
                        self.detail_chart_half_spacings =
                            (span / step / 2).max(CHART_HALF_SPACINGS_MIN);
                        self.detail = Some(detail);
                        self.screen = Screen::Detail;
                        self.status = "position detail · +/- zoom · 0 fit".into();
                    }
                    Err(e) => {
                        self.error = Some(e.to_string());
                        self.status = "detail failed".into();
                    }
                }
            }
            Err(TryRecvError::Empty) => {}
            Err(TryRecvError::Disconnected) => {
                self.busy = false;
                self.load_started = None;
                self.rx = None;
            }
        }
    }

    pub fn db_size_label(&self) -> String {
        self.store
            .as_ref()
            .map(|s| s.file_size_label())
            .unwrap_or_else(|| "—".into())
    }

    pub fn chart_zoom_in(&mut self) {
        self.detail_chart_half_spacings =
            (self.detail_chart_half_spacings / 2).max(CHART_HALF_SPACINGS_MIN);
    }

    pub fn chart_zoom_out(&mut self) {
        let max = self
            .detail
            .as_ref()
            .map(|d| {
                let step = d.tick_spacing.max(1) as i32;
                let span = (d.series_max_tick - d.series_min_tick).max(step);
                (span / step / 2).max(CHART_HALF_SPACINGS_MIN)
            })
            .unwrap_or(CHART_HALF_SPACINGS_DEFAULT);
        self.detail_chart_half_spacings = (self.detail_chart_half_spacings * 2)
            .max(CHART_HALF_SPACINGS_MIN)
            .min(max.max(CHART_HALF_SPACINGS_MIN));
    }

    pub fn chart_zoom_fit(&mut self) {
        if let Some(d) = &self.detail {
            let step = d.tick_spacing.max(1) as i32;
            let span = (d.series_max_tick - d.series_min_tick).max(step);
            self.detail_chart_half_spacings = (span / step / 2).max(CHART_HALF_SPACINGS_MIN);
        }
    }

    pub fn open_history(&mut self, filter: HistoryFilter) {
        self.history_return = self.screen;
        self.history_filter = filter;
        self.reload_history();
        self.screen = Screen::History;
    }

    pub fn reload_history(&mut self) {
        let Some(store) = self.store.as_ref() else {
            self.history.clear();
            return;
        };
        let oid = match &self.history_filter {
            HistoryFilter::All => None,
            HistoryFilter::Position { object_id, .. } => Some(object_id.as_str()),
        };
        match Client::default().position_history(store, oid, 200) {
            Ok(rows) => {
                self.history = rows;
                if self.history.is_empty() {
                    self.history_table.select(None);
                } else {
                    self.history_table.select(Some(0));
                }
                self.status = format!("{} snapshots", self.history.len());
            }
            Err(e) => self.error = Some(e.to_string()),
        }
    }

    pub fn handle_key(&mut self, key: crossterm::event::KeyEvent) {
        use crossterm::event::KeyCode;

        if self.filter_mode {
            match key.code {
                KeyCode::Esc => self.filter_mode = false,
                KeyCode::Enter => self.filter_mode = false,
                KeyCode::Backspace => {
                    self.symbol_query.pop();
                }
                KeyCode::Char(c) => self.symbol_query.push(c),
                _ => {}
            }
            return;
        }

        match self.screen {
            Screen::DbSetup => self.handle_db_setup(key.code),
            Screen::Setup => self.handle_setup(key.code),
            Screen::Positions => self.handle_positions(key.code),
            Screen::Detail => self.handle_detail(key.code),
            Screen::History => self.handle_history(key.code),
            Screen::Settings | Screen::SettingsGraphql | Screen::SettingsWallet => {
                self.handle_settings(key.code)
            }
            Screen::Help => {
                if matches!(key.code, KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('?')) {
                    self.screen = Screen::Positions;
                }
            }
        }
    }

    fn handle_db_setup(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.db_choice_idx = self.db_choice_idx.saturating_sub(1);
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.db_choice_idx = (self.db_choice_idx + 1).min(DbPathChoice::ALL.len() - 1);
            }
            KeyCode::Enter => {
                if self.editing {
                    let path = expand_tilde(self.input.trim());
                    match self.open_db(path) {
                        Ok(()) => {
                            self.editing = false;
                            self.after_db_open();
                        }
                        Err(e) => self.error = Some(e.to_string()),
                    }
                } else {
                    match DbPathChoice::ALL[self.db_choice_idx] {
                        DbPathChoice::UseDefault => match self.open_db(default_db_path()) {
                            Ok(()) => self.after_db_open(),
                            Err(e) => self.error = Some(e.to_string()),
                        },
                        DbPathChoice::CustomPath => {
                            self.editing = true;
                            self.input.clear();
                            self.status = "enter db path".into();
                        }
                    }
                }
            }
            KeyCode::Char(c) if self.editing => self.input.push(c),
            KeyCode::Backspace if self.editing => {
                self.input.pop();
            }
            KeyCode::Esc if self.editing => {
                self.editing = false;
                self.input.clear();
            }
            _ => {}
        }
    }

    fn after_db_open(&mut self) {
        if self.wallet().is_some() {
            self.screen = Screen::Positions;
            self.start_list();
        } else {
            self.screen = Screen::Setup;
            self.status = "set wallet address".into();
        }
    }

    fn handle_setup(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        if self.editing {
            match code {
                KeyCode::Esc => {
                    self.editing = false;
                    self.input.clear();
                }
                KeyCode::Enter => {
                    match self.setup_field {
                        SetupField::Wallet => {
                            let t = self.input.trim().to_string();
                            self.settings.wallet_address = if t.is_empty() { None } else { Some(t) };
                        }
                        SetupField::Graphql => {
                            self.settings.graphql_url = Some(self.input.trim().to_string());
                        }
                        SetupField::Done => {}
                    }
                    let _ = self.save_settings();
                    self.editing = false;
                    self.input.clear();
                }
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Up | KeyCode::Char('k') => {
                self.setup_field = match self.setup_field {
                    SetupField::Wallet => SetupField::Done,
                    SetupField::Graphql => SetupField::Wallet,
                    SetupField::Done => SetupField::Graphql,
                };
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                self.setup_field = self.setup_field.next();
            }
            KeyCode::Enter => {
                if self.setup_field == SetupField::Done {
                    if self.wallet().is_none() {
                        self.error = Some("wallet address required".into());
                        self.setup_field = SetupField::Wallet;
                        return;
                    }
                    let _ = self.save_settings();
                    self.start_list();
                } else {
                    self.editing = true;
                    self.input = match self.setup_field {
                        SetupField::Wallet => {
                            self.settings.wallet_address.clone().unwrap_or_default()
                        }
                        SetupField::Graphql => self.graphql_url(),
                        SetupField::Done => String::new(),
                    };
                }
            }
            _ => {}
        }
    }

    fn handle_positions(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        let len = self.filtered_positions().len();
        match code {
            KeyCode::Char('?') => self.screen = Screen::Help,
            KeyCode::Char('s') => self.screen = Screen::Settings,
            KeyCode::Char('p') => {
                self.show_full_id = !self.show_full_id;
                if let Some(row) = self.selected_position() {
                    self.status = if self.show_full_id {
                        format!("pos {} · pool {}", row.object_id, row.pool_id)
                    } else {
                        format!(
                            "pos {} · pool {}",
                            short_id(&row.object_id),
                            short_id(&row.pool_id)
                        )
                    };
                }
            }
            KeyCode::Char('v') => {
                self.venue_filter = self.venue_filter.next();
                // skip deepbook in cycle for positions UX? keep as-is; deepbook returns empty
                self.table.select(if len > 0 { Some(0) } else { None });
                self.status = format!("venue · {}", self.venue_filter.label());
            }
            KeyCode::Char('/') => {
                self.filter_mode = true;
                self.status = "filter pair / id".into();
            }
            KeyCode::Char('r') => self.start_list(),
            KeyCode::Char('y') => self.open_history(HistoryFilter::All),
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.table.selected().unwrap_or(0);
                self.table.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.table.selected().unwrap_or(0);
                let next = if len == 0 {
                    0
                } else {
                    (i + 1).min(len - 1)
                };
                self.table.select(Some(next));
            }
            KeyCode::Enter => self.start_detail(),
            _ => {}
        }
    }

    fn handle_detail(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.screen = Screen::Positions;
                self.detail = None;
            }
            KeyCode::Char('r') => self.start_detail(),
            KeyCode::Char('+') | KeyCode::Char('=') => {
                self.chart_zoom_in();
                self.status = format!("zoom ±{}", self.detail_chart_half_spacings);
            }
            KeyCode::Char('-') | KeyCode::Char('_') => {
                self.chart_zoom_out();
                self.status = format!("zoom ±{}", self.detail_chart_half_spacings);
            }
            KeyCode::Char('0') => {
                self.chart_zoom_fit();
                self.status = format!("fit ±{}", self.detail_chart_half_spacings);
            }
            KeyCode::Char('y') => {
                if let Some(d) = &self.detail {
                    let object_id = d.row.object_id.clone();
                    let label = d.row.pair_label();
                    self.open_history(HistoryFilter::Position { object_id, label });
                }
            }
            KeyCode::Char('p') => {
                self.show_full_id = !self.show_full_id;
                if let Some(d) = &self.detail {
                    self.status = if self.show_full_id {
                        format!("pos {} · pool {}", d.row.object_id, d.row.pool_id)
                    } else {
                        format!(
                            "pos {} · pool {}",
                            short_id(&d.row.object_id),
                            short_id(&d.row.pool_id)
                        )
                    };
                }
            }
            KeyCode::Char('?') => self.screen = Screen::Help,
            _ => {}
        }
    }

    fn handle_history(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Backspace => {
                self.screen = self.history_return;
            }
            KeyCode::Char('r') => self.reload_history(),
            KeyCode::Up | KeyCode::Char('k') => {
                let i = self.history_table.selected().unwrap_or(0);
                self.history_table.select(Some(i.saturating_sub(1)));
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let i = self.history_table.selected().unwrap_or(0);
                let next = (i + 1).min(self.history.len().saturating_sub(1));
                self.history_table.select(Some(next));
            }
            _ => {}
        }
    }

    fn handle_settings(&mut self, code: crossterm::event::KeyCode) {
        use crossterm::event::KeyCode;
        if self.screen == Screen::SettingsGraphql || self.screen == Screen::SettingsWallet {
            match code {
                KeyCode::Esc => {
                    self.screen = Screen::Settings;
                    self.editing = false;
                    self.input.clear();
                }
                KeyCode::Enter => {
                    if self.screen == Screen::SettingsGraphql {
                        self.settings.graphql_url = Some(self.input.trim().to_string());
                    } else {
                        let t = self.input.trim().to_string();
                        self.settings.wallet_address = if t.is_empty() { None } else { Some(t) };
                    }
                    let _ = self.save_settings();
                    self.screen = Screen::Settings;
                    self.editing = false;
                    self.input.clear();
                    self.status = "saved".into();
                }
                KeyCode::Char(c) => self.input.push(c),
                KeyCode::Backspace => {
                    self.input.pop();
                }
                _ => {}
            }
            return;
        }
        match code {
            KeyCode::Esc | KeyCode::Char('q') => self.screen = Screen::Positions,
            KeyCode::Char('g') => {
                self.screen = Screen::SettingsGraphql;
                self.editing = true;
                self.input = self.graphql_url();
            }
            KeyCode::Char('w') => {
                self.screen = Screen::SettingsWallet;
                self.editing = true;
                self.input = self.settings.wallet_address.clone().unwrap_or_default();
            }
            KeyCode::Char('t') => {
                self.settings.privacy_trim = !self.settings.privacy_trim;
                let _ = self.save_settings();
                self.status = format!(
                    "privacy trim {}",
                    if self.settings.privacy_trim {
                        "on"
                    } else {
                        "off"
                    }
                );
            }
            KeyCode::Char('u') => {
                use sui_clmm_manager::PriceMode;
                let next = PriceMode::from_setting(&self.settings.price_mode).next();
                self.settings.price_mode = next.label().into();
                let _ = self.save_settings();
                self.status = format!("usd prices {}", next.label());
            }
            KeyCode::Char('r') => self.start_list(),
            _ => {}
        }
    }
}
