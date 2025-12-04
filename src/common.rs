use anyhow::{bail, Result};
use num_traits::{Float, FromPrimitive, Num, NumCast, ToPrimitive};
use serde::{Deserialize, Serialize};
use std::fmt::Display;
use std::sync::LazyLock;
use std::{cmp::Ordering, fmt::Debug};

use serde::{de, Deserializer, Serializer};
use std::convert::*;
use strum_macros::{Display, EnumString};

pub(crate) static SYMM_UNIT_INTERVAL: LazyLock<NumInterval<f32>> =
    LazyLock::new(|| NumInterval::new(-1.0, 1.0));

pub(crate) static UNIT_INTERVAL: LazyLock<NumInterval<f32>> =
    LazyLock::new(|| NumInterval::new(0.0, 1.0));

//--------------------------------------------------------------
define_control_types! {
    absolute {
        AbsX => ABS_X,
        AbsY => ABS_Y,
        AbsZ => ABS_Z,
        AbsRX => ABS_RX,
        AbsRY => ABS_RY,
        AbsRZ => ABS_RZ,
        AbsThrottle => ABS_THROTTLE,
        AbsRudder => ABS_RUDDER,
        AbsWheel => ABS_WHEEL,
        AbsGas => ABS_GAS,
        AbsBrake => ABS_BRAKE,
        AbsHat0X => ABS_HAT0X,
        AbsHat0Y => ABS_HAT0Y,
        AbsHat1X => ABS_HAT1X,
        AbsHat1Y => ABS_HAT1Y,
        AbsHat2X => ABS_HAT2X,
        AbsHat2Y => ABS_HAT2Y,
        AbsHat3X => ABS_HAT3X,
        AbsHat3Y => ABS_HAT3Y,
        AbsPressure => ABS_PRESSURE,
        AbsDistance => ABS_DISTANCE,
        AbsTiltX => ABS_TILT_X,
        AbsTiltY => ABS_TILT_Y,
        AbsToolWidth => ABS_TOOL_WIDTH,
        AbsVolume => ABS_VOLUME,
        AbsMisc => ABS_MISC,
        AbsMtSlot => ABS_MT_SLOT, "MT slot being modified",
        AbsMtTouchMajor => ABS_MT_TOUCH_MAJOR, "Major axis of touching ellipse",
        AbsMtTouchMinor => ABS_MT_TOUCH_MINOR, "Minor axis (omit if circular)",
        AbsMtWidthMajor => ABS_MT_WIDTH_MAJOR, "Major axis of approaching ellipse",
        AbsMtWidthMinor => ABS_MT_WIDTH_MINOR, "Minor axis (omit if circular)",
        AbsMtOrientation => ABS_MT_ORIENTATION, "Ellipse orientation",
        AbsMtPositionX => ABS_MT_POSITION_X, "Center X touch position",
        AbsMtPositionY => ABS_MT_POSITION_Y, "Center Y touch position",
        AbsMtToolType => ABS_MT_TOOL_TYPE, "Type of touching device",
        AbsMtBlobId => ABS_MT_BLOB_ID, "Group a set of packets as a blob",
        AbsMtTrackingId => ABS_MT_TRACKING_ID, "Unique ID of the initiated contact",
        AbsMtPressure => ABS_MT_PRESSURE, "Pressure on contact area",
        AbsMtDistance => ABS_MT_DISTANCE, "Contact over distance",
        AbsMtToolX => ABS_MT_TOOL_X, "Center X tool position",
        AbsMtToolY => ABS_MT_TOOL_Y, "Center Y tool position",
    }
    relative {
        RelX => REL_X,
        RelY => REL_Y,
        RelZ => REL_Z,
        RelRX => REL_RX,
        RelRY => REL_RY,
        RelRZ => REL_RZ,
        RelHwheel => REL_HWHEEL,
        RelDial => REL_DIAL,
        RelWheel => REL_WHEEL,
        RelMisc => REL_MISC,
        RelWheelHiRes => REL_WHEEL_HI_RES,
        RelHWheelHiRes => REL_HWHEEL_HI_RES,
    }
    button {
        BtnSouth => BTN_SOUTH,
        BtnEast => BTN_EAST,
        BtnWest => BTN_WEST,
        BtnNorth => BTN_NORTH,
        BtnStart => BTN_START,
        BtnSelect => BTN_SELECT,
        BtnLeft => BTN_LEFT,
        BtnRight => BTN_RIGHT,
        BtnMiddle => BTN_MIDDLE,
        BtnSide => BTN_SIDE,
        BtnExtra => BTN_EXTRA,
    }
    midi {
        PitchWheel,
        ModulationWheel,
        Note,
        ControlChange,
        ProgramChange,
    }
}

//-------------------------------------------------------------
pub trait MakeUnsigned {
    type Type: Num + PartialOrd;
}

macro_rules! impl_make_unsigned {
    ($($t:ty => $w:ty),* $(,)?) => {
        $(impl MakeUnsigned for $t {
            type Type = $w ;
        })*
    };
}

impl_make_unsigned! {
    i8 => u8,
    i16 => u16,
    i32 => u32,
    i64 => u64,
    i128 => u128,
    u8 => u8,
    u16 => u16,
    u32 => u32,
    u64 => u64,
    u128 => u128,
}

//-------------------------------------------------------------
pub trait Widen {
    type Type: Num + PartialOrd;
}

macro_rules! impl_wider_num {
    ($($t:ty => $w:ty),* $(,)?) => {
        $(impl Widen for $t {
            type Type = $w ;
        })*
    };
}

impl_wider_num! {
    i8 => i16,
    i16 => i32,
    i32 => i64,
    i64 => i128,
    i128 => i128,
    u8 => u16,
    u16 => u32,
    u32 => u64,
    u64 => u128,
    u128 => u128,
    f32 => f64,
    f64 => f64,
}

//-------------------------------------------------------------
#[derive(Debug, Clone, Copy)]
pub(crate) struct NumInterval<T> {
    pub(crate) from: T,
    pub(crate) to: T,
}

impl<T> Eq for NumInterval<T>
where
    T: Num,
    T: PartialOrd,
    T: Widen,
    T: FromPrimitive,
    T: Copy,
    T: Debug,
    <T as Widen>::Type: ToPrimitive,
    <T as Widen>::Type: std::convert::From<T>,
{
}

impl<T> PartialEq for NumInterval<T>
where
    T: Num,
    T: PartialOrd,
    T: Widen,
    T: FromPrimitive,
    T: Copy,
    T: Debug,
    <T as Widen>::Type: ToPrimitive,
    <T as Widen>::Type: std::convert::From<T>,
{
    fn eq(&self, other: &Self) -> bool {
        self.span_w() == other.span_w()
    }
}

impl<T: Widen> PartialOrd for NumInterval<T>
where
    T: Num,
    T: PartialOrd,
    T: Widen,
    T: FromPrimitive,
    T: Copy,
    T: Debug,
    <T as Widen>::Type: ToPrimitive,
    <T as Widen>::Type: std::convert::From<T>,
{
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.span_w().partial_cmp(&other.span_w()) {
            Some(Ordering::Equal) => self.from.partial_cmp(&other.from),
            other => other,
        }
    }
}

impl<T> NumInterval<T>
where
    T: Num,
    T: PartialOrd,
    T: Widen,
    T: FromPrimitive,
    T: Copy,
    T: Debug,
    <T as Widen>::Type: ToPrimitive,
    <T as Widen>::Type: std::convert::From<T>,
{
    pub(crate) fn new(from: T, to: T) -> Self {
        if from > to {
            return Self { from: to, to: from };
        }
        Self { from, to }
    }

    pub(crate) fn denormalize_from_unit<FloatT>(
        &self,
        normalized: FloatT,
        allow_extrapolative: bool,
    ) -> T
    where
        T: NumCast,
        FloatT: Float,
        FloatT: Display,
    {
        if !allow_extrapolative && (normalized < FloatT::zero() && normalized > FloatT::one()) {
            panic!("{} is out of [0,1] range", normalized);
        }
        let magnitude = FloatT::from(self.span_w()).unwrap() * normalized;
        self.from + T::from(magnitude).expect("Magnitude {magnitude} must fit.")
    }

    pub(crate) fn normalize_to_unit(&self, value: T) -> f32 {
        let span = self.span_w();
        if span == T::zero().into() {
            return 0.0;
        }
        let diff_w = <T as Widen>::Type::from(value) - <T as Widen>::Type::from(self.from);
        <f32 as num_traits::NumCast>::from(diff_w)
            .expect("Expected to convert {diff_wider:?} to f32")
            / <f32 as num_traits::NumCast>::from(span).expect("Expected to convert {span:?} to f32")
    }

    pub(crate) fn map_from<FromT>(
        &self,
        value: FromT,
        input_range: &NumInterval<FromT>,
        allow_extrapolative: bool,
    ) -> T
    where
        T: NumCast,
        FromT: Widen,
        FromT: Num,
        FromT: Copy,
        FromT: FromPrimitive,
        FromT: PartialOrd,
        FromT: Debug,
        <FromT as Widen>::Type: From<FromT>,
        <FromT as Widen>::Type: ToPrimitive,
    {
        let normalized = input_range.normalize_to_unit(value);
        self.denormalize_from_unit(normalized, allow_extrapolative)
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

    #[allow(dead_code)]
    pub(crate) fn midpoint(&self) -> T {
        (self.from + self.to) / T::from_u8(2).expect("You got the towel.")
    }

    pub(crate) fn span(&self) -> T
    where
        T: NumCast,
        <T as Widen>::Type: From<T>,
    {
        debug_assert!(
            T::from(self.span_w()).is_some(),
            "Span of {self:?} won't fit into T"
        );
        self.to - self.from
    }

    pub(crate) fn span_w(&self) -> <T as Widen>::Type
    where
        <T as Widen>::Type: From<T>,
    {
        let wider_to: <T as Widen>::Type = self.to.into();
        let wider_from: <T as Widen>::Type = self.from.into();
        wider_to - wider_from
    }

    pub(crate) fn cast<OtherT>(&self) -> Option<NumInterval<OtherT>>
    where
        T: ToPrimitive,
        OtherT: NumCast,
        OtherT: Widen,
    {
        Some(NumInterval {
            from: OtherT::from(self.from)?,
            to: OtherT::from(self.to)?,
        })
    }
}

// Custom serde implementation to serialize/deserialize as tuple [from, to]
impl<T> Serialize for NumInterval<T>
where
    T: Widen,
    T: Copy,
    T: Serialize,
{
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        (self.from, self.to).serialize(serializer)
    }
}

impl<'de, T> Deserialize<'de> for NumInterval<T>
where
    T: Num,
    T: PartialOrd,
    T: Widen,
    T: FromPrimitive,
    T: Copy,
    T: Debug,
    T: Deserialize<'de>,
    <T as Widen>::Type: ToPrimitive,
    <T as Widen>::Type: From<T>,
{
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let (from, to) = <(T, T)>::deserialize(deserializer)?;
        Ok(NumInterval::new(from, to))
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
        let r_f32 = NumInterval::new(10.0_f32, 20.0);
        let r_i32 = NumInterval::new(10_i32, 20);
        let r_f64 = NumInterval::new(10.0_f64, 20.0);
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
            NumInterval::new(10.0_f32, 20.1),
            "Ranges with different 'to' fields should be unequal"
        );
    }

    #[test]
    fn test_size_based_ordering() {
        let r_small = NumInterval::new(10, 15);
        let r_medium_a = NumInterval::new(-10, 10);
        let r_medium_b = NumInterval::new(0, 20);
        let r_large = NumInterval::new(0, 30);
        assert_eq!(
            NumInterval::new(-10, 100),
            NumInterval::new(100, -10),
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
        let r = NumInterval::new(10.0, 20.0);
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
        let r = NumInterval::new(-10.0, 15.0);
        assert_eq!(
            r.try_invert_value(-1.0).unwrap(),
            6.0,
            "Inverting value inside range {r:?} failed."
        );
        assert!(
            r.try_invert_value(-11.0).is_err(),
            "Inverting value our of range {r:?} must return error."
        );
        let r = NumInterval::new(10.0, 20.0);
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
        let r = NumInterval::new(-10.0, -20.0);
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
        let r = NumInterval::new(-10.0, 20.0);
        assert_eq!(r.clamp(5.0), 5.0, "Clamping value inside range failed");
        assert_eq!(r.clamp(-50.0), -10.0, "Clamping value below 'from' failed");
        assert_eq!(r.clamp(30.0), 20.0, "Clamping value above 'to' failed");
        assert_eq!(r.clamp(-10.0), -10.0, "Clamping value at 'from' failed");
        assert_eq!(r.clamp(20.0), 20.0, "Clamping value at 'to' failed");
    }

    #[test]
    fn test_normalization() {
        let input_range = NumInterval::<f32>::new(0.0, 100.0);
        let output_range = NumInterval::<f32>::new(-10.0, 20.0);

        let result_min = output_range.map_from(0.0, &input_range, false);
        assert!(
            f32_approx_eq(result_min, -10.0),
            "Min mapping failed: Got {}",
            result_min
        );

        let result_mid = output_range.map_from(50.0, &input_range, false);
        assert!(
            f32_approx_eq(result_mid, 5.0),
            "Midpoint mapping failed: Got {}",
            result_mid
        );

        let result_max = output_range.map_from(100.0, &input_range, false);
        assert!(
            f32_approx_eq(result_max, 20.0),
            "Max mapping failed: Got {}",
            result_max
        );

        let result_outside = output_range.map_from(150.0, &input_range, true);
        assert!(
            f32_approx_eq(result_outside, 35.0),
            "Extrapolation mapping failed: Got {}",
            result_outside
        );

        let zero_span_input = NumInterval::new(5.0, 5.0);
        let zero_result = output_range.map_from(5.0, &zero_span_input, false);
        assert!(
            f32_approx_eq(zero_result, -10.0),
            "Zero span input should return output 'from': Got {}",
            zero_result
        );
    }

    //--------------------------------------------------------

    #[test]
    fn test_absolute_iterator() {
        let abs_controls: Vec<ControlType> = ControlType::iter_absolute().collect();

        // Check we got all absolute controls
        assert!(!abs_controls.is_empty());

        // Verify all are absolute
        for control in &abs_controls {
            assert!(control.is_absolute(), "{:?} should be absolute", control);
            assert!(
                !control.is_relative(),
                "{:?} should not be relative",
                control
            );
            assert!(!control.is_button(), "{:?} should not be button", control);
        }

        // Check specific controls are present
        assert!(abs_controls.contains(&ControlType::AbsX));
        assert!(abs_controls.contains(&ControlType::AbsY));
        assert!(abs_controls.contains(&ControlType::AbsZ));
        assert!(abs_controls.contains(&ControlType::AbsBrake));
        assert!(abs_controls.contains(&ControlType::AbsGas));
    }

    #[test]
    fn test_relative_iterator() {
        let rel_controls: Vec<ControlType> = ControlType::iter_relative().collect();

        // Check we got all relative controls
        assert!(!rel_controls.is_empty());

        // Verify all are relative
        for control in &rel_controls {
            assert!(control.is_relative(), "{:?} should be relative", control);
            assert!(
                !control.is_absolute(),
                "{:?} should not be absolute",
                control
            );
            assert!(!control.is_button(), "{:?} should not be button", control);
        }

        // Check specific controls are present
        assert!(rel_controls.contains(&ControlType::RelX));
        assert!(rel_controls.contains(&ControlType::RelY));
        assert!(rel_controls.contains(&ControlType::RelWheel));
        assert!(rel_controls.contains(&ControlType::RelHwheel));
    }

    #[test]
    fn test_button_iterator() {
        let btn_controls: Vec<ControlType> = ControlType::iter_button().collect();

        // Check we got all button controls
        assert!(!btn_controls.is_empty());

        // Verify all are buttons
        for control in &btn_controls {
            assert!(control.is_button(), "{:?} should be button", control);
            assert!(
                !control.is_relative(),
                "{:?} should not be relative",
                control
            );
        }

        // Check specific controls are present
        assert!(btn_controls.contains(&ControlType::BtnSouth));
        assert!(btn_controls.contains(&ControlType::BtnEast));
        assert!(btn_controls.contains(&ControlType::BtnWest));
        assert!(btn_controls.contains(&ControlType::BtnNorth));
        assert!(btn_controls.contains(&ControlType::BtnLeft));
        assert!(btn_controls.contains(&ControlType::BtnRight));
    }

    #[test]
    fn test_iterator_exact_size() {
        let abs_iter = ControlType::iter_absolute();
        let abs_count = abs_iter.len();
        assert_eq!(abs_count, abs_iter.count());

        let rel_iter = ControlType::iter_relative();
        let rel_count = rel_iter.len();
        assert_eq!(rel_count, rel_iter.count());

        let btn_iter = ControlType::iter_button();
        let btn_count = btn_iter.len();
        assert_eq!(btn_count, btn_iter.count());
    }

    #[test]
    fn test_iterator_next_behavior() {
        let mut iter = ControlType::iter_button();

        // Get first item
        let first = iter.next();
        assert!(first.is_some());

        // Get second item
        let second = iter.next();
        assert!(second.is_some());

        // First and second should be different
        assert_ne!(first, second);

        // Should eventually return None
        let mut count = 2;
        while iter.next().is_some() {
            count += 1;
            if count > 100 {
                panic!("Iterator didn't terminate");
            }
        }

        // After None, should keep returning None
        assert!(iter.next().is_none());
        assert!(iter.next().is_none());
    }

    #[test]
    fn test_from_evdev_absolute() {
        use evdev::{AbsoluteAxisCode, EventType, InputEvent};

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_X.0, 100);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::AbsX);

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_Y.0, 200);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::AbsY);

        let event = InputEvent::new(EventType::ABSOLUTE.0, AbsoluteAxisCode::ABS_BRAKE.0, 50);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::AbsBrake);
    }

    #[test]
    fn test_from_evdev_relative() {
        use evdev::{EventType, InputEvent, RelativeAxisCode};

        let event = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_X.0, 10);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::RelX);

        let event = InputEvent::new(EventType::RELATIVE.0, RelativeAxisCode::REL_WHEEL.0, -1);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::RelWheel);
    }

    #[test]
    fn test_from_evdev_button() {
        use evdev::{EventType, InputEvent, KeyCode};

        let event = InputEvent::new(EventType::KEY.0, KeyCode::BTN_SOUTH.0, 1);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::BtnSouth);

        let event = InputEvent::new(EventType::KEY.0, KeyCode::BTN_LEFT.0, 1);
        let control: ControlType = event.into();
        assert_eq!(control, ControlType::BtnLeft);
    }

    #[test]
    fn test_to_u16_absolute() {
        use evdev::AbsoluteAxisCode;

        let code: u16 = ControlType::AbsX.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_X.0);

        let code: u16 = ControlType::AbsY.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_Y.0);

        let code: u16 = ControlType::AbsGas.into();
        assert_eq!(code, AbsoluteAxisCode::ABS_GAS.0);
    }

    #[test]
    fn test_to_u16_relative() {
        use evdev::RelativeAxisCode;

        let code: u16 = ControlType::RelX.into();
        assert_eq!(code, RelativeAxisCode::REL_X.0);

        let code: u16 = ControlType::RelWheel.into();
        assert_eq!(code, RelativeAxisCode::REL_WHEEL.0);
    }

    #[test]
    fn test_to_u16_button() {
        use evdev::KeyCode;

        let code: u16 = ControlType::BtnSouth.into();
        assert_eq!(code, KeyCode::BTN_SOUTH.0);

        let code: u16 = ControlType::BtnLeft.into();
        assert_eq!(code, KeyCode::BTN_LEFT.0);
    }

    #[test]
    fn test_roundtrip_conversion() {
        use evdev::{EventType, InputEvent};

        // Test absolute
        let original = ControlType::AbsX;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::ABSOLUTE.0, code, 0);
        let converted: ControlType = event.into();
        assert_eq!(original, converted);

        // Test relative
        let original = ControlType::RelWheel;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::RELATIVE.0, code, 0);
        let converted: ControlType = event.into();
        assert_eq!(original, converted);

        // Test button
        let original = ControlType::BtnSouth;
        let code: u16 = original.into();
        let event = InputEvent::new(EventType::KEY.0, code, 0);
        let converted: ControlType = event.into();
        assert_eq!(original, converted);
    }

    #[test]
    fn test_helper_methods() {
        // Test absolute
        assert!(ControlType::AbsX.is_absolute());
        assert!(!ControlType::AbsX.is_relative());
        assert!(!ControlType::AbsX.is_button());

        // Test relative
        assert!(!ControlType::RelX.is_absolute());
        assert!(ControlType::RelX.is_relative());
        assert!(!ControlType::RelX.is_button());

        // Test button
        assert!(!ControlType::BtnSouth.is_absolute());
        assert!(!ControlType::BtnSouth.is_relative());
        assert!(ControlType::BtnSouth.is_button());

        // Test unhandled
        assert!(!ControlType::Unhandled.is_absolute());
        assert!(!ControlType::Unhandled.is_relative());
        assert!(!ControlType::Unhandled.is_button());
        assert!(ControlType::Unhandled.is_unhandled());
    }

    #[test]
    fn test_string_parsing() {
        use std::str::FromStr;

        // Test parsing absolute
        assert_eq!(ControlType::from_str("ABS_X").unwrap(), ControlType::AbsX);
        assert_eq!(
            ControlType::from_str("ABS_BRAKE").unwrap(),
            ControlType::AbsBrake
        );

        // Test parsing relative
        assert_eq!(ControlType::from_str("REL_X").unwrap(), ControlType::RelX);
        assert_eq!(
            ControlType::from_str("REL_WHEEL").unwrap(),
            ControlType::RelWheel
        );

        // Test parsing button
        assert_eq!(
            ControlType::from_str("BTN_SOUTH").unwrap(),
            ControlType::BtnSouth
        );
        assert_eq!(
            ControlType::from_str("BTN_LEFT").unwrap(),
            ControlType::BtnLeft
        );

        // Test invalid string
        assert!(ControlType::from_str("INVALID").is_err());
    }

    #[test]
    fn test_to_string() {
        assert_eq!(ControlType::AbsX.to_string(), "ABS_X");
        assert_eq!(ControlType::RelWheel.to_string(), "REL_WHEEL");
        assert_eq!(ControlType::BtnSouth.to_string(), "BTN_SOUTH");
        assert_eq!(ControlType::Unhandled.to_string(), "UNHANDLED");
    }

    #[test]
    fn test_serialization() {
        use serde_yaml;

        let control = ControlType::AbsX;
        let serialized = serde_yaml::to_string(&control).unwrap();
        assert_eq!(serialized, "ABS_X\n");

        let control = ControlType::BtnSouth;
        let serialized = serde_yaml::to_string(&control).unwrap();
        assert_eq!(serialized, "BTN_SOUTH\n");
    }

    #[test]
    fn test_deserialization() {
        use serde_yaml;

        let yaml = "ABS_X\n";
        let control: ControlType = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(control, ControlType::AbsX);

        let yaml = "BTN_SOUTH\n";
        let control: ControlType = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(control, ControlType::BtnSouth);

        let yaml = "INVALID\n";
        let result: Result<ControlType, _> = serde_yaml::from_str(yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_clone_and_copy() {
        let original = ControlType::AbsX;
        let cloned = original.clone();
        let copied = original;

        assert_eq!(original, cloned);
        assert_eq!(original, copied);
    }

    #[test]
    fn test_default() {
        let default = ControlType::default();
        assert_eq!(default, ControlType::Unhandled);
    }

    #[test]
    fn test_multitouch_controls() {
        // Test that multitouch controls are properly included
        let abs_controls: Vec<ControlType> = ControlType::iter_absolute().collect();

        assert!(abs_controls.contains(&ControlType::AbsMtSlot));
        assert!(abs_controls.contains(&ControlType::AbsMtTouchMajor));
        assert!(abs_controls.contains(&ControlType::AbsMtPositionX));
        assert!(abs_controls.contains(&ControlType::AbsMtPositionY));
    }

    #[test]
    fn test_iterator_clone() {
        // for b in ControlType::iter_button() {
        //     println!("{b:?} {b}");
        // }

        // for a in ControlType::iter_absolute() {
        //     println!("{a:?} {a}");
        // }

        let iter1 = ControlType::iter_button();
        let mut iter2 = iter1.clone();

        // Advance iter2
        iter2.next();
        iter2.next();

        // iter1 should still be at the beginning
        let count1 = iter1.count();
        let count2 = iter2.count();

        assert_eq!(count1, count2 + 2);
    }

    #[test]
    fn test_no_duplicates_in_iterators() {
        use std::collections::HashSet;

        // Check absolute controls
        let abs_controls: Vec<ControlType> = ControlType::iter_absolute().collect();
        let abs_set: HashSet<_> = abs_controls.iter().collect();
        assert_eq!(
            abs_controls.len(),
            abs_set.len(),
            "Duplicate in absolute iterator"
        );

        // Check relative controls
        let rel_controls: Vec<ControlType> = ControlType::iter_relative().collect();
        let rel_set: HashSet<_> = rel_controls.iter().collect();
        assert_eq!(
            rel_controls.len(),
            rel_set.len(),
            "Duplicate in relative iterator"
        );

        // Check button controls
        let btn_controls: Vec<ControlType> = ControlType::iter_button().collect();
        let btn_set: HashSet<_> = btn_controls.iter().collect();
        assert_eq!(
            btn_controls.len(),
            btn_set.len(),
            "Duplicate in button iterator"
        );
    }

    #[test]
    fn test_unhandled_conversion() {
        let code: u16 = ControlType::Unhandled.into();
        assert_eq!(code, 0);

        // MIDI controls should also convert to 0 (unimplemented)
        let code: u16 = ControlType::PitchWheel.into();
        assert_eq!(code, 0);
    }
}
