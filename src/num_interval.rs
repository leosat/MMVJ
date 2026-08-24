use crate::base_num::*;
use anyhow::{Result, bail};
use num_traits::{Bounded, Float, FromPrimitive, Num, NumCast, ToPrimitive, Zero};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::backtrace::Backtrace;
use std::convert::*;
use std::mem::swap;
use std::ops::{Div, Mul, Range, RangeInclusive};
use std::{cmp::Ordering, fmt::Debug};

//-------------------------------------------------------------
pub(crate) const SYMM_UNIT_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(-1.0 as BaseNumT, 1.0 as BaseNumT);
pub(crate) const UNIT_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(0.0 as BaseNumT, 1.0 as BaseNumT);
#[allow(unused)]
pub(crate) const ZERO_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(0.0 as BaseNumT, 0.0 as BaseNumT);
//-------------------------------------------------------------
#[allow(unused)]
pub(crate) const MAX_SPAN_POSITIVE_INTERVAL: NumInterval<BaseNumT> =
    crate::num_interval!(0.0 as BaseNumT, BaseNumT::MAX);
#[allow(unused)]
pub(crate) const MAX_SPAN_NEGATIVE_INTERVAL: NumInterval<BaseNumT> =
    crate::num_interval!(BaseNumT::MIN, 0.0 as BaseNumT);
#[allow(unused)]
pub(crate) const MAX_SPAN_INTERVAL: NumInterval<BaseNumT> = crate::num_interval!(BaseNumT::MIN, BaseNumT::MAX);

pub(crate) trait NumIntervalSpanT {
    type ValueT: NumIntervalValue;
    type SpanT: NumIntervalValue;
    fn as_span_t(&self) -> Self::SpanT;
}

#[macro_export]
macro_rules! num_interval {
    ($from:expr, $value_to:expr) => {{
        let (f, t) = ($from, $value_to);
        if f < t {
            NumInterval { from: f, to: t }
        } else {
            NumInterval { from: t, to: f }
        }
    }};
}

#[derive(Copy, Clone)]
pub(crate) enum OutOfRangePolicy {
    #[allow(unused)]
    Allow,
    Clamp,
    #[allow(unused)]
    WarnAndClamp,
    WarnIfDebugAndClamp,
    _Panic,
}

pub(crate) trait NumIntervalValue:
    std::fmt::Debug
    + std::fmt::Display
    + num_interval_span_impl__::WithSpanTo
    + FromPrimitive
    + ToPrimitive
    + NumIntervalSpanT
    + Num
    + NumCast
    + PartialEq
    + PartialOrd
    + Copy
    + Bounded
    + JsonSchema
    + Default // + ToNumInterval<Self>
{
}

mod num_interval_span_impl__ {
    use super::*;
    pub trait WithSpanTo
    where
        Self: NumIntervalSpanT,
    {
        fn span_impl(&self, to: Self) -> <Self as NumIntervalSpanT>::SpanT;
    }

    macro_rules! impl_span_for_float_or_unsigned {
        ($($value_t:ty),*) => {$(
            impl WithSpanTo for $value_t {
                fn span_impl(&self, to: Self) -> <Self as NumIntervalSpanT>::SpanT {
                    to - self
                }
            }
        )*}
    }

    macro_rules! impl_span_for_signed_ints {
        ($($value_t:ty),*) => {$(
            impl WithSpanTo for $value_t {
                fn span_impl(&self, to: Self) -> <Self as NumIntervalSpanT>::SpanT
                where Self: num_traits::Signed {
                    to.wrapping_sub(*self).as_span_t()
                }
            }
        )*}
    }

    impl_span_for_float_or_unsigned!(f32, f64, u8, u16, u32, u64, u128, usize);
    impl_span_for_signed_ints!(i8, i16, i32, i64, i128, isize);
}

trait WithSpanTo__: num_interval_span_impl__::WithSpanTo
where
    Self: NumIntervalValue,
{
    fn span_to(&self, to: Self) -> <Self as NumIntervalSpanT>::SpanT {
        self.span_impl(to)
    }
}

macro_rules! impl_num_interval_value_and_span_t_for {
    ( $(value_t: $value_t:ty, span_t: $span_t:ty),* $(,)? ) => {
        $(
            impl NumIntervalSpanT for $value_t {
                type SpanT = $span_t ;
                type ValueT = $value_t;
                fn as_span_t(&self) -> $span_t {
                    *self as $span_t
                }
            }

            impl NumIntervalValue for $value_t {}
        )*
    };
}

impl_num_interval_value_and_span_t_for! {
    value_t: i8,    span_t:  u8,
    value_t: i16,   span_t:  u16,
    value_t: i32,   span_t:  u32,
    value_t: i64,   span_t:  u64,
    value_t: isize, span_t:  usize,
    value_t: i128,  span_t:  u128,
    //----------------------------
    value_t: u8,    span_t:  u8,
    value_t: u16,   span_t:  u16,
    value_t: u32,   span_t:  u32,
    value_t: u64,   span_t:  u64,
    value_t: usize, span_t:  usize,
    value_t: u128,  span_t:  u128,
    //----------------------------
    value_t: f32,   span_t:  f32,
    value_t: f64,   span_t:  f64,
}

impl<T: NumIntervalValue> WithSpanTo__ for T {}
// impl<T: NumIntervalValue> ToNumInterval<T> for T {}

//-------------------------------------------------------------
/*TODO: make const*/
pub(crate) fn num_interval_from_type_of<T: NumIntervalValue /* + ToNumInterval<T> */>(_: T) -> NumInterval<T> {
    NumInterval::<T> {
        from: T::min_value(),
        to: T::max_value(),
    }
}

// pub(crate) trait ToNumInterval<T: NumIntervalValue> {
//     fn to_num_interval() -> NumInterval<T> {
//         NumInterval::new(T::min_value(), T::max_value())
//     }
// }

#[allow(unused)]
pub(crate) fn unit_to_symm_unit<T: NumIntervalValue + Float>(val: T, allow_extrapolative: OutOfRangePolicy) -> T {
    T::from(SYMM_UNIT_INTERVAL.map_from_unit(val, allow_extrapolative)).unwrap()
}

#[allow(unused)]
pub(crate) fn symm_unit_to_unit<T: NumIntervalValue + Float>(val: T, allow_extrapolative: OutOfRangePolicy) -> T {
    T::from(UNIT_INTERVAL.map_from_symm_unit(val, allow_extrapolative)).unwrap()
}

pub(crate) fn from_type_interval_to_symm_unit_clamping<T: NumIntervalValue /*+ ToNumInterval<T>*/>(v: T) -> BaseNumT {
    num_interval_from_type_of(v).map_to_symm_unit(v, OutOfRangePolicy::WarnIfDebugAndClamp)
}

pub(crate) fn from_type_interval_to_unit_clamping<T: NumIntervalValue /*+ ToNumInterval<T>*/>(v: T) -> BaseNumT {
    num_interval_from_type_of(v).map_to_unit(v, OutOfRangePolicy::WarnIfDebugAndClamp)
}

//-------------------------------------------------------------
#[derive(Debug, Clone, Copy, schemars::JsonSchema, Serialize, Deserialize)]
#[serde(from = "(T,T)", into = "(T,T)")]
pub(crate) struct NumInterval<T: NumIntervalValue> {
    pub(crate) from: T,
    pub(crate) to: T,
}

impl<T: NumIntervalValue> Mul for NumInterval<T> {
    type Output = NumInterval<T>;

    fn mul(self, rhs: Self) -> Self::Output {
        NumInterval::new(self.from * rhs.from, self.to * rhs.to).sanitize_and_sort()
    }
}

impl<T: NumIntervalValue> From<NumInterval<T>> for (T, T) {
    fn from(value: NumInterval<T>) -> Self {
        (value.from, value.to)
    }
}

impl<T: NumIntervalValue> From<(T, T)> for NumInterval<T> {
    fn from(value: (T, T)) -> Self {
        Self {
            from: value.0,
            to: value.1,
        }
    }
}

impl<T: NumIntervalValue> From<NumInterval<T>> for RangeInclusive<T> {
    fn from(value: NumInterval<T>) -> Self {
        value.make_range_inclusive()
    }
}

impl<T: NumIntervalValue> From<RangeInclusive<T>> for NumInterval<T> {
    fn from(value: RangeInclusive<T>) -> Self {
        Self {
            from: *value.start(),
            to: *value.end(),
        }
    }
}

impl<T: NumIntervalValue> From<NumInterval<T>> for Range<T> {
    fn from(value: NumInterval<T>) -> Self {
        value.make_range()
    }
}

impl<T: NumIntervalValue> From<Range<T>> for NumInterval<T> {
    fn from(value: Range<T>) -> Self {
        Self {
            from: value.start,
            to: value.end,
        }
    }
}

impl<T: NumIntervalValue> Default for NumInterval<T> {
    fn default() -> Self {
        Self {
            from: T::zero(),
            to: T::one(),
        }
    }
}

impl<T: NumIntervalValue> std::fmt::Display for NumInterval<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "[{}, {}]", self.from, self.to)
    }
}

impl<T: NumIntervalValue> NumInterval<T> {
    pub(crate) fn scale(self, scalar: T) -> Self {
        NumInterval::new(self.from * scalar, self.to * scalar).sanitize_and_sort()
    }

    #[allow(unused)]
    pub(crate) const fn from_range_inclusive(range: &RangeInclusive<T>) -> Self {
        Self {
            from: *range.start(),
            to: *range.end(),
        }
    }

    #[allow(unused)]
    pub(crate) const fn from_range(range: &Range<T>) -> Self {
        Self {
            from: range.start,
            to: range.end,
        }
    }

    #[allow(unused)]
    pub(crate) const fn make_range_inclusive(&self) -> RangeInclusive<T> {
        RangeInclusive::<T>::new(self.from, self.to)
    }

    #[allow(unused)]
    pub(crate) const fn make_range(&self) -> Range<T> {
        Range {
            start: self.from,
            end: self.to,
        }
    }

    #[allow(unused)]
    pub(crate) fn is_zero(&self) -> bool {
        self.from.is_zero() && self.to.is_zero()
    }

    #[allow(unused)]
    pub(crate) fn is_zero_span(&self) -> bool {
        self.from == self.to
    }

    pub(crate) const fn from(&self) -> T {
        self.from
    }

    #[allow(unused)]
    pub(crate) fn try_value_cast<InputT: FromPrimitive + ToPrimitive>(&self, val: InputT) -> Option<T> {
        T::from(val)
    }

    pub(crate) const fn to(&self) -> T {
        self.to
    }
}

impl<T: NumIntervalValue> PartialEq for NumInterval<T> {
    fn eq(&self, other: &Self) -> bool {
        self.span() == other.span() && self.from == other.from
    }
}

impl<T: NumIntervalValue> PartialOrd for NumInterval<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        match self.span().partial_cmp(&other.span()) {
            Some(Ordering::Equal) => self.from.partial_cmp(&other.from),
            other => other,
        }
    }
}

impl<T: NumIntervalValue> NumInterval<T> {
    #[inline]
    pub(crate) fn value_cast_with_out_of_range_policy<InputT: NumIntervalValue>(
        &self,
        value: InputT,
        out_of_range_policy: OutOfRangePolicy,
    ) -> T {
        let value_as_tgt_interval_type = self.try_value_cast(value);
        let is_out_of_bounds = match value_as_tgt_interval_type {
            Some(v_cast) => !self.contains_value_closed(v_cast),
            None => true,
        };

        if branches::unlikely(is_out_of_bounds) {
            match out_of_range_policy {
                OutOfRangePolicy::Allow => match value_as_tgt_interval_type {
                    Some(v) => v,
                    None => {
                        if InputT::from(self.from).unwrap_or(InputT::min_value()) > value {
                            T::min_value()
                        } else {
                            T::max_value()
                        }
                    }
                },
                OutOfRangePolicy::Clamp | OutOfRangePolicy::WarnAndClamp | OutOfRangePolicy::WarnIfDebugAndClamp => {
                    if matches!(
                        out_of_range_policy,
                        OutOfRangePolicy::WarnAndClamp
                        | OutOfRangePolicy::WarnIfDebugAndClamp if cfg!(debug_assertions)
                    ) {
                        log::warn!(
                            "{} is out of {:?} interval is not expected here. {}.\nValue clamped, running...",
                            value,
                            self,
                            Backtrace::capture()
                        );
                    }

                    match value_as_tgt_interval_type {
                        Some(v) => self.clamp(v),
                        None => {
                            if InputT::from(self.from).unwrap_or(InputT::min_value()) > value {
                                T::min_value()
                            } else {
                                T::max_value()
                            }
                        }
                    }
                }
                OutOfRangePolicy::_Panic => {
                    panic!(
                        "{} is out of {:?} interval is not expected here, error in implementation.",
                        value, self
                    )
                }
            }
        } else {
            value_as_tgt_interval_type.unwrap()
        }
    }

    pub(crate) fn sanitize_and_sort(self) -> Self {
        if branches::unlikely(self.from > self.to) {
            let mut tmp = self;
            swap(&mut tmp.from, &mut tmp.to);
            tmp
        } else {
            self
        }
    }

    #[allow(unused)]
    pub(crate) fn sanitize_and_sort_inplace(&mut self) -> bool {
        if branches::unlikely(self.from > self.to) {
            swap(&mut self.from, &mut self.to);
            true
        } else {
            false
        }
    }

    pub(crate) fn new(from: T, to: T) -> Self {
        if branches::unlikely(from > to) {
            return Self { from: to, to: from };
        }
        Self { from, to }
    }

    pub(crate) fn trunc(&self) -> Self
    where
        T: Float,
    {
        Self::new(self.from.trunc(), self.to.trunc())
    }

    pub(crate) fn is_unit(&self) -> bool {
        self.from == T::zero() && self.to == T::one()
    }

    pub(crate) fn is_symm_unit(&self) -> bool {
        self.to == T::one() && (T::from_i32(-1) == Some(self.from))
    }

    pub(crate) fn map_from_unit<InputT: NumIntervalValue + Float>(
        &self,
        mut val_norm: InputT,
        out_of_range_policy: OutOfRangePolicy,
    ) -> T {
        val_norm = UNIT_INTERVAL
            .cast::<InputT>()
            .unwrap_or_else(|| {
                panic!(
                    "{UNIT_INTERVAL} is expected to cast to {}... are zero and one representable in it?...",
                    std::any::type_name::<InputT>()
                )
            })
            .value_cast_with_out_of_range_policy(val_norm, out_of_range_policy);

        if self.is_unit() {
            return T::from(val_norm).unwrap_or_else(|| {
                panic!(
                    "{val_norm} of type {} must fit within target type of {}",
                    std::any::type_name_of_val(&val_norm),
                    std::any::type_name::<T>()
                )
            });
        }

        let span_to_value_remapped = InputT::from(self.span()).unwrap_or_else(|| {
            panic!(
                "{} of type {} is expected to cast to {}",
                self.span(),
                std::any::type_name_of_val(&self.span()),
                std::any::type_name::<InputT>()
            )
        }) * val_norm;

        self.from
            + T::from(span_to_value_remapped).unwrap_or_else(|| {
                if span_to_value_remapped.is_sign_positive() {
                    T::max_value()
                } else {
                    T::min_value()
                }
            })
    }

    pub(crate) fn map_from_symm_unit<InputT: NumIntervalValue + Float>(
        &self,
        mut val_symm_norm: InputT,
        out_of_range_policy: OutOfRangePolicy,
    ) -> T {
        val_symm_norm = SYMM_UNIT_INTERVAL
            .cast::<InputT>()
            .unwrap()
            .value_cast_with_out_of_range_policy(val_symm_norm, out_of_range_policy);

        if self.is_symm_unit() {
            return T::from(val_symm_norm).unwrap_or_else(|| {
                panic!(
                    "{val_symm_norm} of type {} must fit within target type of {}",
                    std::any::type_name_of_val(&val_symm_norm),
                    std::any::type_name::<T>()
                )
            });
        }

        let span_to_value_remapped = InputT::from(self.span()).unwrap_or_else(|| {
            panic!(
                "{} of type {} is expected to cast to {}",
                self.span(),
                std::any::type_name_of_val(&self.span()),
                std::any::type_name::<InputT>()
            )
        }) * ((val_symm_norm + InputT::one()).div(InputT::one() + InputT::one()));

        self.from
            + T::from(span_to_value_remapped).unwrap_or_else(|| {
                if span_to_value_remapped.is_sign_positive() {
                    T::max_value()
                } else {
                    T::min_value()
                }
            })
    }

    #[allow(unused)]
    pub(crate) fn map_to_symm_unit<OutputT: NumIntervalValue + Float>(
        &self,
        value: T,
        out_of_range_policy: OutOfRangePolicy,
    ) -> OutputT {
        OutputT::from(SYMM_UNIT_INTERVAL.map_from_unit(
            self.map_to_unit::<OutputT>(value, out_of_range_policy),
            out_of_range_policy,
        ))
        .unwrap_or_else(|| {
            branches::mark_unlikely();
            panic!("Failed to map from {:?} to {:?}", self, SYMM_UNIT_INTERVAL)
        })
    }

    pub(crate) fn map_to_unit<OutputT: Float + Debug>(
        &self,
        mut value: T,
        out_of_range_policy: OutOfRangePolicy,
    ) -> OutputT {
        value = self.value_cast_with_out_of_range_policy(value, out_of_range_policy);

        let err = || {
            branches::mark_unlikely();
            panic!(
                "Can't map to unit: val: {value}, base type: {} span type: {}, output type {}",
                std::any::type_name::<T>(),
                std::any::type_name::<<T as NumIntervalSpanT>::SpanT>(),
                std::any::type_name::<OutputT>()
            )
        };

        let span = self.span();
        if branches::unlikely(span.is_zero()) {
            OutputT::zero()
        } else {
            (OutputT::from(value).unwrap_or_else(err) - OutputT::from(self.from).unwrap_or_else(err))
                / OutputT::from(span).unwrap_or_else(err)
        }
    }

    pub(crate) fn map_from<FromT: NumIntervalValue>(
        &self,
        value: FromT,
        input_interval: &NumInterval<FromT>,
        out_of_range_policy: OutOfRangePolicy,
    ) -> T {
        self.map_from_unit(
            input_interval.map_to_unit::<BaseNumT>(value, out_of_range_policy),
            out_of_range_policy,
        )
    }

    pub(crate) fn _intersects(&self, other: Self) -> bool {
        self.from <= other.to && self.to >= other.from
    }

    pub(crate) fn contains_interval(&self, other: Self) -> bool {
        other.from >= self.from && other.to <= self.to
    }

    pub(crate) fn contains_value_closed(&self, value: T) -> bool {
        value >= self.from && value <= self.to
    }

    pub(crate) fn clamp(&self, value: T) -> T {
        if value < self.from {
            branches::mark_unlikely();
            self.from
        } else if value > self.to {
            branches::mark_unlikely();
            self.to
        } else {
            value
        }
    }

    pub(crate) fn clamp_and_invert(&self, value: T) -> T {
        self.try_invert_value(self.clamp(value)).unwrap_or(value)
    }

    pub(crate) fn try_invert_value(&self, value: T) -> Result<T> {
        if self.contains_value_closed(value) {
            let half_span = self.span() / <<T as NumIntervalSpanT>::SpanT as NumCast>::from(2).unwrap();
            let dist_from_from = self.from.span_to(value);
            if dist_from_from <= half_span {
                Ok(self.to - T::from(dist_from_from).unwrap())
            } else {
                Ok(self.from + T::from(value.span_to(self.to)).unwrap())
            }
        } else {
            let err = format!(
                "Value {:?} is out of interval {:?}, can't invert value within this interval.",
                value, self
            );
            log::warn!("{}", err);
            bail!(err)
        }
    }

    #[allow(unused)]
    pub(crate) fn midpoint(&self) -> T {
        let span_halved = self
            .span()
            .div(<<T as NumIntervalSpanT>::SpanT as NumCast>::from(2).expect("Can't fail"));
        self.from + T::from(span_halved).expect("Must not fail")
    }

    pub(crate) fn span(&self) -> <T as NumIntervalSpanT>::SpanT {
        self.from.span_to(self.to)
    }

    pub(crate) fn cast<OtherT: NumIntervalValue>(&self) -> Option<NumInterval<OtherT>> {
        Some(NumInterval {
            from: OtherT::from(self.from)?,
            to: OtherT::from(self.to)?,
        })
    }
}

#[macro_export]
macro_rules! interval_grow_to_fit {
    ( $interval:ident, $value:ident) => {{
        let mut tmp = $interval.clone();
        tmp.from = tmp.from.min($value);
        tmp.to = tmp.to.max($value);
        tmp
    }};
}

#[cfg(test)]
mod tests {
    use crate::test_utils::fp_approx_eq;

    use super::*;

    #[test]
    fn contains_interval() {
        assert!(NumInterval::new(0.0, 1.0).contains_interval(NumInterval::new(0.0, 1.0)));
        assert!(NumInterval::new(0.0, 1.0).contains_interval(NumInterval::new(0.3, 0.6)));
        assert!(!NumInterval::new(0.0, 1.0).contains_interval(NumInterval::new(0.3, 1.2)));
    }

    #[test]
    fn intersects() {
        assert!(NumInterval::new(0.0, 1.0)._intersects(NumInterval::new(0.0, 1.0)));
        assert!(NumInterval::new(0.0, 1.0)._intersects(NumInterval::new(0.3, 1.6)));
        assert!(!NumInterval::new(0.0, 1.0)._intersects(NumInterval::new(1.1, 1.2)));
    }

    #[test]
    fn test_span_signed_crossing_zero() {
        // Standard negative to positive
        assert_eq!(NumInterval::new(-10i8, 10i8).span(), 20u8);

        // Full signed range should equal max unsigned
        assert_eq!(NumInterval::new(i8::MIN, i8::MAX).span(), u8::MAX);
        assert_eq!(NumInterval::new(i16::MIN, i16::MAX).span(), u16::MAX);
        assert_eq!(NumInterval::new(i32::MIN, i32::MAX).span(), u32::MAX);

        // Crossing zero with offset
        assert_eq!(NumInterval::new(-50i32, 50i32).span(), 100u32);
    }

    #[test]
    fn test_span_float_and_unsigned() {
        assert_eq!(NumInterval::new(0.0 as BaseNumT, 5.0).span(), 5.0);
        assert_eq!(NumInterval::new(100u64, 250u64).span(), 150u64);
        assert_eq!(NumInterval::new(0u8, u8::MAX).span(), u8::MAX);
    }

    #[test]
    fn test_zero_span_edge_cases() {
        let zero_span = NumInterval::new(5.0 as BaseNumT, 5.0);
        assert!(zero_span.is_zero_span());
        assert_eq!(zero_span.span(), 0.0);

        // map_to_unit should safely return 0.0 instead of panicking on division by zero
        assert_eq!(zero_span.map_to_unit::<BaseNumT>(100.0, OutOfRangePolicy::Allow), 0.0);

        // map_from_unit should ignore the normalized value and return `from`
        assert_eq!(zero_span.map_from_unit(0.0, OutOfRangePolicy::Allow), 5.0);
        assert_eq!(zero_span.map_from_unit(1.0, OutOfRangePolicy::Allow), 5.0);
        assert_eq!(zero_span.map_from_unit(999.0, OutOfRangePolicy::Allow), 5.0);
    }

    #[test]
    fn test_map_from_unit_policies() {
        let target = NumInterval::new(10.0 as BaseNumT, 20.0);

        // Normal in-range mapping
        assert!(fp_approx_eq(target.map_from_unit(0.5, OutOfRangePolicy::Allow), 15.0));

        // Allow: extrapolation
        assert!(fp_approx_eq(target.map_from_unit(2.0, OutOfRangePolicy::Allow), 30.0));
        assert!(fp_approx_eq(target.map_from_unit(-0.5, OutOfRangePolicy::Allow), 5.0));
        assert!(fp_approx_eq(target.map_to_unit(0.0, OutOfRangePolicy::Allow), -1.0));
        assert!(fp_approx_eq(target.map_to_unit(5.0, OutOfRangePolicy::Allow), -0.5));
        assert!(fp_approx_eq(target.map_to_unit(25.0, OutOfRangePolicy::Allow), 1.5));
        assert!(fp_approx_eq(target.map_to_unit(30.0, OutOfRangePolicy::Allow), 2.0));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(30.0, OutOfRangePolicy::Allow),
            3.0
        ));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(25.0, OutOfRangePolicy::Allow),
            2.0
        ));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(5.0, OutOfRangePolicy::Allow),
            -2.0
        ));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(0.0, OutOfRangePolicy::Allow),
            -3.0
        ));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(-5.0, OutOfRangePolicy::Allow),
            -4.0
        ));
        assert!(fp_approx_eq(
            target.map_to_symm_unit(-7.5, OutOfRangePolicy::Allow),
            -4.5
        ));

        // Clamp & WarnAndClamp: should clamp to [0.0, 1.0] range
        assert!(fp_approx_eq(target.map_from_unit(1.5, OutOfRangePolicy::Clamp), 20.0));
        assert!(fp_approx_eq(
            target.map_from_unit(-0.2, OutOfRangePolicy::WarnAndClamp),
            10.0
        ));
    }

    #[test]
    fn test_map_from_symm_unit_boundaries() {
        let target = NumInterval::new(10.0f64, 30.0);
        // -1.0 -> from, 0.0 -> midpoint, 1.0 -> to
        assert!(fp_approx_eq(
            target.map_from_symm_unit(-1.0_f32, OutOfRangePolicy::Allow),
            10.0
        ));
        assert!(fp_approx_eq(
            target.map_from_symm_unit(0.0_f32, OutOfRangePolicy::Allow),
            20.0
        ));
        assert!(fp_approx_eq(
            target.map_from_symm_unit(1.0_f32, OutOfRangePolicy::Allow),
            30.0
        ));

        // Out of bounds with clamp
        assert!(fp_approx_eq(
            target.map_from_symm_unit(-5.0_f32, OutOfRangePolicy::Clamp),
            10.0
        ));
        assert!(fp_approx_eq(
            target.map_from_symm_unit(5.0_f32, OutOfRangePolicy::Clamp),
            30.0
        ));
    }

    #[test]
    fn test_clamp_and_invert() {
        let interval = NumInterval::new(0.0 as BaseNumT, 10.0);

        // Inside: clamp is identity, invert reflects across midpoint
        assert!(fp_approx_eq(interval.clamp_and_invert(3.0), 7.0));
        assert!(fp_approx_eq(interval.clamp_and_invert(0.0), 10.0));
        assert!(fp_approx_eq(interval.clamp_and_invert(10.0), 0.0));

        // Below: clamps to `from`, inverts to `to`
        assert!(fp_approx_eq(interval.clamp_and_invert(-5.0), 10.0));
        // Above: clamps to `to`, inverts to `from`
        assert!(fp_approx_eq(interval.clamp_and_invert(15.0), 0.0));
    }

    #[test]
    fn test_scaling() {
        assert_eq!(NumInterval::new(1.0, 2.0).scale(2.0), NumInterval::new(2.0, 4.0));
    }

    #[test]
    fn test_try_invert_value_boundaries() {
        let r = NumInterval::new(-10.0 as BaseNumT, 20.0);
        // Exact boundaries
        assert_eq!(r.try_invert_value(-10.0).unwrap(), 20.0);
        assert_eq!(r.try_invert_value(20.0).unwrap(), -10.0);
        // Midpoint reflects to itself
        assert_eq!(r.try_invert_value(5.0).unwrap(), 5.0);
        // Out of bounds returns Error
        assert!(r.try_invert_value(-10.1).is_err());
        assert!(r.try_invert_value(20.1).is_err());
    }

    #[test]
    fn test_num_interval_macro_and_sorting() {
        // Macro should automatically sort
        let a = num_interval!(10, 5);
        let b = NumInterval::new(5, 10);
        assert_eq!(a, b);

        // Inplace sorting
        let mut c = NumInterval { from: 15_i32, to: -5 };
        assert!(c.sanitize_and_sort_inplace());
        assert_eq!(c.from, -5);
        assert_eq!(c.to, 15);

        // Already sorted returns false
        let mut d = NumInterval::new(-5i32, 15);
        assert!(!d.sanitize_and_sort_inplace());
    }

    #[test]
    fn test_midpoint_calculation() {
        // Integer truncation towards zero
        assert_eq!(NumInterval::new(0i32, 10i32).midpoint(), 5);
        assert_eq!(NumInterval::new(1i32, 4i32).midpoint(), 2);
        assert_eq!(NumInterval::new(-3i32, 4i32).midpoint(), 0);

        assert_eq!(NumInterval::new(u8::MAX - 2_u8, u8::MAX).midpoint(), u8::MAX - 1_u8);
        assert_eq!(NumInterval::new(u8::MIN, u8::MAX).midpoint(), u8::MAX / 2_u8);
        assert_eq!(NumInterval::new(i8::MIN, i8::MAX).midpoint(), -1_i8);
        assert_eq!(NumInterval::new(i8::MIN + 2, i8::MAX).midpoint(), 0_i8);

        // Float precision
        assert!(fp_approx_eq(NumInterval::new(0.0, 1.0).midpoint(), 0.5));
        assert!(fp_approx_eq(NumInterval::new(-1.5, 2.5).midpoint(), 0.5));
    }

    #[test]
    fn test_cast_success_and_failure() {
        // Successful narrowing/widening
        let i32_range = NumInterval::new(100i32, 200);
        assert_eq!(i32_range.cast::<i64>().unwrap().from, 100i64);
        assert_eq!(i32_range.cast::<BaseNumT>().unwrap().from, 100.0);

        // Failing cast due to overflow
        let large_range = NumInterval::new(i64::MAX - 100, i64::MAX);
        assert!(large_range.cast::<i32>().is_none());

        // Failing cast due to precision loss (optional, depends on NumCast impl)
        let huge_float = NumInterval::new(f64::MAX - 1.0, f64::MAX);
        assert!(huge_float.cast::<BaseNumT>().unwrap().is_zero_span()); // BaseNumericT can't represent f64::MAX
    }

    #[test]
    fn test_partial_ord_tie_breaking() {
        // Primary ordering: Span size
        let small = NumInterval::new(0i32, 5);
        let medium = NumInterval::new(-100, 0); // span = 100
        let large = NumInterval::new(0, 200);

        assert!(small < medium);
        assert!(medium < large);

        // Secondary ordering: `from` value when spans are equal
        let a = NumInterval::new(-10i32, 10); // span 20
        let b = NumInterval::new(0i32, 20); // span 20
        let c = NumInterval::new(5i32, 25); // span 20

        assert!(a < b);
        assert!(b < c);
        assert!(a < c);
    }

    #[test]
    fn test_equality_ignores_position_when_sorted() {
        // Equality compares span first, then from.
        // Since construction auto-sorts, (10, 5) and (5, 10) are identical.
        assert_eq!(NumInterval::new(10i32, 5), NumInterval::new(5, 10));
        assert_ne!(NumInterval::new(5i32, 10), NumInterval::new(6, 11)); // Same span, different from
    }

    #[test]
    fn test_range_conversions_roundtrip() {
        let interval = NumInterval::new(5i32, 15);

        let incl: RangeInclusive<i32> = interval.into();
        assert_eq!(*incl.start(), 5);
        assert_eq!(*incl.end(), 15);

        let excl: Range<i32> = interval.into();
        assert_eq!(excl.start, 5);
        assert_eq!(excl.end, 15);

        // Roundtrip
        let back: NumInterval<i32> = excl.into();
        assert_eq!(interval, back);
    }

    #[test]
    fn test_unit_identifiers() {
        assert!(NumInterval::new(0i8, 1).is_unit());
        assert!(NumInterval::new(0.0f64, 1.0).is_unit());
        assert!(!NumInterval::new(1i8, 2).is_unit()); // Shifted, not unit
        assert!(!NumInterval::new(0i8, 2).is_unit()); // Wrong span

        // is_symm_unit specifically checks for [-1, 1]
        assert!(NumInterval::new(-1i8, 1).is_symm_unit());
        assert!(NumInterval::new(-1.0 as BaseNumT, 1.0).is_symm_unit());
        assert!(!NumInterval::new(-2i8, 0).is_symm_unit());
        assert!(!NumInterval::new(-1.0 as BaseNumT, 2.0).is_symm_unit());
    }

    #[test]
    fn test_span() {
        assert!(i8::MIN.unsigned_abs() == 128u8);
        assert!(-3i32 as u64 == 0xfffffffffffffffd_u64);
        assert!(-3i8 as u8 == 0xfd_u8);
        assert!(NumInterval::new(i128::MIN, i128::MAX).span() == i128::MAX as u128 + i128::MIN.unsigned_abs());
        assert!(NumInterval::new(i8::MIN, i8::MAX).span() == i8::MAX as u8 + i8::MIN.unsigned_abs());
        assert!(NumInterval::new(1, i8::MAX).span() == (i8::MAX - 1) as u8);
        assert!(NumInterval::new(i8::MIN, 123).span() == 123_u8 + i8::MIN.unsigned_abs());
        assert!(NumInterval::new(BaseNumT::MIN, BaseNumT::MAX).span() == BaseNumT::MAX - BaseNumT::MIN);
    }

    #[test]
    fn test_zero_interval() {
        assert!(NumInterval::new(0, 0).is_zero());
        assert!(NumInterval::new(0.0, 0.0).is_zero());
        assert!(NumInterval::new(-0.0, 0.0).is_zero());
        assert!(NumInterval::new(-0.0, -0.0).is_zero());
    }

    #[test]
    fn test_are_unit_intervals() {
        assert!(NumInterval::new(0, 1).is_unit());
        assert!(NumInterval::new(0.0, 1.0).is_unit());
        assert!(!NumInterval::new(0.1, 1.0).is_unit());

        assert!(NumInterval::new(-1, 1).is_symm_unit());
        assert!(!NumInterval::new(-1, 2).is_symm_unit());
        assert!(!NumInterval::new(-1.5, 1.0).is_symm_unit());
        assert!(!NumInterval::new(-1.5, 1.5).is_symm_unit());
        assert!(NumInterval::new(-1.0, 1.0).is_symm_unit());
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
    fn test_default() {
        assert_eq!(
            UNIT_INTERVAL,
            NumInterval::default(),
            "The default of number interval type should be {:?}.",
            UNIT_INTERVAL
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
            "Small interval should be less than medium interval"
        );
        assert!(
            r_large > r_medium_a,
            "Large interval should be greater than medium interval"
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
    fn test_full_span_overflow() {
        assert_eq!(NumInterval::new(i8::MIN, i8::MAX).span(), u8::MAX);
        assert_eq!(NumInterval::new(i128::MIN, i128::MAX).span(), u128::MAX);
    }

    #[test]
    fn test_map_to_unit_exptrapolation_overflow_case_for_int() {
        let interval = NumInterval::new(10_i8, 20);
        let mapped = interval.map_to_unit::<BaseNumT>(5, OutOfRangePolicy::Allow);
        assert!(
            fp_approx_eq(-0.5 as BaseNumT, mapped),
            "Extrapolation below 'from' failed. Expected -0.5, got {}",
            mapped
        );

        let mapped = interval.map_to_unit::<BaseNumT>(25, OutOfRangePolicy::Allow);
        assert!(
            fp_approx_eq(1.5 as BaseNumT, mapped),
            "Extrapolation below 'from' failed. Expected 1.5, got {}",
            mapped
        );
    }

    #[test]
    fn test_unit_symm_unit_interconversion() {
        assert!(unit_to_symm_unit(0.0, OutOfRangePolicy::WarnAndClamp) == -1.0);
        assert!(unit_to_symm_unit(0.5, OutOfRangePolicy::WarnAndClamp) == 0.0);
        assert!(unit_to_symm_unit(-1.0, OutOfRangePolicy::Allow) == -3.0);
        assert!(symm_unit_to_unit(0.0, OutOfRangePolicy::WarnAndClamp) == 0.5);
        assert!(symm_unit_to_unit(-1.0, OutOfRangePolicy::WarnAndClamp) == 0.0);
        assert!(symm_unit_to_unit(1.0, OutOfRangePolicy::WarnAndClamp) == 1.0);
        assert!(symm_unit_to_unit(-3.0, OutOfRangePolicy::Allow) == -1.0);
    }

    #[test]
    fn test_invert_involution_odd_span() {
        let r = NumInterval::new(0i32, 5i32);
        for v in 0..=5 {
            let inv = r.try_invert_value(v).unwrap();
            let inv_inv = r.try_invert_value(inv).unwrap();
            assert_eq!(inv_inv, v, "invert(invert({v})) != {v} in interval {r:?}");
            let expected = 5 - v;
            assert_eq!(
                r.try_invert_value(v).unwrap(),
                expected,
                "invert({v}) should be {expected} in {r:?}"
            );
        }

        let r = NumInterval::new(-3i32, 4i32);
        for v in -3..=4 {
            let inv = r.try_invert_value(v).unwrap();
            let inv_inv = r.try_invert_value(inv).unwrap();
            assert_eq!(inv_inv, v, "invert(invert({v})) != {v} in interval {r:?}");
        }
    }

    #[test]

    fn test_containment() {
        let r = NumInterval::new(10.0, 20.0);
        assert!(r.contains_value_closed(10.0), "Inclusive should contain 'from'");
        assert!(r.contains_value_closed(15.0), "Inclusive should contain middle value");
        assert!(r.contains_value_closed(20.0), "Inclusive should contain 'to'");
        assert!(
            !r.contains_value_closed(9.9),
            "Inclusive should not contain value below 'from'"
        );
        assert!(
            !r.contains_value_closed(20.1),
            "Inclusive should not contain value above 'to'"
        );
    }

    #[test]
    fn test_inversion() {
        // Test for no wrapping issues in extreme
        assert_eq!(
            NumInterval::new(i8::MAX - 1, i8::MAX)
                .try_invert_value(i8::MAX)
                .unwrap(),
            i8::MAX - 1
        );

        assert_eq!(
            NumInterval::new(i8::MIN, i8::MIN + 1)
                .try_invert_value(i8::MIN)
                .unwrap(),
            i8::MIN + 1
        );

        assert_eq!(
            NumInterval::new(u8::MAX - 1, u8::MAX)
                .try_invert_value(u8::MAX)
                .unwrap(),
            u8::MAX - 1
        );

        assert_eq!(
            NumInterval::new(u8::MAX, u8::MAX).try_invert_value(u8::MAX).unwrap(),
            u8::MAX
        );

        assert_eq!(
            NumInterval::new(u8::MIN, u8::MIN).try_invert_value(u8::MIN).unwrap(),
            u8::MIN
        );

        let r = NumInterval::new(-10.0, 15.0);
        assert_eq!(
            r.try_invert_value(-1.0).unwrap(),
            6.0,
            "Inverting value inside interval {r:?} failed."
        );
        assert!(
            r.try_invert_value(-11.0).is_err(),
            "Inverting value our of interval {r:?} must return error."
        );
        let r = NumInterval::new(10.0, 20.0);
        assert_eq!(
            r.try_invert_value(15.0).unwrap(),
            15.0,
            "Inverting value inside interval {r:?} failed"
        );
        assert_eq!(
            r.try_invert_value(14.0).unwrap(),
            16.0,
            "Inverting value inside interval {r:?} failed"
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
            "Inverting value inside interval {r:?} failed"
        );
        assert_eq!(
            r.try_invert_value(-14.0).unwrap(),
            -16.0,
            "Inverting value inside interval {r:?} failed"
        );
    }

    #[test]
    fn test_clamping() {
        let r = NumInterval::new(-10.0, 20.0);
        assert_eq!(r.clamp(5.0), 5.0, "Clamping value inside interval failed");
        assert_eq!(r.clamp(-50.0), -10.0, "Clamping value below 'from' failed");
        assert_eq!(r.clamp(30.0), 20.0, "Clamping value above 'to' failed");
        assert_eq!(r.clamp(-10.0), -10.0, "Clamping value at 'from' failed");
        assert_eq!(r.clamp(20.0), 20.0, "Clamping value at 'to' failed");
    }

    #[test]
    fn test_normalization() {
        let input_interval = NumInterval::<BaseNumT>::new(0.0, 100.0);
        let output_interval = NumInterval::<BaseNumT>::new(-10.0, 20.0);

        let result_min = output_interval.map_from(0.0, &input_interval, OutOfRangePolicy::WarnAndClamp);
        assert!(
            fp_approx_eq(result_min, -10.0),
            "Min mapping failed: Got {}",
            result_min
        );

        let result_mid = output_interval.map_from(50.0, &input_interval, OutOfRangePolicy::WarnAndClamp);
        assert!(
            fp_approx_eq(result_mid, 5.0),
            "Midpoint mapping failed: Got {}",
            result_mid
        );

        let result_max = output_interval.map_from(100.0, &input_interval, OutOfRangePolicy::WarnAndClamp);
        assert!(fp_approx_eq(result_max, 20.0), "Max mapping failed: Got {}", result_max);

        let result_outside = output_interval.map_from(150.0, &input_interval, OutOfRangePolicy::Allow);
        assert!(
            fp_approx_eq(result_outside, 35.0),
            "Extrapolation mapping failed: Got {}",
            result_outside
        );

        let zero_span_input = NumInterval::new(42.0, 42.0);
        assert_eq!(
            zero_span_input.map_to_unit::<BaseNumT>(BaseNumT::MAX / 42.0, OutOfRangePolicy::Allow),
            0.0,
            "Mapping to unit interval from a zero span interval must return 0"
        );
        assert_eq!(
            output_interval.map_from(BaseNumT::MAX / 42.0, &zero_span_input, OutOfRangePolicy::WarnAndClamp),
            -10.0,
            "Zero span input should return output 'from'",
        );
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_try_invert_no_signed_overflow() {
        let _ = NumInterval::new(-100_i8, 100_i8).try_invert_value(-100_i8).unwrap();
    }

    #[test]
    fn test_extrapolation_over_dst_type_range_no_panic_sat_at_max() {
        assert_eq!(
            NumInterval::new(0_i8, 10_i8).map_from_unit(100.0_f64, OutOfRangePolicy::Allow),
            i8::MAX
        );
        assert_eq!(
            NumInterval::new(0_i8, 10_i8).map_from_symm_unit(100.0_f64, OutOfRangePolicy::Allow),
            i8::MAX
        )
    }

    #[test]
    fn test_extrapolation_over_dst_type_range_no_panic_sat_at_min() {
        assert_eq!(
            NumInterval::new(0_i8, 10_i8).map_from_unit(-100.0_f64, OutOfRangePolicy::Allow),
            i8::MIN
        );
        assert_eq!(
            NumInterval::new(0_u8, 10_u8).map_from_unit(-100.0_f64, OutOfRangePolicy::Allow),
            u8::MIN
        );
        assert_eq!(
            NumInterval::new(0_i8, 10_i8).map_from_symm_unit(-100.0_f64, OutOfRangePolicy::Allow),
            i8::MIN
        );
        assert_eq!(
            NumInterval::new(0_u8, 10_u8).map_from_symm_unit(-100.0_f64, OutOfRangePolicy::Allow),
            u8::MIN
        )
    }

    #[test]
    fn test_map_from_symm_unit_span_cast_no_panic() {
        let _ = NumInterval::new(0.0_f64, f64::MAX).map_from_symm_unit(0.0_f32, OutOfRangePolicy::Allow);
        let _ = NumInterval::new(0.0_f64, f64::MAX).map_from_symm_unit(f32::MIN, OutOfRangePolicy::Allow);
    }

    // =======================================================

    #[test]
    fn test_apply_out_of_range_policy_both_bounds_overflow() {
        let value_lower = f64::MIN;
        let value_higher = f64::MAX;
        let interval = NumInterval::new(f32::MIN, f32::MAX);
        let out_lower = interval.value_cast_with_out_of_range_policy(value_lower, OutOfRangePolicy::Clamp);
        assert_eq!(
            out_lower,
            f32::MIN,
            "Value of {value_lower} below outside target range should clamp to rage {interval}, but got {out_lower}",
        );
        let out_higher = interval.value_cast_with_out_of_range_policy(value_higher, OutOfRangePolicy::Clamp);
        assert_eq!(
            out_higher,
            f32::MAX,
            "Value of {value_higher} below outside target range should clamp to rage {interval}, but got {out_higher}",
        );
    }

    #[test]
    fn test_apply_out_of_range_policy_cast_panic() {
        assert!(
            i8::MIN
                == NumInterval::new(i8::MIN, i8::MAX)
                    .value_cast_with_out_of_range_policy(i16::MIN, OutOfRangePolicy::Clamp)
        );
    }

    #[test]
    fn test_apply_out_of_range_policy_cast() {
        let value =
            NumInterval::new(42.0_f64, f64::MAX).value_cast_with_out_of_range_policy(-5.0_f32, OutOfRangePolicy::Clamp);

        assert!(fp_approx_eq(value, 42.0_f64));

        let value =
            NumInterval::new(42.0_f32, f32::MAX).value_cast_with_out_of_range_policy(f64::MIN, OutOfRangePolicy::Clamp);

        assert!(fp_approx_eq(value as BaseNumT, 42.0 as BaseNumT));
    }

    #[cfg(debug_assertions)]
    #[test]

    fn test_clamp_nan_propagation() {
        assert!(NumInterval::new(0.0, 1.0).clamp(f32::NAN).is_nan());
    }

    #[test]
    fn test_apply_policy_cast_overflow_i32_to_i8() {
        let interval = NumInterval::new(10i8, 20i8);

        let val_large: i32 = 1000;
        let res_allow = interval.value_cast_with_out_of_range_policy(val_large, OutOfRangePolicy::Allow);
        assert_eq!(res_allow, i8::MAX, "Allow policy should saturate to max on overflow");

        let res_clamp = interval.value_cast_with_out_of_range_policy(val_large, OutOfRangePolicy::Clamp);
        assert_eq!(res_clamp, i8::MAX, "Clamp policy should saturate to max on overflow");

        let val_small: i32 = -1000;
        let res_allow_small = interval.value_cast_with_out_of_range_policy(val_small, OutOfRangePolicy::Allow);
        assert_eq!(
            res_allow_small,
            i8::MIN,
            "Allow policy should saturate to min on underflow"
        );
    }

    #[test]
    fn test_apply_policy_cast_failure_float_to_int() {
        let interval = NumInterval::new(0i8, 10i8);
        let val_large: f64 = 1000.0;
        let res = interval.value_cast_with_out_of_range_policy(val_large, OutOfRangePolicy::Clamp);
        assert_eq!(res, i8::MAX);
        let val_small: f64 = -1000.0;
        let res_small = interval.value_cast_with_out_of_range_policy(val_small, OutOfRangePolicy::Clamp);
        assert_eq!(res_small, i8::MIN);
    }

    #[test]
    fn test_apply_policy_warn_and_clamp() {
        let interval = NumInterval::new(0.0f64, 10.0f64);
        let val = 15.0f64;
        let res = interval.value_cast_with_out_of_range_policy(val, OutOfRangePolicy::WarnAndClamp);
        assert_eq!(res, 10.0, "Should clamp to interval max");
    }

    #[test]
    fn test_apply_policy_panic() {
        let interval = NumInterval::new(0i32, 10i32);
        let val = 20i32;
        let result =
            std::panic::catch_unwind(|| interval.value_cast_with_out_of_range_policy(val, OutOfRangePolicy::_Panic));
        assert!(result.is_err(), "Policy _Panic should trigger panic");
    }
}
