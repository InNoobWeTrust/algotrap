/// Time-policy identifier baked into provenance (§2.4).
pub const TIME_POLICY_ID: &str = "cst-utc8-fixed-v1";

/// Leap-month handling policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LeapMonthPolicy {
    /// Reject leap months; return `TaError::validation` if the date falls in one.
    #[default]
    Reject,
    /// Permit leap months; `lunar_month` is used as-is.
    Allow,
}

/// One plottable I-Ching hexagram-energy channel.
///
/// **Must be created through validated factory functions** (`HexagramEnergy::new`),
/// never via raw struct-literal construction by callers. The invariant
/// `energy == hexagram as f64 - 31.5` is mechanically enforced at construction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HexagramEnergy {
    /// Canonical six-bit hexagram value B = Σ line[i]·2ⁱ. Range 0..=63.
    pub hexagram: u8,
    /// Hexagram value centered around the six-bit range midpoint, in [-31.5, +31.5].
    /// Equal to `hexagram as f64 - 31.5`.
    pub energy: f64,
}

impl HexagramEnergy {
    /// Creates a validated hexagram-energy channel.
    ///
    /// The `energy` field is always computed as `hexagram as f64 - 31.5`.
    /// Callers provide only the canonical hexagram index; energy is derived.
    /// Returns `TaError::validation` if `hexagram > 63`.
    pub fn new(hexagram: u8) -> crate::ta::TaResult<Self> {
        if hexagram > 63 {
            return Err(crate::ta::TaError::validation("hexagram must be 0..=63"));
        }
        Ok(Self {
            hexagram,
            energy: hexagram as f64 - 31.5,
        })
    }
}

/// Complete method-scoped I-Ching reading at one timestamp.
///
/// DTOs are method-neutral; `transformed` and `moving_line` are always `Some`
/// for Plum Blossom but may be `None` for future methods.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct IchingSignal {
    /// Current/root state (本卦). Always present.
    pub original: HexagramEnergy,
    /// Post-trigger state (变卦). `Some` for Plum Blossom; `None` for methods without a moving line.
    pub transformed: Option<HexagramEnergy>,
    /// Internal/mutual characteristic (互卦), structurally derived from `original`.
    /// Always present because every six-line hexagram has a nuclear derivation.
    pub nuclear: HexagramEnergy,
    /// Moving line position 1..=6 (bottom-to-top) for Plum Blossom; `None` otherwise.
    pub moving_line: Option<u8>,
}

/// A three-line I-Ching trigram using the Xiantian (先天) numbering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trigram {
    lines: [u8; 3], // lines[0]=bottom, 1=Yang 0=Yin
}

impl Trigram {
    /// Xiantian trigram number (1..=8). Returns `TaError::validation` for `n ∉ 1..=8`.
    pub fn from_num(n: u8) -> crate::ta::TaResult<Self> {
        let lines = match n {
            1 => [1, 1, 1],
            2 => [1, 1, 0],
            3 => [1, 0, 1],
            4 => [1, 0, 0],
            5 => [0, 1, 1],
            6 => [0, 1, 0],
            7 => [0, 0, 1],
            8 => [0, 0, 0],
            _ => {
                return Err(crate::ta::TaError::validation(
                    "trigram number must be 1..=8",
                ));
            }
        };
        Ok(Self { lines })
    }

    /// Xiantian trigram from a sum modulo 8. Maps `s % 8 == 0` to 8 (Kun).
    pub fn from_num_mod8(s: u32) -> crate::ta::TaResult<Self> {
        let r = s % 8;
        let n = if r == 0 { 8 } else { r as u8 };
        Self::from_num(n)
    }

    /// Bottom-to-top line array `[line[0], line[1], line[2]]`.
    pub fn lines(self) -> [u8; 3] {
        self.lines
    }

    /// Top-to-bottom display string, e.g. `"010"` for Kan.
    pub fn display(self) -> String {
        format!("{}{}{}", self.lines[2], self.lines[1], self.lines[0])
    }

    /// Inverse: reconstruct a Trigram from bottom-to-top lines.
    /// Returns `TaError::validation` if any element is not 0 or 1.
    pub fn from_lines(lines: [u8; 3]) -> crate::ta::TaResult<Self> {
        if lines.iter().any(|&l| l != 0 && l != 1) {
            return Err(crate::ta::TaError::validation(
                "trigram lines must be 0 or 1",
            ));
        }
        Ok(Self { lines })
    }
}

/// A six-line I-Ching hexagram.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hexagram {
    lines: [u8; 6], // lines[0]=bottom, 1=Yang 0=Yin
}

impl Hexagram {
    /// Compose from upper and lower trigrams.
    pub fn from_trigrams(upper: Trigram, lower: Trigram) -> Self {
        let u = upper.lines();
        let l = lower.lines();
        Self {
            lines: [l[0], l[1], l[2], u[0], u[1], u[2]],
        }
    }

    /// Canonical six-bit value B = Σ line[i]·2ⁱ. Range 0..=63.
    pub fn binary_index(self) -> u8 {
        let mut b: u8 = 0;
        for (i, &line) in self.lines.iter().enumerate() {
            if line == 1 {
                b |= 1 << i;
            }
        }
        b
    }

    /// Top-to-bottom 6-char display string (line[5]..line[0]).
    pub fn bits_top_to_bottom(self) -> String {
        format!(
            "{}{}{}{}{}{}",
            self.lines[5],
            self.lines[4],
            self.lines[3],
            self.lines[2],
            self.lines[1],
            self.lines[0]
        )
    }

    /// Reconstruct from a canonical index 0..=63.
    /// Returns `TaError::validation` if `b > 63`.
    pub fn from_binary_index(b: u8) -> crate::ta::TaResult<Self> {
        if b > 63 {
            return Err(crate::ta::TaError::validation(
                "hexagram index must be 0..=63",
            ));
        }
        Ok(Self {
            lines: [
                b & 1,
                (b >> 1) & 1,
                (b >> 2) & 1,
                (b >> 3) & 1,
                (b >> 4) & 1,
                (b >> 5) & 1,
            ],
        })
    }

    /// Flip exactly one line (1=bottom..6=top).
    /// Returns `TaError::validation` if `line ∉ 1..=6`.
    pub fn flip_line(self, line: u8) -> crate::ta::TaResult<Self> {
        if !(1..=6).contains(&line) {
            return Err(crate::ta::TaError::validation("line must be 1..=6"));
        }
        let mut lines = self.lines;
        let idx = (line - 1) as usize;
        lines[idx] ^= 1;
        Ok(Self { lines })
    }

    /// Nuclear hexagram: lower trigram from lines 2,3,4; upper from lines 3,4,5
    /// (1-indexed bottom; 0-indexed: lower=[1,2,3], upper=[2,3,4]).
    pub fn nuclear(self) -> Self {
        let l = self.lines;
        Self {
            lines: [l[1], l[2], l[3], l[2], l[3], l[4]],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ta::TaErrorKind;

    #[test]
    fn hexagram_energy_new_boundary_values() {
        let zero = HexagramEnergy::new(0).expect("0 is valid");
        assert_eq!(zero.hexagram, 0);
        assert_eq!(zero.energy, -31.5);

        let max = HexagramEnergy::new(63).expect("63 is valid");
        assert_eq!(max.hexagram, 63);
        assert_eq!(max.energy, 31.5);
    }

    #[test]
    fn hexagram_energy_new_midpoint_values() {
        let below = HexagramEnergy::new(31).expect("31 is valid");
        assert_eq!(below.energy, -0.5);

        let above = HexagramEnergy::new(32).expect("32 is valid");
        assert_eq!(above.energy, 0.5);
    }

    #[test]
    fn hexagram_energy_new_rejects_out_of_range() {
        let err = HexagramEnergy::new(64).expect_err("64 must be rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn trigram_from_num_round_trips_lines_and_display() {
        let cases: [(u8, [u8; 3], &str); 8] = [
            (1, [1, 1, 1], "111"),
            (2, [1, 1, 0], "011"),
            (3, [1, 0, 1], "101"),
            (4, [1, 0, 0], "001"),
            (5, [0, 1, 1], "110"),
            (6, [0, 1, 0], "010"),
            (7, [0, 0, 1], "100"),
            (8, [0, 0, 0], "000"),
        ];
        for (n, lines, display) in cases {
            let t = Trigram::from_num(n).expect("1..=8 is valid");
            assert_eq!(t.lines(), lines, "lines for trigram {n}");
            assert_eq!(t.display(), display, "display for trigram {n}");
        }
    }

    #[test]
    fn trigram_from_num_rejects_zero_and_nine() {
        let err0 = Trigram::from_num(0).expect_err("0 must be rejected");
        assert_eq!(err0.kind, TaErrorKind::Validation);
        let err9 = Trigram::from_num(9).expect_err("9 must be rejected");
        assert_eq!(err9.kind, TaErrorKind::Validation);
    }

    #[test]
    fn trigram_from_num_mod8_maps_zero_eight_one() {
        let kun0 = Trigram::from_num_mod8(0).expect("0 mod 8 maps to Kun");
        assert_eq!(kun0.lines(), [0, 0, 0]);
        assert_eq!(kun0.display(), "000");

        let kun8 = Trigram::from_num_mod8(8).expect("8 maps to Kun");
        assert_eq!(kun8.lines(), [0, 0, 0]);
        assert_eq!(kun8, Trigram::from_num(8).expect("8 is valid"));

        let qian1 = Trigram::from_num_mod8(1).expect("1 maps to Qian");
        assert_eq!(qian1.lines(), [1, 1, 1]);
        assert_eq!(qian1.display(), "111");
    }

    #[test]
    fn trigram_from_lines_round_trip_and_rejects_non_binary() {
        let t = Trigram::from_lines([1, 0, 1]).expect("[1,0,1] is valid");
        assert_eq!(t.lines(), [1, 0, 1]);
        assert_eq!(t, Trigram::from_num(3).expect("3 is Li"));

        let err = Trigram::from_lines([2, 0, 1]).expect_err("2 is non-binary");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn trigram_display_is_three_char_top_to_bottom_binary() {
        for n in 1..=8u8 {
            let t = Trigram::from_num(n).expect("1..=8 is valid");
            let d = t.display();
            assert_eq!(d.len(), 3, "display len for trigram {n}");
            assert!(
                d.chars().all(|c| c == '0' || c == '1'),
                "display chars for trigram {n}"
            );
            let lines = t.lines();
            let expected = format!("{}{}{}", lines[2], lines[1], lines[0]);
            assert_eq!(d, expected, "top-to-bottom order for trigram {n}");
        }
    }

    #[test]
    fn hexagram_from_trigrams_composes_upper_lower() {
        let qian = Trigram::from_num(1).expect("Qian is valid");
        let kun = Trigram::from_num(8).expect("Kun is valid");
        let lower_qian_upper_kun = Hexagram::from_trigrams(kun, qian);
        assert_eq!(lower_qian_upper_kun.binary_index(), 7);
        assert_eq!(lower_qian_upper_kun.bits_top_to_bottom(), "000111");
        let lower_kun_upper_qian = Hexagram::from_trigrams(qian, kun);
        assert_eq!(lower_kun_upper_qian.binary_index(), 56);
        assert_eq!(lower_kun_upper_qian.bits_top_to_bottom(), "111000");
    }

    #[test]
    fn hexagram_binary_index_roundtrip_exhaustive_and_display_len() {
        for b in 0..=63u8 {
            let h = Hexagram::from_binary_index(b).expect("0..=63 is valid");
            assert_eq!(h.binary_index(), b, "roundtrip for {b}");
            let bits = h.bits_top_to_bottom();
            assert_eq!(bits.len(), 6, "display len for {b}");
            assert!(
                bits.chars().all(|c| c == '0' || c == '1'),
                "display chars for {b}"
            );
            let expected = format!(
                "{}{}{}{}{}{}",
                (b >> 5) & 1,
                (b >> 4) & 1,
                (b >> 3) & 1,
                (b >> 2) & 1,
                (b >> 1) & 1,
                b & 1
            );
            assert_eq!(bits, expected, "top-to-bottom order for {b}");
        }
    }

    #[test]
    fn hexagram_qian_index63_display() {
        let qian = Trigram::from_num(1).expect("Qian is valid");
        let hex = Hexagram::from_trigrams(qian, qian);
        assert_eq!(hex.binary_index(), 63);
        assert_eq!(hex.bits_top_to_bottom(), "111111");
        let decoded = Hexagram::from_binary_index(63).expect("63 is valid");
        assert_eq!(decoded, hex);
        assert_eq!(decoded.bits_top_to_bottom(), "111111");
    }

    #[test]
    fn hexagram_flip_line_bottom_and_top() {
        let all_yin = Hexagram::from_binary_index(0).expect("0 is valid");
        let flipped_bottom = all_yin.flip_line(1).expect("line 1 is valid");
        assert_eq!(flipped_bottom.binary_index(), 1);
        assert_eq!(flipped_bottom.bits_top_to_bottom(), "000001");
        let flipped_top = all_yin.flip_line(6).expect("line 6 is valid");
        assert_eq!(flipped_top.binary_index(), 32);
        assert_eq!(flipped_top.bits_top_to_bottom(), "100000");

        let all_yang = Hexagram::from_binary_index(63).expect("63 is valid");
        let yang_flip_bottom = all_yang.flip_line(1).expect("line 1 is valid");
        assert_eq!(yang_flip_bottom.binary_index(), 62);
        let yang_flip_top = all_yang.flip_line(6).expect("line 6 is valid");
        assert_eq!(yang_flip_top.binary_index(), 31);
    }

    #[test]
    fn hexagram_flip_line_rejects_out_of_range() {
        let h = Hexagram::from_binary_index(0).expect("0 is valid");
        let err0 = h.flip_line(0).expect_err("0 must be rejected");
        assert_eq!(err0.kind, TaErrorKind::Validation);
        let err7 = h.flip_line(7).expect_err("7 must be rejected");
        assert_eq!(err7.kind, TaErrorKind::Validation);
    }

    #[test]
    fn hexagram_nuclear_mapping() {
        // Original bottom->top [1,0,1,0,1,0] (index 21).
        let original = Hexagram::from_binary_index(21).expect("21 is valid");
        let lower = Trigram::from_lines([0, 1, 0]).expect("binary lines");
        let upper = Trigram::from_lines([1, 0, 1]).expect("binary lines");
        let expected = Hexagram::from_trigrams(upper, lower);
        assert_eq!(original.nuclear(), expected);
        assert_eq!(original.nuclear().binary_index(), 42);
        assert_eq!(original.nuclear().bits_top_to_bottom(), "101010");

        // Qian and Kun are nuclear-stable.
        let qian = Trigram::from_num(1).expect("Qian is valid");
        let qian_hex = Hexagram::from_trigrams(qian, qian);
        assert_eq!(qian_hex.nuclear(), qian_hex);
        let kun = Trigram::from_num(8).expect("Kun is valid");
        let kun_hex = Hexagram::from_trigrams(kun, kun);
        assert_eq!(kun_hex.nuclear(), kun_hex);
    }

    #[test]
    fn hexagram_is_copy_clone_eq() {
        fn assert_copy_clone_eq<T: Copy + Clone + PartialEq + Eq>() {}
        assert_copy_clone_eq::<Hexagram>();
        let a = Hexagram::from_binary_index(21).expect("21 is valid");
        let b = a;
        assert_eq!(a, b);
        let c = Clone::clone(&a);
        assert_eq!(a, c);
    }

    #[test]
    fn hexagram_from_binary_index_rejects_64() {
        let err = Hexagram::from_binary_index(64).expect_err("64 rejected");
        assert_eq!(err.kind, TaErrorKind::Validation);
    }

    #[test]
    fn trigram_roundtrip_all_xiantian() {
        for n in 1..=8u8 {
            let t = Trigram::from_num(n).expect("1..=8 is valid");
            let lines = t.lines();
            let rt = Trigram::from_lines(lines).expect("lines are binary");
            assert_eq!(rt, t, "from_lines roundtrip for trigram {n}");
            assert_eq!(rt.lines(), lines, "lines roundtrip for trigram {n}");
        }
    }

    #[test]
    fn hexagram_exhaustive_roundtrip() {
        for b in 0..=63u8 {
            let h = Hexagram::from_binary_index(b).expect("0..=63 is valid");
            assert_eq!(h.binary_index(), b, "roundtrip for {b}");
        }
    }

    #[test]
    fn flip_line_double_flip_identity() {
        let original = Hexagram::from_binary_index(21).expect("21 is valid");
        let flipped = original.flip_line(3).expect("line 3 is valid");
        assert_ne!(flipped, original, "single flip changes hexagram");
        let restored = flipped.flip_line(3).expect("line 3 is valid");
        assert_eq!(restored, original, "double flip restores original");
    }

    #[test]
    fn nuclear_qian_is_qian() {
        let qian = Trigram::from_num(1).expect("Qian is valid");
        let qian_hex = Hexagram::from_trigrams(qian, qian);
        assert_eq!(qian_hex.binary_index(), 63);
        assert_eq!(qian_hex.nuclear(), qian_hex);
    }

    #[test]
    fn nuclear_kun_is_kun() {
        let kun = Trigram::from_num(8).expect("Kun is valid");
        let kun_hex = Hexagram::from_trigrams(kun, kun);
        assert_eq!(kun_hex.binary_index(), 0);
        assert_eq!(kun_hex.nuclear(), kun_hex);
    }
}
