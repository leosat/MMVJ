use crate::{
    base_num::BaseNumT,
    num_interval::{NumInterval, OutOfRangePolicy},
};

use crate::num_interval::UNIT_INTERVAL;

pub(crate) fn linear(x: BaseNumT, slope: BaseNumT, shift_x: BaseNumT, shift_y: BaseNumT) -> BaseNumT {
    slope * (x - shift_x) + shift_y
}

pub(crate) fn apply_center_symmetric_with_abs_value<T: crate::num_interval::NumIntervalValue + num_traits::Float>(
    v: T,
    interval: NumInterval<T>,
    f: impl Fn(T) -> T,
    out_of_range_policy: OutOfRangePolicy,
) -> T {
    let value_symm_norm = interval.map_to_symm_unit::<T>(v, out_of_range_policy);
    interval.map_from_symm_unit(value_symm_norm.signum() * f(value_symm_norm.abs()), out_of_range_policy)
}

pub(crate) fn smoothstep(x: BaseNumT) -> BaseNumT {
    UNIT_INTERVAL.clamp(3.0 * x * x - 2.0 * x * x * x)
}

pub(crate) fn s_curve(x: BaseNumT, steepness: BaseNumT) -> BaseNumT {
    if steepness.abs() < 1e-8 {
        return x;
    }

    let u = 0.5 * steepness * (x - 0.5);
    let denom = (0.25 * steepness).tanh();

    if denom.abs() < 1e-8 {
        return x;
    }

    let y = 0.5 * (1.0 + u.tanh() / denom);
    y.clamp(0.0, 1.0)
}

pub(crate) fn exp_curve(x: BaseNumT, mut base: BaseNumT) -> BaseNumT {
    if base <= 1.0 {
        log::error!("Exponential curve base ({}) must be more than 1.0.", base);
        base = crate::schemas_transform::default_norm_exp_base();
    };
    if !UNIT_INTERVAL.contains_value_closed(x) {
        log::error!("{x} is not contained in {UNIT_INTERVAL}");
        return x;
    };
    (base.powf(x) - 1.0) / (base - 1.0)
}

pub(crate) fn signed_power(x: BaseNumT, power: BaseNumT) -> BaseNumT {
    if power <= 0.0 {
        log::error!("Power must be > 0.0.");
        return x;
    }

    x.abs().powf(power) * x.signum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear() {
        assert_eq!(linear(0.1, 1., 0., 0.), 0.1);
        assert_eq!(linear(0.5, 1., 0., 0.), 0.5);
        assert_eq!(linear(1., 1., 0., 0.), 1.);
    }

    #[test]
    fn test_smoothstep() {
        let result = smoothstep(0.5);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_power() {
        let result = signed_power(0.5, 2.0);
        assert_eq!(result, 0.25);

        assert_eq!(
            apply_center_symmetric_with_abs_value(
                1.2345,
                UNIT_INTERVAL,
                |v_abs| { v_abs },
                OutOfRangePolicy::WarnIfDebugAndClamp,
            ),
            1.0
        );

        assert_eq!(
            apply_center_symmetric_with_abs_value(1.2345, UNIT_INTERVAL, |v_abs| { v_abs }, OutOfRangePolicy::Allow,),
            1.2345
        );

        assert_eq!(
            apply_center_symmetric_with_abs_value(
                0.5,
                UNIT_INTERVAL,
                |v_abs| { signed_power(v_abs, 42.0) },
                OutOfRangePolicy::Allow,
            ),
            0.5
        );

        assert_eq!(
            apply_center_symmetric_with_abs_value(
                0.75,
                UNIT_INTERVAL,
                |v_abs| { signed_power(v_abs, 2.0) },
                OutOfRangePolicy::Allow,
            ),
            0.625
        )
    }
}
