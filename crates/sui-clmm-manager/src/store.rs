//! Local SQLite config + prefetched pool snapshots.

use crate::pool::PoolRow;
use crate::position::{PositionDetail, PositionSnapshotRow};
use crate::venue::Venue;
use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub const APP_DIR_NAME: &str = "sui-clmm-manager";
pub const DEFAULT_DB_FILE: &str = "sui-clmm.db";
pub const ACTIVE_POINTER_FILE: &str = "active_db";

const KEY_GRAPHQL: &str = "graphql_url";
/// Wallet address for read-only position list (like Sol `wallet_pubkey`).
const KEY_WALLET: &str = "wallet_address";
const KEY_POOL_LIMIT: &str = "pool_limit";
const KEY_PRIVACY: &str = "privacy_trim";
const KEY_PRICE_MODE: &str = "price_mode";

#[derive(Debug, Clone, Default)]
pub struct AppSettings {
    pub graphql_url: Option<String>,
    /// Sui address (0x…) — positions owner. Never a secret.
    pub wallet_address: Option<String>,
    pub pool_limit: Option<u32>,
    pub privacy_trim: bool,
    /// `aftermath` (default) | `off` — Sol twin uses jupiter/titan.
    pub price_mode: String,
}

#[derive(Debug, Clone)]
pub struct CachedToken {
    pub coin_type: String,
    pub symbol: String,
    pub decimals: u8,
}

pub struct Store {
    pub path: PathBuf,
    conn: Connection,
}

impl Store {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).context("create db dir")?;
        }
        let conn = Connection::open(&path).context("open sqlite")?;
        let store = Self { path, conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn db_size_bytes(&self) -> u64 {
        fs::metadata(&self.path).map(|m| m.len()).unwrap_or(0)
    }

    pub fn file_size_bytes(&self) -> Option<u64> {
        fs::metadata(&self.path).ok().map(|m| m.len())
    }

    pub fn file_size_label(&self) -> String {
        match self.file_size_bytes() {
            Some(n) if n >= 1_048_576 => format!("{:.1} MB", n as f64 / 1_048_576.0),
            Some(n) if n >= 1024 => format!("{:.1} KB", n as f64 / 1024.0),
            Some(n) => format!("{n} B"),
            None => "—".into(),
        }
    }

    fn migrate(&self) -> Result<()> {
        self.conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS settings (
                key TEXT PRIMARY KEY NOT NULL,
                value TEXT NOT NULL
            );
            CREATE TABLE IF NOT EXISTS tokens (
                coin_type TEXT PRIMARY KEY NOT NULL,
                symbol TEXT NOT NULL,
                decimals INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE TABLE IF NOT EXISTS pool_snapshots (
                object_id TEXT PRIMARY KEY NOT NULL,
                venue TEXT NOT NULL,
                name TEXT NOT NULL,
                coin_type_a TEXT NOT NULL,
                coin_type_b TEXT NOT NULL,
                symbol_a TEXT NOT NULL,
                symbol_b TEXT NOT NULL,
                decimals_a INTEGER NOT NULL,
                decimals_b INTEGER NOT NULL,
                fee_rate INTEGER NOT NULL,
                tick_spacing INTEGER NOT NULL,
                liquidity TEXT NOT NULL,
                coin_a TEXT NOT NULL,
                coin_b TEXT NOT NULL,
                current_tick INTEGER NOT NULL,
                sqrt_price TEXT NOT NULL,
                is_paused INTEGER NOT NULL,
                rank INTEGER NOT NULL,
                refreshed_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pool_rank ON pool_snapshots(rank);
            CREATE INDEX IF NOT EXISTS idx_pool_venue ON pool_snapshots(venue);
            CREATE TABLE IF NOT EXISTS position_snapshots (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                created_at INTEGER NOT NULL,
                venue TEXT NOT NULL,
                object_id TEXT NOT NULL,
                pool_id TEXT NOT NULL,
                pair_label TEXT NOT NULL,
                liquidity TEXT NOT NULL,
                amount_a TEXT NOT NULL,
                amount_b TEXT NOT NULL,
                symbol_a TEXT NOT NULL,
                symbol_b TEXT NOT NULL,
                decimals_a INTEGER NOT NULL,
                decimals_b INTEGER NOT NULL,
                current_tick INTEGER NOT NULL,
                tick_lower INTEGER NOT NULL,
                tick_upper INTEGER NOT NULL,
                in_range INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_pos_snap_obj
                ON position_snapshots(object_id, created_at DESC);
            "#,
        )?;
        Ok(())
    }

    fn now() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0)
    }

    fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()
            .context("get setting")
    }

    fn set_setting(&self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = excluded.value",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn load_settings(&self) -> Result<AppSettings> {
        // Migrate legacy `address` key once.
        let wallet = match self.get_setting(KEY_WALLET)? {
            Some(w) => Some(w),
            None => self.get_setting("address")?,
        };
        Ok(AppSettings {
            graphql_url: self.get_setting(KEY_GRAPHQL)?,
            wallet_address: wallet,
            pool_limit: self
                .get_setting(KEY_POOL_LIMIT)?
                .and_then(|s| s.parse().ok()),
            privacy_trim: self
                .get_setting(KEY_PRIVACY)?
                .map(|s| s == "1" || s.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
            price_mode: self
                .get_setting(KEY_PRICE_MODE)?
                .unwrap_or_else(|| "aftermath".into()),
        })
    }

    pub fn save_settings(&self, s: &AppSettings) -> Result<()> {
        if let Some(ref url) = s.graphql_url {
            self.set_setting(KEY_GRAPHQL, url)?;
        }
        if let Some(ref addr) = s.wallet_address {
            self.set_setting(KEY_WALLET, addr)?;
        }
        if let Some(limit) = s.pool_limit {
            self.set_setting(KEY_POOL_LIMIT, &limit.to_string())?;
        }
        self.set_setting(KEY_PRIVACY, if s.privacy_trim { "1" } else { "0" })?;
        let mode = if s.price_mode.is_empty() {
            "aftermath"
        } else {
            s.price_mode.as_str()
        };
        self.set_setting(KEY_PRICE_MODE, mode)?;
        Ok(())
    }

    pub fn upsert_token(&self, t: &CachedToken) -> Result<()> {
        self.conn.execute(
            "INSERT INTO tokens(coin_type, symbol, decimals, updated_at)
             VALUES(?1, ?2, ?3, ?4)
             ON CONFLICT(coin_type) DO UPDATE SET
               symbol = excluded.symbol,
               decimals = excluded.decimals,
               updated_at = excluded.updated_at",
            params![t.coin_type, t.symbol, t.decimals, Self::now()],
        )?;
        Ok(())
    }

    pub fn get_token(&self, coin_type: &str) -> Result<Option<CachedToken>> {
        self.conn
            .query_row(
                "SELECT coin_type, symbol, decimals FROM tokens WHERE coin_type = ?1",
                params![coin_type],
                |r| {
                    Ok(CachedToken {
                        coin_type: r.get(0)?,
                        symbol: r.get(1)?,
                        decimals: r.get::<_, i64>(2)? as u8,
                    })
                },
            )
            .optional()
            .context("get token")
    }

    /// Replace all pool snapshots with a fresh ranked set.
    pub fn replace_pool_snapshots(&self, rows: &[PoolRow]) -> Result<()> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute("DELETE FROM pool_snapshots", [])?;
        let now = Self::now();
        {
            let mut stmt = tx.prepare(
                "INSERT INTO pool_snapshots(
                    object_id, venue, name, coin_type_a, coin_type_b,
                    symbol_a, symbol_b, decimals_a, decimals_b,
                    fee_rate, tick_spacing, liquidity, coin_a, coin_b,
                    current_tick, sqrt_price, is_paused, rank, refreshed_at
                 ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            )?;
            for row in rows {
                stmt.execute(params![
                    row.object_id,
                    row.venue.short(),
                    row.name,
                    row.coin_type_a,
                    row.coin_type_b,
                    row.symbol_a,
                    row.symbol_b,
                    row.decimals_a,
                    row.decimals_b,
                    row.fee_rate,
                    row.tick_spacing,
                    row.liquidity.to_string(),
                    row.coin_a.to_string(),
                    row.coin_b.to_string(),
                    row.current_tick,
                    row.sqrt_price.to_string(),
                    if row.is_paused { 1 } else { 0 },
                    row.rank,
                    now,
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    pub fn list_pool_snapshots(&self, venue: Option<Venue>) -> Result<Vec<PoolRow>> {
        let mut sql = String::from(
            "SELECT object_id, venue, name, coin_type_a, coin_type_b,
                    symbol_a, symbol_b, decimals_a, decimals_b,
                    fee_rate, tick_spacing, liquidity, coin_a, coin_b,
                    current_tick, sqrt_price, is_paused, rank
             FROM pool_snapshots",
        );
        if venue.is_some() {
            sql.push_str(" WHERE venue = ?1");
        }
        sql.push_str(" ORDER BY rank ASC");

        let mut stmt = self.conn.prepare(&sql)?;
        let map_row = |r: &rusqlite::Row<'_>| -> rusqlite::Result<PoolRow> {
            let venue_s: String = r.get(1)?;
            Ok(PoolRow {
                object_id: r.get(0)?,
                venue: Venue::parse(&venue_s).unwrap_or_default(),
                name: r.get(2)?,
                coin_type_a: r.get(3)?,
                coin_type_b: r.get(4)?,
                symbol_a: r.get(5)?,
                symbol_b: r.get(6)?,
                decimals_a: r.get::<_, i64>(7)? as u8,
                decimals_b: r.get::<_, i64>(8)? as u8,
                fee_rate: r.get::<_, i64>(9)? as u32,
                tick_spacing: r.get::<_, i64>(10)? as u32,
                liquidity: r
                    .get::<_, String>(11)?
                    .parse()
                    .unwrap_or(0),
                coin_a: r.get::<_, String>(12)?.parse().unwrap_or(0),
                coin_b: r.get::<_, String>(13)?.parse().unwrap_or(0),
                current_tick: r.get(14)?,
                sqrt_price: r.get::<_, String>(15)?.parse().unwrap_or(0),
                is_paused: r.get::<_, i64>(16)? != 0,
                rank: r.get::<_, i64>(17)? as u32,
            })
        };

        let rows = if let Some(v) = venue {
            let mapped = stmt.query_map(params![v.short()], map_row)?;
            mapped.collect::<Result<Vec<_>, _>>()?
        } else {
            let mapped = stmt.query_map([], map_row)?;
            mapped.collect::<Result<Vec<_>, _>>()?
        };
        Ok(rows)
    }

    pub fn last_prefetch_at(&self) -> Result<Option<i64>> {
        self.conn
            .query_row(
                "SELECT MAX(refreshed_at) FROM pool_snapshots",
                [],
                |r| r.get::<_, Option<i64>>(0),
            )
            .context("last prefetch")
    }

    /// Record position size after a detail refresh (history).
    pub fn record_position_snapshot(&self, d: &PositionDetail) -> Result<()> {
        self.conn.execute(
            "INSERT INTO position_snapshots(
                created_at, venue, object_id, pool_id, pair_label,
                liquidity, amount_a, amount_b, symbol_a, symbol_b,
                decimals_a, decimals_b, current_tick, tick_lower, tick_upper, in_range
             ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
            params![
                Self::now(),
                d.row.venue.short(),
                d.row.object_id,
                d.row.pool_id,
                d.row.pair_label(),
                d.row.liquidity.to_string(),
                d.amount_a.to_string(),
                d.amount_b.to_string(),
                d.row.symbol_a,
                d.row.symbol_b,
                d.row.decimals_a,
                d.row.decimals_b,
                d.current_tick,
                d.row.tick_lower,
                d.row.tick_upper,
                if d.row.in_range == Some(true) { 1 } else { 0 },
            ],
        )?;
        Ok(())
    }

    pub fn list_position_snapshots(
        &self,
        object_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<PositionSnapshotRow>> {
        let limit = limit.clamp(1, 500) as i64;
        let map = |r: &rusqlite::Row<'_>| -> rusqlite::Result<PositionSnapshotRow> {
            Ok(PositionSnapshotRow {
                id: r.get::<_, i64>(0)? as u64,
                created_at: r.get(1)?,
                venue: r.get(2)?,
                object_id: r.get(3)?,
                pool_id: r.get(4)?,
                pair_label: r.get(5)?,
                liquidity: r.get::<_, String>(6)?.parse().unwrap_or(0),
                amount_a: r.get::<_, String>(7)?.parse().unwrap_or(0),
                amount_b: r.get::<_, String>(8)?.parse().unwrap_or(0),
                symbol_a: r.get(9)?,
                symbol_b: r.get(10)?,
                decimals_a: r.get::<_, i64>(11)? as u8,
                decimals_b: r.get::<_, i64>(12)? as u8,
                current_tick: r.get(13)?,
                tick_lower: r.get(14)?,
                tick_upper: r.get(15)?,
                in_range: r.get::<_, i64>(16)? != 0,
            })
        };
        if let Some(oid) = object_id {
            let mut stmt = self.conn.prepare(
                "SELECT id, created_at, venue, object_id, pool_id, pair_label,
                        liquidity, amount_a, amount_b, symbol_a, symbol_b,
                        decimals_a, decimals_b, current_tick, tick_lower, tick_upper, in_range
                 FROM position_snapshots
                 WHERE object_id = ?1
                 ORDER BY created_at DESC
                 LIMIT ?2",
            )?;
            let rows = stmt.query_map(params![oid, limit], map)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT id, created_at, venue, object_id, pool_id, pair_label,
                        liquidity, amount_a, amount_b, symbol_a, symbol_b,
                        decimals_a, decimals_b, current_tick, tick_lower, tick_upper, in_range
                 FROM position_snapshots
                 ORDER BY created_at DESC
                 LIMIT ?1",
            )?;
            let rows = stmt.query_map(params![limit], map)?;
            Ok(rows.collect::<Result<Vec<_>, _>>()?)
        }
    }
}

pub fn default_config_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    home.join(".config").join(APP_DIR_NAME)
}

pub fn default_db_path() -> PathBuf {
    default_config_dir().join(DEFAULT_DB_FILE)
}

pub fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."));
        home.join(rest)
    } else if path == "~" {
        std::env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
    } else {
        PathBuf::from(path)
    }
}

pub fn display_path(path: &Path) -> String {
    if let Ok(home) = std::env::var("HOME") {
        let home_path = PathBuf::from(&home);
        if let Ok(stripped) = path.strip_prefix(&home_path) {
            return format!("~/{}", stripped.display());
        }
    }
    path.display().to_string()
}

pub fn resolve_existing_db(cli_db: Option<&str>) -> Option<PathBuf> {
    if let Some(p) = cli_db {
        let path = expand_tilde(p);
        if path.exists() {
            return Some(path);
        }
    }
    let pointer = default_config_dir().join(ACTIVE_POINTER_FILE);
    if let Ok(contents) = fs::read_to_string(&pointer) {
        let path = expand_tilde(contents.trim());
        if path.exists() {
            return Some(path);
        }
    }
    let def = default_db_path();
    if def.exists() {
        Some(def)
    } else {
        None
    }
}

pub fn write_active_pointer(path: &Path) -> Result<()> {
    let dir = default_config_dir();
    fs::create_dir_all(&dir)?;
    fs::write(dir.join(ACTIVE_POINTER_FILE), path.to_string_lossy().as_bytes())?;
    Ok(())
}
