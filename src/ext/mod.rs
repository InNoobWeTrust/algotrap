/// BingX market-data client integration.
pub mod bingx;
/// Yahoo Finance historical market-data client.
pub mod yfinance;

/// ntfy notification client types.
#[cfg(feature = "ntfy")]
pub mod ntfy;

/// Browser automation support.
#[cfg(feature = "webdriver")]
pub mod webdriver;
