# ETF Dashboard Generator

An automated ETF flow dashboard generator that fetches, processes, and visualizes cryptocurrency ETF data for Bitcoin, Ethereum, and Solana.

## Features

- 📊 **Automated Data Collection**: Scrapes latest ETF flow data from farside.co.uk
- 💰 **Price Integration**: Fetches historical price data from Yahoo Finance
- 📈 **Feature Engineering**: Calculates moving averages, cumulative flows, and aggregations
- 🎨 **Interactive Dashboards**: Generates HTML dashboards with Plotly charts
- 💾 **CSV Export**: Saves all processed data for further analysis

## Output

The application generates files in the `output/etf_dashboard/` directory:

### For Each Asset (BTC, ETH, SOL):

**CSV Files:**
- `{ASSET}_funds_netflow.csv` - Daily netflow for individual funds
- `{ASSET}_total_netflow.csv` - Total netflow with MA20 and cumulative
- `{ASSET}_cumulative_total.csv` - Cumulative netflow over time
- `{ASSET}_price.csv` - Historical asset prices
- `{ASSET}_fund_volumes.csv` - Trading volumes by fund
- `{ASSET}_volume_total.csv` - Total trading volume with MA20

**HTML Dashboard:**
- `{ASSET}_dashboard.html` - Interactive visualization with:
  - Stacked bar chart of individual fund netflows
  - Cumulative netflow line chart
  - Asset price chart
  - Trading volume chart with MA20
  - Data table with recent flows

## Prerequisites

- Rust toolchain (1.91.1 or later)
- GeckoDriver (for Firefox WebDriver)
  - Download from: https://github.com/mozilla/geckodriver/releases
  - Add to PATH or place in project directory
- Firefox browser installed
- Internet connection for data fetching

## Installation

```bash
# Clone the repository
git clone https://github.com/InNoobWeTrust/algotrap.git
cd algotrap

# Build the project
cargo build -p etf_dashboard --release
```

## Usage

### Running the Dashboard Generator

```bash
# Run directly with cargo
cargo run -p etf_dashboard

# Or run the built binary
./target/release/etf_dashboard
```

### Viewing the Dashboards

After running, open the generated HTML files in your browser:

```bash
# On Linux/macOS
open output/etf_dashboard/BTC_dashboard.html

# On Windows
start output/etf_dashboard/BTC_dashboard.html
```

## How It Works

1. **Data Collection**
   - Uses Firefox WebDriver to scrape ETF flow tables from farside.co.uk
   - Fetches price and volume data from Yahoo Finance API

2. **Data Processing**
   - Parses HTML tables into Polars DataFrames
   - Calculates aggregate metrics (total netflow, cumulative flows)
   - Computes 20-period moving averages
   - Joins price and volume data by date

3. **Visualization**
   - Renders interactive charts using Plotly.js
   - Generates responsive HTML dashboards
   - Exports data to CSV for external analysis

## Data Sources

- **ETF Flow Data**: [Farside Investors](https://farside.co.uk/)
  - Bitcoin ETF: https://farside.co.uk/bitcoin-etf-flow-all-data/
  - Ethereum ETF: https://farside.co.uk/ethereum-etf-flow-all-data/
  - Solana ETF: https://farside.co.uk/sol/

- **Price/Volume Data**: Yahoo Finance API

## Configuration

The application uses hardcoded asset tickers and URLs. To modify:

Edit `src/main.rs` and update the constants:

```rust
const BTC_TICKER: &str = "BTC-USD";
const ETH_TICKER: &str = "ETH-USD";
const SOL_TICKER: &str = "SOL-USD";

const ETF_BTC_URL: &str = "https://farside.co.uk/bitcoin-etf-flow-all-data/";
const ETF_ETH_URL: &str = "https://farside.co.uk/ethereum-etf-flow-all-data/";
const ETF_SOL_URL: &str = "https://farside.co.uk/sol/";
```

## Development

### Running Tests

```bash
cargo test -p etf_dashboard
```

### Building Documentation

```bash
cargo doc -p etf_dashboard --open
```

### Project Structure

```
bins/etf_dashboard/
├── Cargo.toml          # Dependencies and metadata
├── README.md           # This file
└── src/
    └── main.rs         # Main application code
```

## Troubleshooting

### GeckoDriver Issues

If you encounter WebDriver errors:

1. Ensure GeckoDriver is in your PATH
2. Check that Firefox is installed
3. Verify GeckoDriver version matches your Firefox version
4. Check `geckodriver.log` for detailed error messages

### Data Fetching Issues

If data fetching fails:

1. Check your internet connection
2. Verify the farside.co.uk URLs are accessible
3. Check if website structure has changed
4. Review log output for specific errors

### Missing Data

If some assets have missing volume or price data:

- The application logs warnings and continues with available data
- Check the log output for specific ticker/fund failures
- Some funds may not have historical data available

## License

This project is part of the algotrap repository. See the main repository for license information.

## Contributing

Contributions are welcome! Please ensure:

- Code follows existing style conventions
- Tests are added for new functionality
- Documentation is updated accordingly
- Commits are small and focused

## Acknowledgments

- ETF flow data from [Farside Investors](https://farside.co.uk/)
- Price/volume data from Yahoo Finance
- Built with [Polars](https://www.pola.rs/) for data processing
- Visualizations powered by [Plotly.js](https://plotly.com/javascript/)
