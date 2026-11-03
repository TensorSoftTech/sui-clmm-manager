use crate::app::{App, DbPathChoice, HistoryFilter, Screen, SetupField};
use crate::theme;
use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table};
use sui_clmm_manager::{
    LiquiditySample, TokenSide, chart_view_window, format_amount, format_usd, format_usd_axis,
    short_id, slice_liquidity_series,
};

const FOOTER_H: u16 = 6;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    match app.screen {
        Screen::Detail => draw_detail_screen(frame, app, area),
        _ => {
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1),
                    Constraint::Min(5),
                    Constraint::Length(FOOTER_H),
                ])
                .split(area);
            draw_header(frame, chunks[0], app);
            match app.screen {
                Screen::DbSetup => draw_db_setup(frame, chunks[1], app),
                Screen::Setup => draw_setup(frame, chunks[1], app),
                Screen::Positions => draw_positions(frame, chunks[1], app),
                Screen::History => draw_history(frame, chunks[1], app),
                Screen::Settings | Screen::SettingsGraphql | Screen::SettingsWallet => {
                    draw_settings(frame, chunks[1], app)
                }
                Screen::Help => draw_help(frame, chunks[1]),
                Screen::Detail => unreachable!(),
            }
            draw_footer(frame, app, chunks[2]);
        }
    }
}

fn panel(title: &str, warn: bool) -> Block<'static> {
    Block::default()
        .borders(Borders::ALL)
        .border_style(if warn { theme::warn() } else { theme::border() })
        .title(Span::styled(format!(" {title} "), theme::title()))
        .style(theme::body())
}

/// Slow left→right fill inside the panel that is refreshing (wraps and repeats).
fn draw_panel_load(frame: &mut Frame, area: Rect, ratio: f64, title: &str, label: &str) {
    let inner_w = area.width.saturating_sub(2) as usize;
    let filled = if inner_w == 0 {
        0
    } else {
        ((ratio.clamp(0.0, 1.0) * inner_w as f64).round() as usize).min(inner_w)
    };
    let empty = inner_w.saturating_sub(filled);
    let bar = format!("{}{}", "█".repeat(filled), "░".repeat(empty));
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(bar, theme::accent_text())),
            Line::from(""),
            Line::from(Span::styled(label, theme::warn())),
        ])
        .block(panel(title, true)),
        area,
    );
}

fn draw_header(frame: &mut Frame, area: Rect, app: &App) {
    let title = match app.screen {
        Screen::DbSetup => "sui-clmm · database",
        Screen::Setup => "sui-clmm · setup",
        Screen::Positions => "sui-clmm · positions",
        Screen::Detail => "sui-clmm · detail",
        Screen::History => "sui-clmm · history",
        Screen::Settings | Screen::SettingsGraphql | Screen::SettingsWallet => {
            "sui-clmm · settings"
        }
        Screen::Help => "sui-clmm · help",
    };
    let line = Line::from(vec![
        Span::styled(title, theme::title()),
        Span::raw("  "),
        Span::styled(format!("wallet {}", app.wallet_label()), theme::muted()),
        Span::raw("  "),
        Span::styled(&app.status, theme::dim()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

fn draw_footer(frame: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Length(3)])
        .split(area);

    let busy = if app.busy { " · busy" } else { "" };
    let err = app
        .error
        .as_ref()
        .map(|e| format!(" · err: {e}"))
        .unwrap_or_default();
    let filter = if app.filter_mode {
        format!(" /{}█", app.symbol_query)
    } else if !app.symbol_query.is_empty() {
        format!(" filter:{}", app.symbol_query)
    } else {
        String::new()
    };
    let keys = format!(
        "r refresh · v venue · / filter · y history · enter detail · s settings · ? help · q quit{filter}{busy}{err}"
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(keys, theme::muted()))).block(panel("keys", false)),
        chunks[0],
    );

    let max_chars = area.width.saturating_sub(4) as usize;
    let rpc_line = app.meter.footer_line(max_chars.max(16));
    let failed = app.meter.history().iter().any(|c| !c.ok);
    let title = format!(
        "{} · db {}",
        app.meter.footer_title().trim(),
        app.db_size_label()
    );
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            rpc_line,
            if failed { theme::warn() } else { theme::dim() },
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(theme::border())
                .title(Span::styled(format!(" {title} "), theme::title()))
                .style(theme::body()),
        ),
        chunks[1],
    );
}

fn draw_db_setup(frame: &mut Frame, area: Rect, app: &App) {
    let def = App::default_db_display();
    let mut lines = vec![
        Line::from(Span::styled(
            "SQLite for settings + token cache (pool prefetch kept for later)",
            theme::body(),
        )),
        Line::from(""),
    ];
    for (i, choice) in DbPathChoice::ALL.iter().enumerate() {
        let prefix = if i == app.db_choice_idx { "› " } else { "  " };
        let style = if i == app.db_choice_idx {
            theme::highlight()
        } else {
            theme::body()
        };
        lines.push(Line::from(Span::styled(
            format!("{prefix}{}", choice.label(&def)),
            style,
        )));
    }
    if app.editing {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("path: {}_", app.input),
            theme::warn(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("database", false)), area);
}

fn draw_setup(frame: &mut Frame, area: Rect, app: &App) {
    if app.busy {
        draw_panel_load(
            frame,
            area,
            app.load_bar_ratio(),
            "setup",
            if app.load_label.is_empty() {
                "loading positions…"
            } else {
                app.load_label.as_str()
            },
        );
        return;
    }
    let mut lines = vec![
        Line::from(Span::styled(
            "Read-only: paste a Sui address to list LP positions",
            theme::body(),
        )),
        Line::from(""),
    ];
    for field in SetupField::ALL {
        let selected = app.setup_field == field;
        let prefix = if selected { "› " } else { "  " };
        let value = match field {
            SetupField::Wallet => app
                .settings
                .wallet_address
                .clone()
                .unwrap_or_else(|| "(required)".into()),
            SetupField::Graphql => app.graphql_url(),
            SetupField::Done => String::new(),
        };
        let style = if selected {
            theme::highlight()
        } else {
            theme::body()
        };
        if field == SetupField::Done {
            lines.push(Line::from(Span::styled(
                format!("{prefix}{}", field.label()),
                style,
            )));
        } else {
            lines.push(Line::from(Span::styled(
                format!("{prefix}{} · {value}", field.label()),
                style,
            )));
        }
    }
    if app.editing {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("edit: {}_", app.input),
            theme::warn(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("setup", false)), area);
}

fn draw_positions(frame: &mut Frame, area: Rect, app: &mut App) {
    if app.busy {
        draw_panel_load(
            frame,
            area,
            app.load_bar_ratio(),
            "positions",
            if app.load_label.is_empty() {
                "refreshing list…"
            } else {
                app.load_label.as_str()
            },
        );
        return;
    }

    let header = Row::new(vec![
        "venue", "pair", "range", "liq", "fee", "rng", "id",
    ])
    .style(theme::muted());

    let rows_data: Vec<_> = app
        .filtered_positions()
        .into_iter()
        .map(|p| {
            Row::new(vec![
                Cell::from(p.venue.short()),
                Cell::from(p.pair_label()),
                Cell::from(p.range_label()),
                Cell::from(p.liquidity_label()),
                Cell::from(p.fee_bps_label()),
                Cell::from(p.status_label()),
                Cell::from(short_id(&p.object_id)),
            ])
            .style(theme::body())
        })
        .collect();

    let table = Table::new(
        rows_data,
        [
            Constraint::Length(9),
            Constraint::Min(12),
            Constraint::Length(16),
            Constraint::Length(8),
            Constraint::Length(9),
            Constraint::Length(4),
            Constraint::Length(14),
        ],
    )
    .header(header)
    .row_highlight_style(theme::highlight())
    .block(panel(
        &format!(
            "positions · {} · {}",
            app.filtered_positions().len(),
            app.venue_filter.label()
        ),
        false,
    ));

    frame.render_stateful_widget(table, area, &mut app.table);
}

fn draw_detail_screen(frame: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(12),
            Constraint::Min(8),
            Constraint::Length(FOOTER_H),
        ])
        .split(area);

    draw_header(frame, chunks[0], app);

    if app.busy && app.detail.is_none() {
        draw_panel_load(
            frame,
            chunks[1],
            app.load_bar_ratio(),
            "position",
            if app.load_label.is_empty() {
                "loading position · pool · ticks…"
            } else {
                app.load_label.as_str()
            },
        );
        frame.render_widget(
            Paragraph::new(Span::styled("waiting for liquidity series…", theme::dim()))
                .block(panel("liquidity distribution", false)),
            chunks[2],
        );
        draw_footer(frame, app, chunks[3]);
        return;
    }

    // Refreshing an open detail: keep chart, swap stats panel for the bar.
    if app.busy && app.detail.is_some() {
        draw_panel_load(
            frame,
            chunks[1],
            app.load_bar_ratio(),
            "position",
            if app.load_label.is_empty() {
                "refreshing this position…"
            } else {
                app.load_label.as_str()
            },
        );
        // Still draw chart from last known detail below.
        if let Some(d) = app.detail.as_ref() {
            let (lo, hi) = chart_view_window(
                d.current_tick,
                d.tick_spacing,
                app.detail_chart_half_spacings,
                (d.series_min_tick, d.series_max_tick),
                d.row.tick_lower,
                d.row.tick_upper,
            );
            let (view, view_max) = slice_liquidity_series(&d.series, lo, hi);
            draw_liquidity_chart(
                frame,
                chunks[2],
                &view,
                view_max,
                d.pool_liquidity,
                d.usd.as_ref().and_then(|u| u.pool_tvl_usd),
                d.current_tick,
                &d.row.symbol_a,
                &d.row.symbol_b,
                Some(d.row.tick_lower),
                Some(d.row.tick_upper),
                app.detail_chart_half_spacings,
            );
        }
        draw_footer(frame, app, chunks[3]);
        return;
    }

    let Some(d) = app.detail.as_ref() else {
        frame.render_widget(
            Paragraph::new("no detail").block(panel("position", false)),
            chunks[1],
        );
        draw_footer(frame, app, chunks[3]);
        return;
    };

    let r = &d.row;
    let pos_id = if app.show_full_id {
        r.object_id.clone()
    } else {
        short_id(&r.object_id)
    };
    let pool_id = if app.show_full_id {
        r.pool_id.clone()
    } else {
        short_id(&r.pool_id)
    };
    let range = if r.in_range == Some(true) {
        Span::styled("IN RANGE", theme::ok())
    } else if r.in_range == Some(false) {
        Span::styled("OUT OF RANGE", theme::warn())
    } else {
        Span::styled("?", theme::dim())
    };

    let stats = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("venue ", theme::header_key()),
            Span::styled(r.venue.full(), theme::accent_text()),
            Span::styled("  · ", theme::dim()),
            Span::styled(r.venue.short(), theme::muted()),
        ]),
        Line::from(vec![
            Span::styled("position ", theme::header_key()),
            Span::styled(pos_id, theme::accent_text()),
            Span::raw("  "),
            range,
        ]),
        Line::from(vec![
            Span::styled("pool ", theme::header_key()),
            Span::styled(r.pair_label(), theme::body()),
            Span::styled(
                format!("  {}  spacing {}", r.fee_bps_label(), d.tick_spacing),
                theme::muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled("pool id ", theme::header_key()),
            Span::styled(pool_id, theme::dim()),
        ]),
        Line::from(vec![
            Span::styled("ticks ", theme::header_key()),
            Span::styled(
                format!(
                    "[{}, {}] · current {} · pool ticks {}",
                    r.tick_lower,
                    r.tick_upper,
                    d.current_tick,
                    d.ticks.len()
                ),
                theme::body(),
            ),
        ]),
        Line::from(vec![
            Span::styled("in position ", theme::header_key()),
            Span::styled(
                format!("{}  +  {}", d.amount_a_ui, d.amount_b_ui),
                theme::ok(),
            ),
            Span::styled(
                d.usd
                    .as_ref()
                    .and_then(|u| u.position_usd)
                    .map(|v| format!("  · {}", format_usd(v)))
                    .unwrap_or_default(),
                theme::ok(),
            ),
        ]),
        Line::from(vec![
            Span::styled("liq ", theme::header_key()),
            Span::styled(
                format!(
                    "pos {} · pool {}",
                    r.liquidity_label(),
                    sui_clmm_manager::format_compact_u128(d.pool_liquidity)
                ),
                theme::body(),
            ),
        ]),
        Line::from(vec![
            Span::styled("pool vault ", theme::header_key()),
            Span::styled(
                format!(
                    "{} {} · {} {}",
                    r.symbol_a,
                    format_amount(d.coin_a_reserve, r.decimals_a),
                    r.symbol_b,
                    format_amount(d.coin_b_reserve, r.decimals_b)
                ),
                theme::body(),
            ),
            Span::styled(
                d.usd
                    .as_ref()
                    .and_then(|u| u.pool_tvl_usd)
                    .map(|v| format!("  · TVL {}", format_usd(v)))
                    .unwrap_or_default(),
                theme::muted(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                if d.fees_claim_ok {
                    "fees (claim) "
                } else {
                    "fees (stored) "
                },
                theme::header_key(),
            ),
            Span::styled(
                format!("{}  +  {}", d.fee_a_ui, d.fee_b_ui),
                theme::warn(),
            ),
            Span::styled(
                d.usd
                    .as_ref()
                    .and_then(|u| u.fees_usd)
                    .map(|v| format!("  · {}", format_usd(v)))
                    .unwrap_or_default(),
                theme::warn(),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                if d.rewards.iter().any(|r| r.claim_ok) {
                    "rewards (claim) "
                } else {
                    "rewards "
                },
                theme::header_key(),
            ),
            Span::styled(
                if d.rewards.is_empty() {
                    "—".into()
                } else {
                    d.rewards
                        .iter()
                        .map(|r| {
                            if let Some(u) = r.usd {
                                format!("{} ({})", r.amount_ui, format_usd(u))
                            } else {
                                r.amount_ui.clone()
                            }
                        })
                        .collect::<Vec<_>>()
                        .join("  +  ")
                },
                theme::accent_text(),
            ),
            Span::styled(
                d.rewards_usd
                    .map(|v| format!("  · Σ {}", format_usd(v)))
                    .unwrap_or_default(),
                theme::accent_text(),
            ),
        ]),
        Line::from(vec![
            Span::styled("usd ", theme::header_key()),
            Span::styled(
                d.usd
                    .as_ref()
                    .map(|u| {
                        format!(
                            "{} @ {} / {} · {}",
                            u.mode.label(),
                            u.price_a
                                .price
                                .map(format_usd)
                                .unwrap_or_else(|| "—".into()),
                            u.price_b
                                .price
                                .map(format_usd)
                                .unwrap_or_else(|| "—".into()),
                            u.note
                        )
                    })
                    .unwrap_or_else(|| "off".into()),
                theme::dim(),
            ),
        ]),
        Line::from(Span::styled(
            format!("{} · +/- zoom · 0 fit · y history · p id · r refresh · esc", d.note),
            theme::dim(),
        )),
    ])
    .block(panel("position", false));
    frame.render_widget(stats, chunks[1]);

    let (lo, hi) = chart_view_window(
        d.current_tick,
        d.tick_spacing,
        app.detail_chart_half_spacings,
        (d.series_min_tick, d.series_max_tick),
        r.tick_lower,
        r.tick_upper,
    );
    let (view, view_max) = slice_liquidity_series(&d.series, lo, hi);
    draw_liquidity_chart(
        frame,
        chunks[2],
        &view,
        view_max,
        d.pool_liquidity,
        d.usd.as_ref().and_then(|u| u.pool_tvl_usd),
        d.current_tick,
        &r.symbol_a,
        &r.symbol_b,
        Some(r.tick_lower),
        Some(r.tick_upper),
        app.detail_chart_half_spacings,
    );

    draw_footer(frame, app, chunks[3]);
}

fn draw_liquidity_chart(
    frame: &mut Frame,
    area: Rect,
    series: &[LiquiditySample],
    max_l: u64,
    pool_liquidity: u128,
    pool_tvl_usd: Option<f64>,
    current_tick: i32,
    sym_a: &str,
    sym_b: &str,
    range_lo: Option<i32>,
    range_hi: Option<i32>,
    zoom_half_spacings: i32,
) {
    let y_usd = pool_tvl_usd.is_some_and(|v| v.is_finite() && v > 0.0);
    let title = if y_usd {
        format!(
            "liquidity · Y ≈ USD · X ticks · {sym_a}/{sym_b} · zoom ±{zoom_half_spacings} · +/-/0"
        )
    } else {
        format!(
            "liquidity · Y = relative depth · X ticks · {sym_a}/{sym_b} · zoom ±{zoom_half_spacings} · +/-/0"
        )
    };
    let block = panel(&title, false);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if series.is_empty() || inner.width < 8 || inner.height < 4 {
        frame.render_widget(
            Paragraph::new(Span::styled(
                "no liquidity series · open detail again after ticks load",
                theme::dim(),
            )),
            inner,
        );
        return;
    }

    let y_axis_w = if y_usd { 9u16 } else { 10u16 };
    let x_axis_h = 2u16;
    let chart_w = inner.width.saturating_sub(y_axis_w);
    let chart_h = inner.height.saturating_sub(x_axis_h);
    if chart_w == 0 || chart_h == 0 {
        return;
    }

    let chart = Rect {
        x: inner.x + y_axis_w,
        y: inner.y,
        width: chart_w,
        height: chart_h,
    };
    let y_lbl = Rect {
        x: inner.x,
        y: inner.y,
        width: y_axis_w,
        height: chart_h,
    };
    let x_lbl = Rect {
        x: chart.x,
        y: chart.y + chart_h,
        width: chart_w,
        height: x_axis_h,
    };

    frame.render_widget(Clear, chart);

    let n = series.len();
    let cols = chart_w as usize;
    let tick_lo = series.first().map(|s| s.tick).unwrap_or(0);
    let tick_hi = series.last().map(|s| s.tick).unwrap_or(0);
    let tick_span = (tick_hi - tick_lo).max(1);
    // USD mode: full height = pool active L ↔ vault TVL (Sol twin). Else in-range relative.
    let pool_l_scaled = sui_clmm_manager::scale_u128(pool_liquidity.max(1)).max(1) as f64;
    let range_max = series
        .iter()
        .filter(|s| s.in_position)
        .map(|s| s.liquidity)
        .max()
        .unwrap_or(max_l)
        .max(1);
    let bar_denom = if y_usd {
        pool_l_scaled
    } else {
        range_max.max(1) as f64
    };
    let max_l = bar_denom;

    // Tick-linear X: column ↔ tick; step = last sample with tick ≤ col (no right collapse).
    let tick_for_col = |col: usize| -> i32 {
        if cols <= 1 {
            tick_lo
        } else {
            tick_lo + ((col as i64 * tick_span as i64) / (cols as i64 - 1)) as i32
        }
    };
    let sample_at_tick = |tick: i32| -> &LiquiditySample {
        let mut best = &series[0];
        for s in series {
            if s.tick <= tick {
                best = s;
            } else {
                break;
            }
        }
        best
    };
    let col_for_tick = |target: i32| -> Option<usize> {
        if cols == 0 {
            return None;
        }
        let t = target.clamp(tick_lo, tick_hi);
        Some((((t - tick_lo) as i64 * (cols as i64 - 1)) / tick_span as i64) as usize)
    };

    let current_col = col_for_tick(current_tick);
    let lo_col = range_lo.and_then(col_for_tick);
    let hi_col = range_hi.and_then(col_for_tick);
    let _ = n;

    for col in 0..cols {
        let tick = tick_for_col(col);
        let sample = sample_at_tick(tick);
        let h = ((sample.liquidity as f64 / bar_denom) * chart_h as f64).round() as u16;
        let h = h.clamp(1, chart_h);
        let is_current = current_col == Some(col);
        for row in 0..h {
            let y = chart.y + chart_h - 1 - row;
            let x = chart.x + col as u16;
            let style = if is_current {
                theme::warn()
            } else {
                match sample.side {
                    TokenSide::A => theme::token_a(),
                    TokenSide::B => theme::token_b(),
                    TokenSide::Both => {
                        if row < h / 2 {
                            theme::token_b()
                        } else {
                            theme::token_a()
                        }
                    }
                    TokenSide::Outside => theme::outside(),
                }
            };
            let ch = if is_current {
                "┃"
            } else if sample.side == TokenSide::Both && row == h / 2 {
                "▄"
            } else {
                "█"
            };
            frame.render_widget(
                Paragraph::new(Span::styled(ch, style)),
                Rect {
                    x,
                    y,
                    width: 1,
                    height: 1,
                },
            );
        }
        if is_current {
            for row in h..chart_h {
                let y = chart.y + chart_h - 1 - row;
                frame.render_widget(
                    Paragraph::new(Span::styled("│", theme::warn())),
                    Rect {
                        x: chart.x + col as u16,
                        y,
                        width: 1,
                        height: 1,
                    },
                );
            }
        }
        let is_lo = lo_col == Some(col) && !is_current;
        let is_hi = hi_col == Some(col) && !is_current;
        if is_lo || is_hi {
            let ch = if is_lo { "▌" } else { "▐" };
            for row in 0..chart_h {
                let y = chart.y + chart_h - 1 - row;
                frame.render_widget(
                    Paragraph::new(Span::styled(ch, theme::accent_text())),
                    Rect {
                        x: chart.x + col as u16,
                        y,
                        width: 1,
                        height: 1,
                    },
                );
            }
        }
    }

    // Y labels
    let y_w = y_axis_w as usize;
    let pad = |s: String| {
        if s.chars().count() >= y_w {
            s.chars().take(y_w).collect()
        } else {
            format!("{s:>width$}", width = y_w)
        }
    };
    let (top, mid, bot) = if let Some(tvl) = pool_tvl_usd.filter(|v| v.is_finite() && *v > 0.0) {
        (
            format_usd_axis(tvl),
            format_usd_axis(tvl / 2.0),
            "$0".into(),
        )
    } else {
        (
            format!("{max_l:.0}"),
            format!("{:.0}", max_l / 2.0),
            "0".into(),
        )
    };
    let y_lines = vec![
        Line::from(Span::styled(pad(top), theme::dim())),
        Line::from(""),
        Line::from(Span::styled(pad(mid), theme::dim())),
        Line::from(""),
        Line::from(Span::styled(pad(bot), theme::dim())),
    ];
    frame.render_widget(Paragraph::new(y_lines), y_lbl);

    let x_text = format!("{tick_lo}          spot {current_tick}          {tick_hi}");
    frame.render_widget(
        Paragraph::new(Span::styled(x_text, theme::dim())),
        x_lbl,
    );
}

fn draw_history(frame: &mut Frame, area: Rect, app: &mut App) {
    let scoped = matches!(app.history_filter, HistoryFilter::Position { .. });
    let header = Row::new([
        Cell::from("when"),
        Cell::from("venue"),
        Cell::from(if scoped { "·" } else { "pair" }),
        Cell::from("size"),
        Cell::from("Δ vs prev"),
        Cell::from("tick"),
    ])
    .style(theme::muted());

    let rows: Vec<Row> = if app.history.is_empty() {
        vec![Row::new([
            Cell::from(Span::styled("—", theme::dim())),
            Cell::from(Span::styled(
                "no snapshots yet · open a position detail (enter) to record size",
                theme::muted(),
            )),
            Cell::from("—"),
            Cell::from("—"),
            Cell::from("—"),
            Cell::from("—"),
        ])]
    } else {
        app.history
            .iter()
            .enumerate()
            .map(|(i, r)| {
                // history is newest-first; Δ vs next older row
                let delta = app
                    .history
                    .get(i + 1)
                    .map(|prev| r.delta_vs(prev))
                    .unwrap_or_else(|| "—".into());
                let pair = if scoped {
                    "·".into()
                } else {
                    r.pair_label.clone()
                };
                let tick = if r.in_range {
                    format!("{} in", r.current_tick)
                } else {
                    format!("{} out", r.current_tick)
                };
                Row::new([
                    Cell::from(r.time_label()),
                    Cell::from(Span::styled(r.venue.clone(), theme::accent_text())),
                    Cell::from(pair),
                    Cell::from(Span::styled(r.size_label(), theme::ok())),
                    Cell::from(Span::styled(delta, theme::muted())),
                    Cell::from(tick),
                ])
                .style(theme::body())
            })
            .collect()
    };

    let title = match &app.history_filter {
        HistoryFilter::All => format!(
            "history · all · {} · size snapshots · r reload · Esc",
            app.history.len()
        ),
        HistoryFilter::Position { label, .. } => format!(
            "history · {label} · {} · r reload · Esc",
            app.history.len()
        ),
    };

    let table = Table::new(
        rows,
        [
            Constraint::Length(16),
            Constraint::Length(9),
            Constraint::Length(if scoped { 1 } else { 12 }),
            Constraint::Min(20),
            Constraint::Length(22),
            Constraint::Length(12),
        ],
    )
    .header(header)
    .block(panel(&title, false))
    .row_highlight_style(theme::highlight());

    frame.render_stateful_widget(table, area, &mut app.history_table);
}

fn draw_settings(frame: &mut Frame, area: Rect, app: &App) {
    let mut lines = vec![
        Line::from(Span::styled("Settings", theme::title())),
        Line::from(""),
        Line::from(format!("Wallet: {}", app.wallet_label())),
        Line::from(format!("GraphQL: {}", app.graphql_url())),
        Line::from(format!(
            "Privacy trim: {}",
            if app.settings.privacy_trim { "on" } else { "off" }
        )),
        Line::from(format!(
            "USD prices: {} (Aftermath REST)",
            if app.settings.price_mode.is_empty() {
                "aftermath"
            } else {
                app.settings.price_mode.as_str()
            }
        )),
        Line::from(""),
        Line::from(Span::styled(
            "w edit wallet · g graphql · t privacy · u usd mode · r reload · esc back",
            theme::muted(),
        )),
    ];
    if app.screen == Screen::SettingsGraphql {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("edit url: {}_", app.input),
            theme::warn(),
        )));
    }
    if app.screen == Screen::SettingsWallet {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            format!("edit wallet: {}_", app.input),
            theme::warn(),
        )));
    }
    frame.render_widget(Paragraph::new(lines).block(panel("settings", false)), area);
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let lines = vec![
        Line::from(Span::styled("Keys", theme::title())),
        Line::from("  j/k · ↑/↓     move list / history"),
        Line::from("  enter         open position detail"),
        Line::from("  esc           back"),
        Line::from("  r             refresh list or detail"),
        Line::from("  y             size history (all · or this position from detail)"),
        Line::from("  v             cycle venue filter (all / bluefin / …)"),
        Line::from("  /             symbol filter"),
        Line::from("  +/-/0         chart zoom in / out / fit"),
        Line::from("  p             toggle full object ids"),
        Line::from("  s             settings"),
        Line::from("  ?             this help · q quit"),
        Line::from(""),
        Line::from(Span::styled("Settings", theme::title())),
        Line::from("  w             edit wallet address"),
        Line::from("  g             edit GraphQL URL"),
        Line::from("  t             privacy trim (short ids)"),
        Line::from("  u             USD prices on/off (Aftermath)"),
        Line::from(""),
        Line::from(Span::styled("Position detail", theme::title())),
        Line::from("  in position   amounts from L + √P at pool tick"),
        Line::from("  fees          stored owed, or claim-sim when bound ticks load"),
        Line::from("  rewards       Bluefin emissions (+ time accrual · claim-sim)"),
        Line::from("  usd           Aftermath POST /api/prices → size / fees / rewards / TVL"),
        Line::from("  chart         tick-linear depth · Y ≈ USD when TVL priced"),
        Line::from("                ▌▐ range · ┃ spot · A left of spot · B right"),
        Line::from(""),
        Line::from(Span::styled(
            "Footer: GraphQL call counter + hist · SQLite db size",
            theme::muted(),
        )),
        Line::from(Span::styled(
            "Venues: Bluefin (fees+rewards) · Momentum/Cetus/Turbos (size; ticks later)",
            theme::muted(),
        )),
    ];
    let block = panel("help", false);
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme::FG)),
        inner,
    );
}
