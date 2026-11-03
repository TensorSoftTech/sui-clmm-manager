//! Smoke: list positions for a known Momentum LP holder.
use sui_clmm_client::Client;
use sui_clmm_manager::{GraphQlMeter, Store, VenueFilter};

fn main() -> anyhow::Result<()> {
    let owner = std::env::args()
        .nth(1)
        .unwrap_or_else(|| {
            "0x84bad7c441b9b1863177ade7c7d323b16e99c1cd4a2de0fa25e756f932bc0cfd".into()
        });
    let meter = GraphQlMeter::default();
    let gql = Client::connect("https://graphql.mainnet.sui.io/graphql", &meter)?;
    let path = std::env::temp_dir().join("sui-clmm-pos-smoke.db");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path)?;
    let client = Client::default();
    let rows = client.list_positions(&gql, Some(&store), &owner, VenueFilter::All)?;
    println!(
        "owner {} · {} positions · gql {}",
        &owner[..18.min(owner.len())],
        rows.len(),
        meter.graphql_count()
    );
    for r in rows.iter().take(8) {
        println!(
            "  {} {} {} L={} {}",
            r.venue.short(),
            r.pair_label(),
            r.range_label(),
            r.liquidity_label(),
            &r.object_id[..18.min(r.object_id.len())]
        );
    }
    if let Some(row) = rows.first() {
        let d = client.position_detail(&gql, Some(&store), row)?;
        println!(
            "detail: in position {} + {} · tick={} in_range={:?} · {}",
            d.amount_a_ui, d.amount_b_ui, d.current_tick, d.row.in_range, d.note
        );
        let hist = client.position_history(&store, Some(&row.object_id), 5)?;
        println!("history snapshots: {}", hist.len());
    }
    Ok(())
}
