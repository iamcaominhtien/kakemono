//! Value scales with "nice" ticks (1, 2 or 5 times a power of ten), as
//! Excel chooses them, within the bounds the file fixes.

use crate::chart::Axis;
use crate::excel::numfmt::{self, NumFmt};

/// The most ticks of a scale, whatever its bounds.
const MAX_TICKS: usize = 50;
/// Excel leaves zero off a line chart's scale when the data's spread is
/// less than this share of its largest magnitude.
const ZERO_SPREAD_RATIO: f64 = 1.0 / 6.0;
/// The fewest ticks a scale aims at.
const MIN_TARGET_TICKS: usize = 2;
/// The most times [`readable`] makes a step coarser to stop tick labels
/// repeating (each time at least doubles it).
const MAX_COARSER_STEPS: usize = 8;
/// The most ticks a scale aims at.
const MAX_TARGET_TICKS: usize = 10;
/// Excel's automatic maximum is the first tick above the data's highest
/// value plus this share of their spread (and its minimum, below zero, the
/// first tick below their lowest minus as much): data up to 100 end at
/// 120, not 100.
const HEADROOM_RATIO: f64 = 0.05;

/// Whether a scale reaches zero.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Zero {
    /// Always (bars and areas grow from zero).
    Always,
    /// Unless the data lie far from zero (lines, scatter), as Excel does.
    Auto,
}

/// What the file fixes of a scale: its bounds and its tick step.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub(super) struct Fixed {
    /// The minimum.
    pub(super) min: Option<f64>,
    /// The maximum.
    pub(super) max: Option<f64>,
    /// The distance between two ticks.
    pub(super) unit: Option<f64>,
}

impl Fixed {
    /// What `axis` fixes; nothing when there is no axis.
    pub(super) fn of(axis: Option<&Axis>) -> Self {
        axis.map_or_else(Self::default, |axis| Self {
            min: axis.min,
            max: axis.max,
            unit: axis.major_unit,
        })
    }
}

/// A linear scale: its bounds, its tick step, its direction.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct Scale {
    /// The lowest value.
    pub(super) min: f64,
    /// The highest value.
    pub(super) max: f64,
    /// The distance between two ticks.
    pub(super) step: f64,
    /// The scale runs from its maximum to its minimum (`maxMin`).
    pub(super) reversed: bool,
}

impl Scale {
    /// A scale of the data from `low` to `high` (`None` when there is no
    /// data), with the bounds and step `fixed` the file gives, aiming at
    /// about `target` ticks otherwise; with Excel's [`HEADROOM_RATIO`]
    /// past the data when `headroom` (not for shares of 100 %).
    pub(super) fn nice(
        data: Option<(f64, f64)>,
        fixed: Fixed,
        zero: Zero,
        (target, headroom): (usize, bool),
        reversed: bool,
    ) -> Self {
        let (mut low, mut high) = data.unwrap_or((0.0, 1.0));
        let spread = high - low;
        let reaches_zero = match zero {
            Zero::Always => true,
            Zero::Auto => spread >= ZERO_SPREAD_RATIO * low.abs().max(high.abs()),
        };
        if reaches_zero {
            low = low.min(0.0);
            high = high.max(0.0);
        }

        // Fixed bounds that contradict each other are left out.
        let (fixed_min, fixed_max) = match (fixed.min, fixed.max) {
            (Some(min), Some(max)) if min >= max => (None, None),
            other => other,
        };
        if let Some(min) = fixed_min {
            low = min;
            high = high.max(min);
        }
        if let Some(max) = fixed_max {
            high = max;
            low = low.min(max);
        }
        if high <= low {
            let pad = if low == 0.0 { 1.0 } else { low.abs() / 10.0 };
            if fixed_max.is_none() {
                high = low + pad;
            } else {
                low = high - pad;
            }
        }
        let target = target.clamp(MIN_TARGET_TICKS, MAX_TARGET_TICKS) as f64;
        let mut step = fixed
            .unit
            .filter(|unit| unit.is_finite() && *unit > 0.0 && (high - low) / unit <= MAX_TICKS as f64)
            .unwrap_or_else(|| nice_step((high - low) / target));
        // Never more ticks than the most a scale has.
        while (high - low) / step > MAX_TICKS as f64 {
            step *= 2.0;
        }
        // The data's bounds with Excel's headroom, rounded to the step.
        let spread_data = headroom && data.is_some_and(|(first, last)| last > first);
        let room = if spread_data {
            (high - low) * HEADROOM_RATIO
        } else {
            0.0
        };
        let (low, high) = (
            if low < 0.0 { low - room } else { low },
            if high > 0.0 { high + room } else { high },
        );
        let min = fixed_min.unwrap_or_else(|| (low / step).floor() * step);
        let mut max = fixed_max.unwrap_or_else(|| (high / step).ceil() * step);
        if max <= min {
            max = min + step;
        }
        Self {
            min,
            max,
            step,
            reversed,
        }
    }

    /// The tick values, from the lowest: from the minimum by the step (a
    /// fixed minimum that is not a multiple of the step is the first tick,
    /// as in Excel), rounded to the step's decimals so that 0.1 × 3 shows
    /// as 0.3.
    pub(super) fn ticks(&self) -> Vec<f64> {
        let decimals = (-self.step.log10()).ceil().max(0.0) + 1.0;
        let unit = 10f64.powf(decimals.min(f64::from(f64::MAX_10_EXP)));
        (0..=MAX_TICKS)
            .map(|index| self.min + index as f64 * self.step)
            .take_while(|value| *value <= self.max + self.step * 1e-9)
            .map(|value| {
                let rounded = (value * unit).round() / unit;
                let value = if rounded.is_finite() { rounded } else { value };
                if value == 0.0 { 0.0 } else { value }
            })
            .collect()
    }

    /// Where `value` falls on a scale drawn from `start` (its minimum, or
    /// maximum when reversed) to `end`, clamped to the scale.
    pub(super) fn at(&self, value: f64, start: f32, end: f32) -> f32 {
        let ratio = ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0) as f32;
        let ratio = if self.reversed { 1.0 - ratio } else { ratio };
        start + (end - start) * ratio
    }
}

/// The nice step (1, 2 or 5 times a power of ten) nearest above `raw`.
fn nice_step(raw: f64) -> f64 {
    if !(raw.is_finite() && raw > 0.0) {
        return 1.0;
    }
    let power = 10f64.powf(raw.log10().floor());
    let fraction = raw / power;
    let nice = if fraction <= 1.0 {
        1.0
    } else if fraction <= 2.0 {
        2.0
    } else if fraction <= 5.0 {
        5.0
    } else {
        10.0
    };
    nice * power
}

/// The lowest and highest of `values`; `None` when there is none.
pub(super) fn range(values: impl IntoIterator<Item = f64>) -> Option<(f64, f64)> {
    values.into_iter().fold(None, |acc, value| match acc {
        None => Some((value, value)),
        Some((low, high)) => Some((low.min(value), high.max(value))),
    })
}

/// The scale `build` makes for about `target` ticks, or for fewer when
/// the labels of its ticks in `format` would repeat (steps of 0.5 in a
/// format without decimals show 0, 1, 1, 2, 2).
pub(super) fn readable(build: impl Fn(usize) -> Scale, target: usize, format: &NumFmt, date1904: bool) -> Scale {
    let repeats = |scale: &Scale| {
        let labels: Vec<String> = scale
            .ticks()
            .iter()
            .map(|tick| label(*tick, format, date1904))
            .collect();
        labels.windows(2).any(|pair| pair[0] == pair[1])
    };
    let mut target = target.max(1);
    let mut scale = build(target);
    while repeats(&scale) && target > MIN_TARGET_TICKS {
        target /= 2;
        scale = build(target);
    }
    // At the fewest ticks a target gives, the step itself grows (0.5 → 1
    // → 2 → 5), the bounds widened to multiples of it.
    for _ in 0..MAX_COARSER_STEPS {
        if !repeats(&scale) {
            break;
        }
        let step = nice_step(scale.step * 1.5);
        scale = Scale {
            min: (scale.min / step).floor() * step,
            max: ((scale.max / step).ceil() * step).max((scale.min / step).floor() * step + step),
            step,
            reversed: scale.reversed,
        };
    }
    scale
}

/// The label of the tick `value` in the number format `format`.
pub(super) fn label(value: f64, format: &NumFmt, date1904: bool) -> String {
    numfmt::format(value, format, date1904)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_ticks_from_zero() {
        let scale = Scale::nice(Some((5.0, 37.0)), Fixed::default(), Zero::Always, (5, true), false);
        assert_eq!(scale.ticks(), [0.0, 10.0, 20.0, 30.0, 40.0]);
        let scale = Scale::nice(Some((0.0, 0.37)), Fixed::default(), Zero::Always, (5, true), false);
        assert_eq!(scale.ticks(), [0.0, 0.1, 0.2, 0.3, 0.4]);
        let scale = Scale::nice(Some((-12.0, 7.0)), Fixed::default(), Zero::Always, (4, true), false);
        assert_eq!(scale.ticks(), [-15.0, -10.0, -5.0, 0.0, 5.0, 10.0]);
    }

    #[test]
    fn lines_far_from_zero_leave_it_out() {
        let scale = Scale::nice(Some((92.0, 104.0)), Fixed::default(), Zero::Auto, (5, true), false);
        assert!(scale.min >= 90.0 && scale.max <= 105.0, "{scale:?}");
        let scale = Scale::nice(Some((10.0, 100.0)), Fixed::default(), Zero::Auto, (5, true), false);
        assert_eq!(scale.min, 0.0);
    }

    #[test]
    fn fixed_bounds_win() {
        let scale = Scale::nice(
            Some((0.0, 37.0)),
            Fixed {
                min: Some(-10.0),
                max: Some(50.0),
                unit: None,
            },
            Zero::Always,
            (6, true),
            false,
        );
        assert_eq!(scale.ticks(), [-10.0, 0.0, 10.0, 20.0, 30.0, 40.0, 50.0]);
        // Bounds that contradict each other are left out.
        let scale = Scale::nice(
            Some((0.0, 4.0)),
            Fixed {
                min: Some(9.0),
                max: Some(1.0),
                unit: None,
            },
            Zero::Always,
            (4, true),
            false,
        );
        assert_eq!(scale.ticks(), [0.0, 1.0, 2.0, 3.0, 4.0, 5.0]);
    }

    #[test]
    fn the_maximum_keeps_room_past_the_data_as_excel_s() {
        // Data up to 100 by 20: Excel ends the scale at 120.
        let scale = Scale::nice(Some((0.0, 100.0)), Fixed::default(), Zero::Always, (5, true), false);
        assert_eq!(scale.max, 120.0);
        let scale = Scale::nice(Some((0.0, 94.0)), Fixed::default(), Zero::Always, (5, true), false);
        assert_eq!(scale.max, 100.0);
        // Shares of 100 % end at 100 %.
        let scale = Scale::nice(Some((0.0, 1.0)), Fixed::default(), Zero::Always, (5, false), false);
        assert_eq!(scale.max, 1.0);
    }

    #[test]
    fn the_file_s_unit_sets_the_step() {
        let fixed = Fixed {
            min: Some(40.0),
            max: Some(280.0),
            unit: Some(40.0),
        };
        let scale = Scale::nice(Some((60.0, 270.0)), fixed, Zero::Auto, (5, true), false);
        assert_eq!(scale.ticks(), [40.0, 80.0, 120.0, 160.0, 200.0, 240.0, 280.0]);
        // A unit that would give too many ticks is left out.
        let tiny = Fixed {
            unit: Some(1e-9),
            ..Fixed::default()
        };
        let scale = Scale::nice(Some((0.0, 10.0)), tiny, Zero::Always, (5, true), false);
        assert!(scale.ticks().len() <= MAX_TICKS + 1);
        assert_eq!(scale.ticks().len(), 7);
    }

    #[test]
    fn repeated_labels_take_a_coarser_step() {
        let format = NumFmt::Code("0".into());
        let build = |target| Scale::nice(Some((0.0, 3.0)), Fixed::default(), Zero::Always, (target, true), false);
        assert_eq!(build(7).step, 0.5);
        let scale = readable(build, 7, &format, false);
        assert_eq!(scale.ticks(), [0.0, 1.0, 2.0, 3.0, 4.0]);
    }

    #[test]
    fn a_scale_of_zeros_in_whole_dollars_does_not_repeat_a_label() {
        // All values 0: the scale runs 0 to 1, by 0.5 at the fewest ticks.
        let format = NumFmt::Code("\"$\"#,##0".into());
        let build = |target| Scale::nice(Some((0.0, 0.0)), Fixed::default(), Zero::Always, (target, true), false);
        let scale = readable(build, 5, &format, false);
        let labels: Vec<String> = scale.ticks().iter().map(|tick| label(*tick, &format, false)).collect();
        assert_eq!(labels, ["$0", "$1"]);
    }

    #[test]
    fn odd_data_still_gives_a_scale() {
        for data in [None, Some((3.0, 3.0)), Some((0.0, 0.0)), Some((-1e300, 1e300))] {
            let scale = Scale::nice(data, Fixed::default(), Zero::Auto, (5, true), false);
            assert!(scale.max > scale.min && scale.step > 0.0, "{data:?} {scale:?}");
            let ticks = scale.ticks();
            assert!(!ticks.is_empty() && ticks.len() <= MAX_TICKS + 1);
        }
    }

    #[test]
    fn positions_follow_the_direction() {
        let scale = Scale::nice(Some((0.0, 10.0)), Fixed::default(), Zero::Always, (5, true), false);
        assert_eq!(scale.at(f64::midpoint(scale.min, scale.max), 100.0, 0.0), 50.0);
        assert_eq!(scale.at(50.0, 100.0, 0.0), 0.0);
        let reversed = Scale {
            reversed: true,
            ..scale
        };
        assert_eq!(reversed.at(scale.max, 100.0, 0.0), 100.0);
    }
}
