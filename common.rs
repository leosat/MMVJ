use anyhow::{bail, Result};
use num_traits::{Float, FromPrimitive, Num, NumCast};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::Debug;
use std::ops::{Add, Div};

use strum_macros::{Display, EnumString};
#[derive(Debug, PartialEq, Display, EnumString, Clone, Copy)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
pub enum MmVjEventCode {
    #[strum(serialize = "ABS_X")]
    AbsX,
    #[strum(serialize = "ABS_Y")]
    AbsY,
    #[strum(serialize = "ABS_Z")]
    AbsZ,
    #[strum(serialize = "ABS_RX")]
    AbsRX,
    #[strum(serialize = "ABS_RY")]
    AbsRY,
    #[strum(serialize = "ABS_RZ")]
    AbsRZ,
    #[strum(serialize = "REL_X")]
    MouseRelX,
    #[strum(serialize = "REL_Y")]
    MouseRelY,
    #[strum(serialize = "REL_WHEEL")]
    MouseScrollWheel,
    #[strum(serialize = "REL_HWHEEL")]
    MouseScrollHWheel,
    #[strum(serialize = "BTN_START")]
    MouseBtnStart,
    #[strum(serialize = "BTN_SELECT")]
    MouseBtnSelect,
    #[strum(serialize = "BTN_LEFT")]
    MouseBtnLeft,
    #[strum(serialize = "BTN_RIGHT")]
    MouseBtnRight,
    #[strum(serialize = "BTN_MIDDLE")]
    MouseBtnMiddle,
    #[strum(serialize = "BTN_SIDE")]
    MouseBtnSide,
    #[strum(serialize = "BTN_EXTRA")]
    MouseBtnExtra,
    Unknown(u16),
}

impl From<evdev::InputEvent> for MmVjEventCode {
    fn from(event: evdev::InputEvent) -> Self {
        let code = event.code();
        match event.event_type() {
            evdev::EventType::ABSOLUTE => match code {
                c if c == evdev::AbsoluteAxisCode::ABS_X.0 => MmVjEventCode::AbsX,
                c if c == evdev::AbsoluteAxisCode::ABS_Y.0 => MmVjEventCode::AbsY,
                c if c == evdev::AbsoluteAxisCode::ABS_Z.0 => MmVjEventCode::AbsZ,
                c if c == evdev::AbsoluteAxisCode::ABS_RX.0 => MmVjEventCode::AbsRX,
                c if c == evdev::AbsoluteAxisCode::ABS_RY.0 => MmVjEventCode::AbsRY,
                c if c == evdev::AbsoluteAxisCode::ABS_RZ.0 => MmVjEventCode::AbsRZ,
                _ => MmVjEventCode::Unknown(code),
            },
            evdev::EventType::RELATIVE => match code {
                c if c == evdev::RelativeAxisCode::REL_X.0 => MmVjEventCode::MouseRelX,
                c if c == evdev::RelativeAxisCode::REL_Y.0 => MmVjEventCode::MouseRelY,
                c if c == evdev::RelativeAxisCode::REL_WHEEL.0 => MmVjEventCode::MouseScrollWheel,
                c if c == evdev::RelativeAxisCode::REL_HWHEEL.0 => MmVjEventCode::MouseScrollHWheel,
                _ => MmVjEventCode::Unknown(code),
            },
            evdev::EventType::KEY => match code {
                c if c == evdev::KeyCode::BTN_START.0 => MmVjEventCode::MouseBtnStart,
                c if c == evdev::KeyCode::BTN_SELECT.0 => MmVjEventCode::MouseBtnSelect,
                c if c == evdev::KeyCode::BTN_LEFT.0 => MmVjEventCode::MouseBtnLeft,
                c if c == evdev::KeyCode::BTN_RIGHT.0 => MmVjEventCode::MouseBtnRight,
                c if c == evdev::KeyCode::BTN_MIDDLE.0 => MmVjEventCode::MouseBtnMiddle,
                c if c == evdev::KeyCode::BTN_SIDE.0 => MmVjEventCode::MouseBtnSide,
                c if c == evdev::KeyCode::BTN_EXTRA.0 => MmVjEventCode::MouseBtnExtra,
                _ => MmVjEventCode::Unknown(code),
            },
            _ => MmVjEventCode::Unknown(code),
        }
    }
}

pub mod mmvj_event_code_serde {
    use crate::common::MmVjEventCode;
    use serde::{de, Deserialize, Deserializer, Serializer};
    pub fn serialize<S: Serializer>(
        code: &MmVjEventCode,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&code.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<MmVjEventCode, D::Error> {
        let s = String::deserialize(deserializer)?;
        s.parse::<MmVjEventCode>()
            .map_err(|_| de::Error::custom(format!("Invalid code string: {}", s)))
    }
}

//-------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Interval<T: Debug + Num> {
    pub(crate) from: T,
    pub(crate) to: T,
}

impl<T: FromPrimitive + Num + PartialOrd + Copy + Debug> PartialOrd for Interval<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        //self.span().partial_cmp(&other.span())
        match self.span().partial_cmp(&other.span()) {
            Some(Ordering::Equal) => self.from.partial_cmp(&other.from),
            other => other,
        }
    }
}

impl<T: Add<Output = T> + Div<Output = T> + FromPrimitive + Num + PartialOrd + Copy + Debug>
    Interval<T>
{
    pub(crate) fn new(from: T, to: T) -> Self {
        if from > to {
            return Self { from: to, to: from };
        }
        Self { from: from, to: to }
    }

    pub(crate) fn contains_inclusive(&self, value: T) -> bool {
        value >= self.from && value <= self.to
    }

    #[allow(dead_code)]
    pub(crate) fn contains_exclusive(&self, value: T) -> bool {
        value >= self.from && value < self.to
    }

    pub(crate) fn clamp(&self, value: T) -> T {
        if value < self.from {
            self.from
        } else if value > self.to {
            self.to
        } else {
            value
        }
    }

    pub(crate) fn clamp_and_invert(&self, value: T) -> T {
        self.try_invert_value(self.clamp(value)).unwrap_or(value)
    }

    pub(crate) fn try_invert_value(&self, value: T) -> Result<T> {
        if self.contains_inclusive(value) {
            Ok((self.to + self.from) - value)
        } else {
            let err = format!(
                "Value {:?} is out of range {:?}, can't invert value within this range.",
                value, self
            );
            log::warn!("{}", err);
            bail!(err)
        }
    }

    pub(crate) fn midpoint(&self) -> T {
        (self.from + self.to) / T::from_u8(2).expect("You got the towel.")
    }

    pub(crate) fn span(&self) -> T {
        self.to - self.from
    }
}

impl<T: FromPrimitive + Float + Debug> Interval<T> {
    pub(crate) fn denormalize(&self, normalized: T) -> T {
        self.from + (self.span() * normalized)
    }

    pub(crate) fn normalize(&self, value: T) -> T {
        let span = self.span();
        if span == T::zero() {
            return T::zero();
        }
        (value - self.from) / span
    }

    pub(crate) fn map_from(&self, value: T, input_range: &Interval<T>) -> T {
        let normalized = input_range.normalize(value);
        self.denormalize(normalized)
    }
}

impl<T: Num + NumCast + Copy + Debug> Interval<T> {
    pub(crate) fn cast<U: Num + NumCast + Copy + Debug>(&self) -> Option<Interval<U>> {
        Some(Interval {
            from: U::from(self.from)?,
            to: U::from(self.to)?,
        })
    }
}

// Custom serde implementation to serialize/deserialize as tuple [from, to]
impl<T: Num + Debug + Serialize + Copy> Serialize for Interval<T> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.from, self.to).serialize(serializer)
    }
}

impl<'de, T: FromPrimitive + Num + Debug + PartialOrd + Deserialize<'de> + Copy> Deserialize<'de>
    for Interval<T>
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (from, to) = <(T, T)>::deserialize(deserializer)?;
        Ok(Interval::new(from, to))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EPSILON: f32 = 0.0001;
    fn f32_approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < EPSILON
    }

    #[test]
    fn test_construction_and_equality() {
        let r_f32 = Interval::new(10.0_f32, 20.0);
        let r_i32 = Interval::new(10_i32, 20);
        let r_f64 = Interval::new(10.0_f64, 20.0);
        assert_eq!(r_f32.from, 10.0);
        assert_eq!(r_f32.to, 20.0);
        assert_eq!(
            r_f32,
            r_i32.cast::<f32>().unwrap(),
            "i32/u32 construction failed to match f32 construction"
        );
        assert_eq!(
            r_f32,
            r_f64.cast::<f32>().unwrap(),
            "f64 construction failed to match f32 construction"
        );
        assert_ne!(
            r_f32,
            Interval::new(10.0_f32, 20.1),
            "Ranges with different 'to' fields should be unequal"
        );
    }

    #[test]
    fn test_size_based_ordering() {
        let r_small = Interval::new(10, 15);
        let r_medium_a = Interval::new(-10, 10);
        let r_medium_b = Interval::new(0, 20);
        let r_large = Interval::new(0, 30);
        assert_eq!(
            Interval::new(-10, 100),
            Interval::new(100, -10),
            "Ranges with r1.from == r2.to and r1.to == r2.from should compare as equal
            (because spans are equal and 'from' and 'to' members are set on constructions 
            such that from < to)."
        );
        assert!(
            r_small < r_medium_a,
            "Small range should be less than medium range"
        );
        assert!(
            r_large > r_medium_a,
            "Large range should be greater than medium range"
        );
        assert_eq!(
            r_medium_a.to - r_medium_a.from,
            r_medium_b.to - r_medium_b.from,
            "Sizes should be equal for tie-breaker test"
        );
        assert!(
            r_medium_a < r_medium_b,
            "Ranges of equal size should be ordered by 'from' value (-10 < 0)"
        );
        assert!(
            r_medium_a <= r_medium_b,
            "Ranges of equal size should be ordered by 'from' value (-10 <= 0)"
        );
        assert!(
            r_medium_b > r_medium_a,
            "Ranges of equal size should be ordered by 'from' value (0 > -10)"
        );
    }

    #[test]

    fn test_containment() {
        let r = Interval::new(10.0, 20.0);
        assert!(
            r.contains_inclusive(10.0),
            "Inclusive should contain 'from'"
        );
        assert!(
            r.contains_inclusive(15.0),
            "Inclusive should contain middle value"
        );
        assert!(r.contains_inclusive(20.0), "Inclusive should contain 'to'");
        assert!(
            !r.contains_inclusive(9.9),
            "Inclusive should not contain value below 'from'"
        );
        assert!(
            !r.contains_inclusive(20.1),
            "Inclusive should not contain value above 'to'"
        );
        assert!(
            r.contains_exclusive(10.0),
            "Exclusive should contain 'from'"
        );
        assert!(
            r.contains_exclusive(19.99),
            "Exclusive should contain value just below 'to'"
        );
        assert!(
            !r.contains_exclusive(9.9),
            "Exclusive should not contain value below 'from'"
        );
        assert!(
            !r.contains_exclusive(20.0),
            "Exclusive should not contain 'to'"
        );
        assert!(
            !r.contains_exclusive(20.1),
            "Exclusive should not contain value above 'to'"
        );
    }

    #[test]
    fn test_inversion() {
        let r = Interval::new(-10.0, 15.0);
        assert_eq!(
            r.try_invert_value(-1.0).unwrap(),
            6.0,
            "Inverting value inside range {r:?} failed."
        );
        assert!(
            r.try_invert_value(-11.0).is_err(),
            "Inverting value our of range {r:?} must return error."
        );
        let r = Interval::new(10.0, 20.0);
        assert_eq!(
            r.try_invert_value(15.0).unwrap(),
            15.0,
            "Inverting value inside range {r:?} failed"
        );
        assert_eq!(
            r.try_invert_value(14.0).unwrap(),
            16.0,
            "Inverting value inside range {r:?} failed"
        );
        let r = Interval::new(-10.0, -20.0);
        assert_eq!(
            r.from, -20.0,
            "Range {r:?} should get sorted on construction such that from < to."
        );
        assert_eq!(
            r.to, -10.0,
            "Range {r:?} should get sorted on construction such that from < to."
        );
        assert_eq!(
            r.try_invert_value(-15.0).unwrap(),
            -15.0,
            "Inverting value inside range {r:?} failed"
        );
        assert_eq!(
            r.try_invert_value(-14.0).unwrap(),
            -16.0,
            "Inverting value inside range {r:?} failed"
        );
    }

    #[test]
    fn test_clamping() {
        let r = Interval::new(-10.0, 20.0);
        assert_eq!(r.clamp(5.0), 5.0, "Clamping value inside range failed");
        assert_eq!(r.clamp(-50.0), -10.0, "Clamping value below 'from' failed");
        assert_eq!(r.clamp(30.0), 20.0, "Clamping value above 'to' failed");
        assert_eq!(r.clamp(-10.0), -10.0, "Clamping value at 'from' failed");
        assert_eq!(r.clamp(20.0), 20.0, "Clamping value at 'to' failed");
    }

    #[test]
    fn test_normalization() {
        let input_range = Interval::new(0.0, 100.0);
        let output_range = Interval::new(-10.0, 20.0);

        let result_min = output_range.map_from(0.0, &input_range);
        assert!(
            f32_approx_eq(result_min, -10.0),
            "Min mapping failed: Got {}",
            result_min
        );

        let result_mid = output_range.map_from(50.0, &input_range);
        assert!(
            f32_approx_eq(result_mid, 5.0),
            "Midpoint mapping failed: Got {}",
            result_mid
        );

        let result_max = output_range.map_from(100.0, &input_range);
        assert!(
            f32_approx_eq(result_max, 20.0),
            "Max mapping failed: Got {}",
            result_max
        );

        let result_outside = output_range.map_from(150.0, &input_range);
        assert!(
            f32_approx_eq(result_outside, 35.0),
            "Extrapolation mapping failed: Got {}",
            result_outside
        );

        let zero_span_input = Interval::new(5.0, 5.0);
        let zero_result = output_range.map_from(5.0, &zero_span_input);
        assert!(
            f32_approx_eq(zero_result, -10.0),
            "Zero span input should return output 'from': Got {}",
            zero_result
        );
    }
}
