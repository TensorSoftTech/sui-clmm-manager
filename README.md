# sui-clmm-tui

> **Unofficial.** Not affiliated with Bluefin, Momentum, Cetus, Turbos, DeepBook, Aftermath, or Mysten.
>
> **WIP — not production software.** Experimental. Do not rely on it with funds you cannot afford to lose.
> Claim-sim / USD figures can differ slightly from venue UIs (clock skew, indexer lag, price source).

Sui counterpart to [sol-clmm-manager](https://github.com/TensorSoftTech/sol-clmm-manager) — ratatui TUI + libs for concentrated liquidity on Sui.

Sol twin uses **Jupiter / Titan** for USD. This project uses **[Aftermath Finance](https://docs.aftermath.finance/for-developers/api/rest-api/prices) REST** (`POST https://aftermath.finance/api/prices`) — no API key. Toggle in settings (`u`).

## What works today

**Read path (TUI):** wallet address in SQLite → GraphQL owned LP positions → detail (size, fees/rewards where wired, chart, USD).

```
wallet (SQLite)
  → Sui GraphQL owned objects (per venue Position type)
  → list + detail
  → Aftermath /api/prices (USD)
  → Bluefin: tick table DF + fee/reward claim-sim
```

| Area | Status |
|------|--------|
| Wallet address + privacy trim | Works |
| Positions list (multi-venue) | Works — empty venue = you hold no NFTs there |
| Size `in position` (L + √P Q64.64) | Works |
| Liquidity chart (tick-linear · ▌▐ range · ┃ spot · +/-/0 zoom) | Works; **Y ≈ USD** when Aftermath TVL available |
| Size history (`y`) | Works — snapshot on each detail open/refresh |
| Footer GraphQL meter + SQLite size | Works |
| USD (Aftermath) | Works — position / fees / rewards / vault TVL |
| Help (`?`) | Works |

### Per venue

| Venue | List | Detail size/chart | Fees | Rewards | Notes |
|-------|------|-------------------|------|---------|--------|
| **Bluefin** | Yes | Yes + pool ticks | Claim-sim (bound ticks + growth) | Claim-sim + **time accrual** (`reward_per_seconds × dt / L`) | Most complete |
| **Momentum** | Yes | Yes (synthetic chart) | Stored `owed_coin_x/y` only | On-NFT `reward_infos` **not shown yet** | No tick table |
| **Cetus** | Yes | Yes (synthetic chart) | Usually **0 on NFT** (fees elsewhere) | Not wired | No tick table |
| **Turbos** | Yes (NFT → inner) | Yes (synthetic chart) | Stored owed when inner loads | Not wired | No tick table |
| **DeepBook** | — | — | — | — | Stub (CLOB / router later) |

Pool-top **prefetch** is implemented for Bluefin only (naïve `objects(filter: Pool)` — slow on Cetus/Turbos; GraphQL has **no** liquidity sort). Prefetch + **`router`** are **not** in the TUI yet.

## External APIs

| Service | Use | Auth |
|---------|-----|------|
| Sui GraphQL (default mainnet) | Objects, owned positions, ticks, coin metadata | None (public; rate-limited) |
| [Aftermath prices](https://docs.aftermath.finance/for-developers/api/rest-api/prices) | USD map for coin types | None — `POST /api/prices` `{ "coinTypes": [...] }` |

## Workspace

```
crates/
  sui-clmm-manager/   types, GraphQl meter, Store, chart, fees/rewards math, Aftermath quotes
  bluefin/            positions + ticks + fee/reward claim-sim + pool prefetch
  momentum/           positions (stored fees)
  cetus/              positions
  turbos/             position NFTs → inner
  deepbook/           stub
  router/             swap planner over prefetched pools (stub quotes)
  sui-clmm-client/    Client API
  sui-clmm-tui/       ratatui binary
```

## TODO / next

- [ ] Cetus / Momentum / Turbos: tick tables + fee claim-sim (parity with Bluefin)
- [ ] Momentum (and others): show reward streams + accrual
- [ ] Cetus fee model (often not on the position NFT)
- [ ] Faster pool universe than full GraphQL type scan (factory tables / venue HTTP / indexer) for router
- [ ] TUI pool browser (deferred)
- [ ] DeepBook GraphQL + router quotes / multi-hop
- [ ] gRPC / Move: simulate + execute (collect fees/rewards, liquidity writes)
- [ ] Optional second USD source / caching / offline mode

## Run

```bash
cargo run -p sui-clmm-tui -- --address 0xYOUR_SUI_ADDRESS
# optional:
cargo run -p sui-clmm-tui -- --graphql-url https://graphql.mainnet.sui.io/graphql
```

**Keys:** `j/k` move · `enter` detail · `r` refresh · `y` history · `v` venue · `/` filter · `+/-/0` chart zoom · `p` full id · `s` settings · `?` help · `q` quit.

**Settings:** `w` wallet · `g` GraphQL · `t` privacy trim · `u` Aftermath USD on/off.

Default DB: `~/.config/sui-clmm-manager/sui-clmm.db`
