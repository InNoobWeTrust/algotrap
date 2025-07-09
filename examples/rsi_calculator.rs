use algotrap::ta::prelude::*;
use polars::prelude::*;

fn main() {
    let data = [
        44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
        45.61, 46.28, 46.28, 46.00, 46.03, 46.41, 46.22, 45.64, 46.21,
    ];
    let series = Series::new("price".into(), &data);
    let df = DataFrame::new(vec![series.into()]).unwrap();

    let rsi_len = 14;
    let df = df
        .lazy()
        .with_column(col("price").rsi(rsi_len).alias("rsi"))
        .collect()
        .unwrap();

    dbg!(df);
}