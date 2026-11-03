//! Thin GraphQL client + session call meter.

use anyhow::{Context, Result, anyhow};
use serde_json::{Value, json};
use std::cell::RefCell;
use std::collections::VecDeque;
use std::time::Duration;

pub const DEFAULT_GRAPHQL_URL: &str = "https://graphql.mainnet.sui.io/graphql";
pub const GRAPHQL_HISTORY_CAP: usize = 64;

const PUBLIC_HOSTS: &[&str] = &[
    "graphql.mainnet.sui.io",
    "graphql.testnet.sui.io",
    "graphql.devnet.sui.io",
];

pub fn is_public_graphql(url: &str) -> bool {
    let host = url
        .trim()
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase();
    PUBLIC_HOSTS.iter().any(|h| host == *h)
}

#[derive(Debug, Clone)]
pub struct GraphQlCall {
    pub method: String,
    pub note: String,
    pub ok: bool,
}

impl GraphQlCall {
    pub fn label(&self) -> String {
        let mark = if self.ok { "" } else { "!" };
        if self.note.is_empty() {
            format!("{}{mark}", self.method)
        } else {
            format!(
                "{}({}{})",
                self.method,
                self.note,
                if self.ok { "" } else { "!" }
            )
        }
    }
}

#[derive(Debug, Default)]
pub struct GraphQlMeter {
    inner: RefCell<GraphQlMeterInner>,
}

#[derive(Debug, Default)]
struct GraphQlMeterInner {
    total: u64,
    graphql: u64,
    history: VecDeque<GraphQlCall>,
}

impl GraphQlMeter {
    pub fn record(&self, method: &str, note: impl Into<String>, ok: bool) {
        let mut g = self.inner.borrow_mut();
        g.total = g.total.saturating_add(1);
        g.graphql = g.graphql.saturating_add(1);
        if g.history.len() >= GRAPHQL_HISTORY_CAP {
            g.history.pop_front();
        }
        g.history.push_back(GraphQlCall {
            method: method.to_string(),
            note: note.into(),
            ok,
        });
    }

    pub fn total(&self) -> u64 {
        self.inner.borrow().total
    }

    pub fn graphql_count(&self) -> u64 {
        self.inner.borrow().graphql
    }

    pub fn history(&self) -> Vec<GraphQlCall> {
        self.inner.borrow().history.iter().cloned().collect()
    }

    pub fn clear(&self) {
        *self.inner.borrow_mut() = GraphQlMeterInner::default();
    }

    /// Merge another meter's counts + history (after a background fetch).
    pub fn merge(&self, other: &GraphQlMeter) {
        let o = other.inner.borrow();
        let mut g = self.inner.borrow_mut();
        g.total = g.total.saturating_add(o.total);
        g.graphql = g.graphql.saturating_add(o.graphql);
        for c in &o.history {
            if g.history.len() >= GRAPHQL_HISTORY_CAP {
                g.history.pop_front();
            }
            g.history.push_back(c.clone());
        }
    }

    pub fn footer_title(&self) -> String {
        let total = self.total();
        let gql = self.graphql_count();
        format!(" calls · {total} · gql×{gql} ")
    }

    pub fn footer_line(&self, max_chars: usize) -> String {
        let g = self.inner.borrow();
        let head = format!("hist {} · ", g.total);
        if g.history.is_empty() {
            return format!("{head}(none yet)");
        }
        let labels: Vec<String> = g.history.iter().map(|c| c.label()).collect();
        let joined = labels.join(" · ");
        let budget = max_chars.saturating_sub(head.len());
        if joined.len() <= budget {
            return format!("{head}{joined}");
        }
        let mut acc = String::new();
        for lab in labels.iter().rev() {
            let next = if acc.is_empty() {
                lab.clone()
            } else {
                format!("{lab} · {acc}")
            };
            if next.len() > budget.saturating_sub(1) {
                break;
            }
            acc = next;
        }
        format!("{head}…{acc}")
    }
}

/// Blocking GraphQL client with metered posts.
pub struct GraphQl<'a> {
    pub url: String,
    pub http: reqwest::blocking::Client,
    pub meter: &'a GraphQlMeter,
}

impl<'a> GraphQl<'a> {
    pub fn connect(url: &str, meter: &'a GraphQlMeter) -> Result<Self> {
        let http = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(60))
            .user_agent("sui-clmm-tui/0.1")
            .build()
            .context("build http client")?;
        Ok(Self {
            url: url.to_string(),
            http,
            meter,
        })
    }

    /// Paginate objects owned by `owner` with optional Move type filter.
    pub fn owned_objects(
        &self,
        owner: &str,
        type_filter: &str,
        page_size: u32,
        max_pages: u32,
    ) -> Result<Vec<Value>> {
        const Q: &str = r#"
        query Owned($owner: SuiAddress!, $type: String!, $first: Int!, $after: String) {
          objects(first: $first, after: $after, filter: { owner: $owner, type: $type }) {
            pageInfo { hasNextPage endCursor }
            nodes {
              address
              asMoveObject {
                contents {
                  type { repr }
                  json
                }
              }
            }
          }
        }
        "#;
        let mut out = Vec::new();
        let mut after: Option<String> = None;
        for _ in 0..max_pages {
            let vars = json!({
                "owner": owner,
                "type": type_filter,
                "first": page_size,
                "after": after,
            });
            let data = self.post("objects", type_filter, Q, vars)?;
            let objects = data
                .get("objects")
                .ok_or_else(|| anyhow!("missing objects"))?;
            if let Some(nodes) = objects.get("nodes").and_then(|n| n.as_array()) {
                out.extend(nodes.iter().cloned());
            }
            let has_next = objects
                .pointer("/pageInfo/hasNextPage")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_next {
                break;
            }
            after = objects
                .pointer("/pageInfo/endCursor")
                .and_then(|v| v.as_str())
                .map(str::to_string);
            if after.is_none() {
                break;
            }
        }
        Ok(out)
    }

    pub fn object_json(&self, object_id: &str) -> Result<Value> {
        const Q: &str = r#"
        query Obj($id: SuiAddress!) {
          object(address: $id) {
            address
            asMoveObject {
              contents {
                type { repr }
                json
              }
            }
          }
        }
        "#;
        let data = self.post("object", object_id, Q, json!({ "id": object_id }))?;
        data.get("object")
            .cloned()
            .ok_or_else(|| anyhow!("object not found: {object_id}"))
    }

    pub fn post(&self, method: &str, note: &str, query: &str, variables: Value) -> Result<Value> {
        let body = json!({
            "query": query,
            "variables": variables,
        });
        let res = self
            .http
            .post(&self.url)
            .json(&body)
            .send()
            .context("graphql http");
        let ok_http = res.is_ok();
        let res = match res {
            Ok(r) => r,
            Err(e) => {
                self.meter.record(method, note, false);
                return Err(e.into());
            }
        };
        let status = res.status();
        let text = res.text().unwrap_or_default();
        if !status.is_success() {
            self.meter.record(method, note, false);
            return Err(anyhow!("graphql http {status}: {}", truncate(&text, 200)));
        }
        let parsed: Value = serde_json::from_str(&text).context("graphql json")?;
        if let Some(errors) = parsed.get("errors")
            && errors.as_array().is_some_and(|a| !a.is_empty())
        {
            self.meter.record(method, note, false);
            return Err(anyhow!(
                "graphql errors: {}",
                truncate(&errors.to_string(), 300)
            ));
        }
        self.meter.record(method, note, ok_http);
        Ok(parsed
            .get("data")
            .cloned()
            .ok_or_else(|| anyhow!("graphql missing data"))?)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max])
    }
}

/// Placeholder for future gRPC sim/execute wiring.
pub mod grpc_stub {
    /// Default mainnet gRPC host hint (not connected in v0).
    pub const DEFAULT_GRPC_HOST: &str = "fullnode.mainnet.sui.io:443";

    pub fn status() -> &'static str {
        "gRPC stub — SimulateTransaction / ExecuteTransaction deferred"
    }
}
