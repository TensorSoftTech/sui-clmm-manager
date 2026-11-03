//! External quote / price providers (Aftermath ≈ Sol Jupiter/Titan).

pub mod aftermath;

pub use aftermath::{AFTERMATH_PRICES_URL, fetch_usd_prices, fetch_usd_prices_info};
