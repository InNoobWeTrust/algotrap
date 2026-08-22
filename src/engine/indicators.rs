//! Indicator binding registry.
//!
//! Single source of truth wiring a requested [`ValidatedIndicator`] to
//! (a) the TA leaves required to compute it and (b) the frame columns it
//! advertises. Adding an indicator means adding one enum variant plus one
//! binding arm here; column lists, SQL projections, and TA projections all
//! derive from the binding, and the compiler-enforced exhaustive `match`
//! rejects unregistered variants.

use crate::engine::validation::ValidatedIndicator;
use crate::ta::indicator::IndicatorOutput;

/// Binds one requested indicator to its computation leaves and frame columns.
pub(crate) struct IndicatorBinding {
    /// TA outputs required to compute this indicator. Empty for pure
    /// presentation or time columns that need no TA kernel.
    pub leaves: &'static [IndicatorOutput],
    /// Advertised frame columns when they intentionally differ from the
    /// leaves' canonical names (or when there are no leaves); empty means
    /// "derive one column per leaf via [`IndicatorOutput::column_name`]".
    pub explicit_columns: &'static [&'static str],
}

/// Resolves the binding for every indicator variant.
///
/// The match is exhaustive: introducing a variant without registering it here
/// is a compile error, which is what makes the registry safe to extend.
pub(crate) fn binding(indicator: &ValidatedIndicator) -> IndicatorBinding {
    use IndicatorOutput as Io;
    use ValidatedIndicator as Vi;

    match indicator {
        Vi::SMA => bind(&[Io::VolumeSma]),
        Vi::EMA => bind(&[Io::Ema200]),
        Vi::RSI => bind(&[Io::Rssi, Io::RssiMa]),
        Vi::RevRsi => bind(&[
            Io::NeutralReverseRsi,
            Io::BullishReverseRsi,
            Io::BearishReverseRsi,
        ]),
        Vi::ATR => bind(&[Io::AtrUpperBand, Io::AtrLowerBand, Io::AtrPercent]),
        Vi::ATRRevPercent => bind(&[Io::AtrReversionPercent]),
        Vi::BandReversion => bind(&[Io::BandReversion]),
        Vi::BiasReversion => bind(&[Io::BiasReversion]),
        Vi::Sharpe => bind(&[Io::Sharpe]),
        Vi::StructurePower => bind(&[Io::StructurePower, Io::StructurePowerSma]),
        Vi::IsAtrGap => bind(&[Io::IsAtrGap]),
        Vi::BodyRatio => bind(&[Io::BodyRatio]),
        // Leverage is an engine-owned presentation expression over ATR, so it
        // requires the ATR leaf but advertises its own derived column.
        Vi::Leverage => IndicatorBinding {
            leaves: &[Io::Atr],
            explicit_columns: &["leverage"],
        },
        // Date is engine-owned time formatting with no TA involvement.
        Vi::Date => IndicatorBinding {
            leaves: &[],
            explicit_columns: &["Date"],
        },
    }
}

const fn bind(leaves: &'static [IndicatorOutput]) -> IndicatorBinding {
    IndicatorBinding {
        leaves,
        explicit_columns: &[],
    }
}

/// Returns the TA leaves required to compute this indicator.
pub(crate) fn leaves(indicator: &ValidatedIndicator) -> &'static [IndicatorOutput] {
    binding(indicator).leaves
}

/// Returns the frame columns this indicator contributes, in canonical order.
pub(crate) fn advertised_columns(indicator: &ValidatedIndicator) -> Vec<String> {
    let registered = binding(indicator);
    if registered.explicit_columns.is_empty() {
        registered
            .leaves
            .iter()
            .map(|leaf| leaf.column_name().to_string())
            .collect()
    } else {
        registered
            .explicit_columns
            .iter()
            .map(|column| (*column).to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_advertise_exactly_their_leaf_columns() {
        for indicator in [
            ValidatedIndicator::SMA,
            ValidatedIndicator::EMA,
            ValidatedIndicator::RSI,
            ValidatedIndicator::RevRsi,
            ValidatedIndicator::ATR,
            ValidatedIndicator::ATRRevPercent,
            ValidatedIndicator::BandReversion,
            ValidatedIndicator::BiasReversion,
            ValidatedIndicator::Sharpe,
            ValidatedIndicator::StructurePower,
            ValidatedIndicator::IsAtrGap,
            ValidatedIndicator::BodyRatio,
        ] {
            let expected: Vec<String> = leaves(&indicator)
                .iter()
                .map(|leaf| leaf.column_name().to_string())
                .collect();
            assert_eq!(
                advertised_columns(&indicator),
                expected,
                "binding for {indicator:?} must derive columns from its leaves"
            );
        }
    }

    #[test]
    fn leverage_requires_atr_but_advertises_its_own_column() {
        let registered = binding(&ValidatedIndicator::Leverage);
        assert_eq!(registered.leaves, &[IndicatorOutput::Atr]);
        assert_eq!(
            advertised_columns(&ValidatedIndicator::Leverage),
            vec!["leverage"]
        );
    }

    #[test]
    fn date_needs_no_ta_leaves() {
        assert!(binding(&ValidatedIndicator::Date).leaves.is_empty());
        assert_eq!(advertised_columns(&ValidatedIndicator::Date), vec!["Date"]);
    }

    #[test]
    fn every_computable_indicator_binds_at_least_one_column() {
        for indicator in [
            ValidatedIndicator::SMA,
            ValidatedIndicator::EMA,
            ValidatedIndicator::RSI,
            ValidatedIndicator::RevRsi,
            ValidatedIndicator::ATR,
            ValidatedIndicator::ATRRevPercent,
            ValidatedIndicator::BandReversion,
            ValidatedIndicator::BiasReversion,
            ValidatedIndicator::Sharpe,
            ValidatedIndicator::StructurePower,
            ValidatedIndicator::IsAtrGap,
            ValidatedIndicator::BodyRatio,
            ValidatedIndicator::Leverage,
            ValidatedIndicator::Date,
        ] {
            assert!(
                !advertised_columns(&indicator).is_empty(),
                "{indicator:?} advertises no columns"
            );
        }
    }
}
