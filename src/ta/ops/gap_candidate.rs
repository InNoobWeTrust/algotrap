//! Proprietary quantized price-movement signal ("gap zones").
//!
//! This is not conventional support/resistance, not a generic volume profile,
//! and not a generic filled-gap heuristic.
//!
//! Rationale: price movement is used as a more reliable cross-market proxy than
//! direct volume feeds. Qualifying candle-body boundaries represent inferred
//! high-confidence concentration boundaries, while sparse boundaries between them
//! indicate lower trade concentration / empty ranges.
//!
//! Two-layer architecture and ownership:
//! - Pure TA/stream layer (this module): computes only scalar per-closed-candle
//!   detection facts from stateful upstream ATR/RSI-derived inputs plus candle
//!   geometry. It must never own a dynamic zone list, DuckDB, chart rendering, or
//!   trade decisions.
//! - Downstream query layer (`crate::query::gap_zones`): materializes a bounded,
//!   ephemeral, time-sorted relation of the latest qualifying prior zones with full
//!   raw candle metadata. It deliberately returns RAW zones and applies no trust,
//!   RSSI weighting, invalidation, concentration, empty-range, or nearest-limit
//!   aggregation. Consumers (charts, LLM tooling) render the retained zones with
//!   opacity and derive border-concentration / empty-range insight themselves,
//!   composing it with other signals — the LLM performs the proprietary weighting
//!   and synthesis rather than a fixed analytical formula.
//!
//! Operational rules: closed candles only; the decision candle does not create a
//! usable zone for itself; no generic touch/fill/cross invalidation; do not collapse
//! the relation into only one nearest upper/lower value; do not persist/carry a
//! long-lived dynamic zone list.
//!
//! Intended use: compose zone-border concentrations and empty-range traversal context
//! with independent signals to reason about eligibility, momentum/traversal potential,
//! and suitable SL/TP limit regions. This signal promises no results and is not
//! standalone financial advice.
//!
//! Pure scalar gap-candidate facts for one supplied closed row.
//!
//! Validates the row's required scalar inputs, applies exactly
//! `is_atr_gap == true && body_ratio >= body_ratio_threshold` with a
//! caller-supplied threshold, and returns qualification plus nullable
//! candidate body bounds and direction. The operation is stateless: all
//! stateful work has already produced `is_atr_gap` and `body_ratio`
//! upstream. The conventional `0.618` value belongs to application
//! defaults, not to this pure TA operation.

use crate::ta::{TaError, TaResult, validate_finite_value};

/// Required scalar inputs for one supplied closed row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapCandidateInput {
    pub open: f64,
    pub close: f64,
    pub is_atr_gap: bool,
    pub body_ratio: f64,
    /// Caller-supplied qualification threshold, inclusive `[0.0, 1.0]`.
    ///
    /// The consumer owns this value (for example an application default
    /// of `0.618`); this module only validates and applies it.
    pub body_ratio_threshold: f64,
}

/// Candle-body direction for a qualifying candidate row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GapCandidateDirection {
    Bullish,
    Bearish,
    Flat,
}

/// Scalar candidate facts for exactly one supplied closed row.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GapCandidateFacts {
    pub qualifies: bool,
    pub body_bottom: Option<f64>,
    pub body_top: Option<f64>,
    pub direction: Option<GapCandidateDirection>,
}

/// Validates one row's scalar inputs and returns its candidate facts.
///
/// Validation order is `open`, then `close`, then `body_ratio`, then
/// `body_ratio_threshold`, using the existing TA finite-value validation
/// behavior. `body_ratio_threshold` must additionally lie in inclusive
/// `[0.0, 1.0]`. Qualification is exactly
/// `is_atr_gap && body_ratio >= body_ratio_threshold`.
pub fn gap_candidate_facts(input: GapCandidateInput) -> TaResult<GapCandidateFacts> {
    validate_finite_value("gap candidate open", input.open)?;
    validate_finite_value("gap candidate close", input.close)?;
    validate_finite_value("gap candidate body_ratio", input.body_ratio)?;
    validate_finite_value(
        "gap candidate body_ratio_threshold",
        input.body_ratio_threshold,
    )?;
    if !(0.0..=1.0).contains(&input.body_ratio_threshold) {
        return Err(TaError::validation(
            "gap candidate body_ratio_threshold must be in [0.0, 1.0]",
        ));
    }

    if !(input.is_atr_gap && input.body_ratio >= input.body_ratio_threshold) {
        return Ok(GapCandidateFacts {
            qualifies: false,
            body_bottom: None,
            body_top: None,
            direction: None,
        });
    }

    let direction = if input.close > input.open {
        GapCandidateDirection::Bullish
    } else if input.close < input.open {
        GapCandidateDirection::Bearish
    } else {
        GapCandidateDirection::Flat
    };

    Ok(GapCandidateFacts {
        qualifies: true,
        body_bottom: Some(input.open.min(input.close)),
        body_top: Some(input.open.max(input.close)),
        direction: Some(direction),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaErrorKind;

    #[test]
    fn inclusive_boundary_body_ratio_qualifies() {
        let facts = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.618,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(facts.qualifies);
        assert_eq!(facts.body_bottom, Some(100.0));
        assert_eq!(facts.body_top, Some(105.0));
        assert_eq!(facts.direction, Some(GapCandidateDirection::Bullish));
    }

    #[test]
    fn both_predicate_operands_are_mandatory() {
        let just_below = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.6179999999999999,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(!just_below.qualifies);
        assert_eq!(just_below.body_bottom, None);
        assert_eq!(just_below.body_top, None);
        assert_eq!(just_below.direction, None);

        let gap_false = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: false,
            body_ratio: 0.618,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(!gap_false.qualifies);
        assert_eq!(gap_false.body_bottom, None);
        assert_eq!(gap_false.body_top, None);
        assert_eq!(gap_false.direction, None);

        let gap_false_above = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: false,
            body_ratio: 0.9,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(!gap_false_above.qualifies);
        assert_eq!(gap_false_above.body_bottom, None);
        assert_eq!(gap_false_above.body_top, None);
        assert_eq!(gap_false_above.direction, None);
    }

    #[test]
    fn qualifying_directions_produce_ordered_bounds() {
        let bullish = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.8,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(bullish.qualifies);
        assert_eq!(bullish.body_bottom, Some(100.0));
        assert_eq!(bullish.body_top, Some(105.0));
        assert_eq!(bullish.direction, Some(GapCandidateDirection::Bullish));

        let bearish = gap_candidate_facts(GapCandidateInput {
            open: 105.0,
            close: 100.0,
            is_atr_gap: true,
            body_ratio: 0.8,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(bearish.qualifies);
        assert_eq!(bearish.body_bottom, Some(100.0));
        assert_eq!(bearish.body_top, Some(105.0));
        assert_eq!(bearish.direction, Some(GapCandidateDirection::Bearish));

        let flat = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 100.0,
            is_atr_gap: true,
            body_ratio: 0.618,
            body_ratio_threshold: 0.618,
        })
        .unwrap();
        assert!(flat.qualifies);
        assert_eq!(flat.body_bottom, Some(100.0));
        assert_eq!(flat.body_top, Some(100.0));
        assert_eq!(flat.direction, Some(GapCandidateDirection::Flat));
    }

    #[test]
    fn non_candidates_return_false_with_all_nulls() {
        let fixtures = [
            GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.5,
                body_ratio_threshold: 0.618,
            },
            GapCandidateInput {
                open: 105.0,
                close: 100.0,
                is_atr_gap: false,
                body_ratio: 0.9,
                body_ratio_threshold: 0.618,
            },
            GapCandidateInput {
                open: 100.0,
                close: 100.0,
                is_atr_gap: false,
                body_ratio: 0.0,
                body_ratio_threshold: 0.0,
            },
        ];
        for input in fixtures {
            let facts = gap_candidate_facts(input).unwrap();
            assert!(!facts.qualifies);
            assert_eq!(facts.body_bottom, None);
            assert_eq!(facts.body_top, None);
            assert_eq!(facts.direction, None);
        }
    }

    #[test]
    fn non_finite_inputs_return_validation_errors_in_order() {
        let non_finite = [f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        for bad in non_finite {
            let err = gap_candidate_facts(GapCandidateInput {
                open: bad,
                close: 100.0,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: 0.618,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(err.message, "gap candidate open must be finite");

            let err = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: bad,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: 0.618,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(err.message, "gap candidate close must be finite");

            let err = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: bad,
                body_ratio_threshold: 0.618,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(err.message, "gap candidate body_ratio must be finite");

            let err = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: bad,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(
                err.message,
                "gap candidate body_ratio_threshold must be finite"
            );
        }

        let err = gap_candidate_facts(GapCandidateInput {
            open: f64::NAN,
            close: f64::INFINITY,
            is_atr_gap: true,
            body_ratio: f64::NEG_INFINITY,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(err.kind, TaErrorKind::Validation);
        assert_eq!(err.message, "gap candidate open must be finite");

        let err = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: f64::NAN,
            is_atr_gap: true,
            body_ratio: f64::NAN,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(err.kind, TaErrorKind::Validation);
        assert_eq!(err.message, "gap candidate close must be finite");

        let err = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: false,
            body_ratio: f64::NAN,
            body_ratio_threshold: 0.618,
        })
        .unwrap_err();
        assert_eq!(err.kind, TaErrorKind::Validation);
        assert_eq!(err.message, "gap candidate body_ratio must be finite");

        let err = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.8,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(err.kind, TaErrorKind::Validation);
        assert_eq!(
            err.message,
            "gap candidate body_ratio_threshold must be finite"
        );
    }

    #[test]
    fn repeated_calls_are_deterministic_without_state() {
        let input = GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.8,
            body_ratio_threshold: 0.618,
        };
        let first = gap_candidate_facts(input).unwrap();
        let second = gap_candidate_facts(input).unwrap();
        assert_eq!(first, second);

        let non_candidate = GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: false,
            body_ratio: 0.9,
            body_ratio_threshold: 0.618,
        };
        let first = gap_candidate_facts(non_candidate).unwrap();
        let second = gap_candidate_facts(non_candidate).unwrap();
        assert_eq!(first, second);
        assert!(!second.qualifies);
    }

    #[test]
    fn equality_boundary_against_supplied_threshold_qualifies() {
        for threshold in [0.0, 0.5, 0.618, 0.75, 1.0] {
            let facts = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: threshold,
                body_ratio_threshold: threshold,
            })
            .unwrap();
            assert!(
                facts.qualifies,
                "body_ratio == threshold ({threshold}) must qualify"
            );
            assert_eq!(facts.body_bottom, Some(100.0));
            assert_eq!(facts.body_top, Some(105.0));
            assert_eq!(facts.direction, Some(GapCandidateDirection::Bullish));
        }

        let just_below = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.7499999999999999,
            body_ratio_threshold: 0.75,
        })
        .unwrap();
        assert!(!just_below.qualifies);
        assert_eq!(just_below.body_bottom, None);
        assert_eq!(just_below.body_top, None);
        assert_eq!(just_below.direction, None);
    }

    #[test]
    fn supplied_threshold_changes_qualification_for_same_candidate() {
        let base = GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.6,
            body_ratio_threshold: 0.5,
        };
        let low_threshold = gap_candidate_facts(base).unwrap();
        assert!(low_threshold.qualifies);
        assert_eq!(low_threshold.body_bottom, Some(100.0));
        assert_eq!(low_threshold.body_top, Some(105.0));

        let high_threshold = gap_candidate_facts(GapCandidateInput {
            body_ratio_threshold: 0.7,
            ..base
        })
        .unwrap();
        assert!(!high_threshold.qualifies);
        assert_eq!(high_threshold.body_bottom, None);
        assert_eq!(high_threshold.body_top, None);
        assert_eq!(high_threshold.direction, None);

        let exact_threshold = gap_candidate_facts(GapCandidateInput {
            body_ratio_threshold: 0.6,
            ..base
        })
        .unwrap();
        assert!(exact_threshold.qualifies);
    }

    #[test]
    fn non_finite_threshold_returns_validation_error_in_order() {
        for bad in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let err = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: bad,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(
                err.message,
                "gap candidate body_ratio_threshold must be finite"
            );
        }

        let open_wins = gap_candidate_facts(GapCandidateInput {
            open: f64::NAN,
            close: f64::NAN,
            is_atr_gap: true,
            body_ratio: f64::NAN,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(open_wins.message, "gap candidate open must be finite");

        let close_wins = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: f64::INFINITY,
            is_atr_gap: true,
            body_ratio: f64::NAN,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(close_wins.message, "gap candidate close must be finite");

        let body_ratio_wins = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: f64::NEG_INFINITY,
            body_ratio_threshold: f64::NAN,
        })
        .unwrap_err();
        assert_eq!(
            body_ratio_wins.message,
            "gap candidate body_ratio must be finite"
        );
    }

    #[test]
    fn out_of_range_threshold_returns_validation_error() {
        for bad in [-1.0, -0.000001, 1.0000000001, 1.5, 2.0] {
            let err = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: bad,
            })
            .unwrap_err();
            assert_eq!(err.kind, TaErrorKind::Validation);
            assert_eq!(
                err.message,
                "gap candidate body_ratio_threshold must be in [0.0, 1.0]"
            );
        }

        for edge in [0.0, 1.0] {
            let facts = gap_candidate_facts(GapCandidateInput {
                open: 100.0,
                close: 105.0,
                is_atr_gap: true,
                body_ratio: 0.8,
                body_ratio_threshold: edge,
            })
            .unwrap();
            assert_eq!(facts.qualifies, 0.8 >= edge);
        }

        let zero_threshold = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: 0.0,
            body_ratio_threshold: 0.0,
        })
        .unwrap();
        assert!(zero_threshold.qualifies);

        let body_ratio_error_wins_over_range = gap_candidate_facts(GapCandidateInput {
            open: 100.0,
            close: 105.0,
            is_atr_gap: true,
            body_ratio: f64::NAN,
            body_ratio_threshold: 2.0,
        })
        .unwrap_err();
        assert_eq!(
            body_ratio_error_wins_over_range.message,
            "gap candidate body_ratio must be finite"
        );
    }
}
