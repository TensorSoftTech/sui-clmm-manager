use sui_clmm_client::Client;
use sui_clmm_manager::{GraphQlMeter, Store, VenueFilter};

fn main() -> anyhow::Result<()> {
    let meter = GraphQlMeter::default();
    let gql = Client::connect("https://graphql.mainnet.sui.io/graphql", &meter)?;
    let path = std::env::temp_dir().join("sui-clmm-smoke.db");
    let _ = std::fs::remove_file(&path);
    let store = Store::open(&path)?;
    let client = Client::new(10);
    let stats = client.prefetch_pools(&gql, &store, VenueFilter::Only(sui_clmm_manager::Venue::Bluefin))?;
    println!(
        "stored={} gql_calls={} venues={:?}",
        stats.stored,
        stats.graphql_calls,
        stats.venues.iter().map(|(v, n)| (v.short(), *n)).collect::<Vec<_>>()
    );
    let rows = client.list_pools(&store, VenueFilter::All, None)?;
    for r in rows.iter().take(5) {
        println!(
            "  #{} {} {} liq={}",
            r.rank,
            r.venue.short(),
            r.pair_label(),
            r.liquidity_label()
        );
    }
    let router = client.swap_router(&store, VenueFilter::All)?;
    println!("{}", router.status());
    Ok(())
}
