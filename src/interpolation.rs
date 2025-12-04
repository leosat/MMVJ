use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

pub(crate) struct InterpolationCurve;

impl InterpolationCurve {
    pub(crate) fn linear(x: f32, slope: f32, shift_x: f32, shift_y: f32) -> f32 {
        slope * (x - shift_x) + shift_y
    }

    pub(crate) fn quadratic(x: f32) -> f32 {
        x * x
    }

    pub(crate) fn cubic(x: f32) -> f32 {
        x * x * x
    }

    pub(crate) fn smoothstep(x: f32) -> f32 {
        3.0 * x * x - 2.0 * x * x * x
    }

    /// Default steepness: 10
    pub(crate) fn s_curve(x: f32, steepness: f32) -> f32 {
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

    pub(crate) fn exponential(x: f32, base: f32) -> f32 {
        if base <= 1.0 {
            return x;
        }

        (base.powf(x) - 1.0) / (base - 1.0)
    }

    pub(crate) fn symmetric_power(x: f32, power: f32) -> f32 {
        if power <= 0.0 {
            return x;
        }

        // Scale x from [0, 1] to [-1, 1]
        let x_scaled = 2.0 * x - 1.0;

        // Apply power curve symmetrically
        let x_abs_pow = x_scaled.abs().powf(power);
        let sign = if x_scaled < 0.0 { -1.0 } else { 1.0 };

        // Scale back to [0, 1]
        (x_abs_pow * sign + 1.0) / 2.0
    }

    pub(crate) fn power(x: f32, power: f32) -> f32 {
        if power <= 0.0 {
            return x;
        }

        let result = x.abs().powf(power);
        if x < 0.0 {
            -result
        } else {
            result
        }
    }
}

// Value filters for input conditioning
pub(crate) struct ValueFilter;

impl ValueFilter {
    pub(crate) fn ema(prev_val: f32, current_input: f32, dt: f32, time_constant: f32) -> f32 {
        let alpha = 1.0 - (-dt / time_constant).exp();
        prev_val + alpha * (current_input - prev_val)
    }

    pub(crate) fn lowpass(prev_val: f32, current_input: f32, dt: f32, time_constant: f32) -> f32 {
        let alpha = if time_constant > 0.0 {
            1.0 - (-dt / time_constant).exp()
        } else {
            1.0
        };
        return prev_val + alpha * (current_input - prev_val);
    }

    pub(crate) fn _moving_average_weighted(
        new_value: f32,
        history: &mut VecDeque<(Instant, f32)>,
        now: Instant,
        window: Duration,
    ) -> f32 {
        history.push_back((now, new_value));
        let window_limit = now - window;
        while history.len() > 2 && history[1].0 < window_limit {
            history.pop_front();
        }
        if history.len() < 2 {
            return new_value;
        }
        let mut area = 0.0;
        let mut total_dt = 0.0;
        for i in 0..history.len() - 1 {
            let (t0, val) = history[i];
            let (t1, _) = history[i + 1];
            let start = t0.max(window_limit);
            let dt = t1.duration_since(start).as_secs_f32();
            if dt > 0.0 {
                area += val * dt;
                total_dt += dt;
            }
        }
        if total_dt > 0.0 {
            area / total_dt
        } else {
            new_value
        }
    }

    pub(crate) fn _moving_average_time_based(
        value: f32,
        history: &mut VecDeque<(Instant, f32)>,
        now: Instant,
        window: Duration,
    ) -> f32 {
        history.push_back((now, value));
        let window_start = now.checked_sub(window).unwrap_or(now);
        while history.len() > 1 && history[1].0 < window_start {
            history.pop_front();
        }
        if history.len() < 2 {
            return value;
        }
        let mut total_weighted_sum = 0.0;
        let mut total_time = 0.0;
        for i in 0..(history.len() - 1) {
            let (t0, v0) = history[i];
            let (t1, _) = history[i + 1];

            let effective_start = t0.max(window_start);
            let duration = t1.duration_since(effective_start).as_secs_f32();

            if duration > 0.0 {
                total_weighted_sum += v0 * duration;
                total_time += duration;
            }
        }
        if total_time > 0.0 {
            total_weighted_sum / total_time
        } else {
            value
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_interpolation() {
        assert_eq!(InterpolationCurve::linear(0.1, 1., 0., 0.), 0.1);
        assert_eq!(InterpolationCurve::linear(0.5, 1., 0., 0.), 0.5);
        assert_eq!(InterpolationCurve::linear(1., 1., 0., 0.), 1.);
    }

    #[test]
    fn test_quadratic_interpolation() {
        assert_eq!(InterpolationCurve::quadratic(0.5), 0.25);
        assert_eq!(InterpolationCurve::quadratic(0.), 0.0);
        assert_eq!(InterpolationCurve::quadratic(1.), 1.0);
    }

    #[test]
    fn test_smoothstep_interpolation() {
        let result = InterpolationCurve::smoothstep(0.5);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_symmetric_power() {
        let result = InterpolationCurve::symmetric_power(0.5, 2.0);
        assert_eq!(result, 0.5); // Should be symmetric around 0.5

        let result_low = InterpolationCurve::symmetric_power(0.25, 2.0);
        let result_high = InterpolationCurve::symmetric_power(0.75, 2.0);

        // Should be symmetric
        assert!((result_low - (1.0 - result_high)).abs() < 0.001);
    }
}
