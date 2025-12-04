use std::collections::HashMap;

pub(crate) struct InterpolationCurve;

impl InterpolationCurve {
    pub(crate) fn apply(curve_type: &str, x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        // Ensure x is clamped to [0, 1]
        let x = x.clamp(0.0, 1.0);

        match curve_type {
            "linear" => Self::linear(x, params),
            "quadratic" => Self::quadratic(x),
            "cubic" => Self::cubic(x),
            "smoothstep" => Self::smoothstep(x),
            "s_curve" => Self::s_curve(x, params),
            "exponential" => Self::exponential(x, params),
            "symmetric_power" => Self::symmetric_power(x, params),
            "power" => Self::power(x, params),
            _ => x, // Default to linear
        }
    }

    fn linear(x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        if let Some(p) = params {
            let slope = p.get("slope").copied().unwrap_or(1.0);
            let shift_x = p.get("shift_x").copied().unwrap_or(0.0);
            let shift_y = p.get("shift_y").copied().unwrap_or(0.0);
            slope * (x - shift_x) + shift_y
        } else {
            x
        }
    }

    fn quadratic(x: f32) -> f32 {
        x * x
    }

    fn cubic(x: f32) -> f32 {
        x * x * x
    }

    fn smoothstep(x: f32) -> f32 {
        3.0 * x * x - 2.0 * x * x * x
    }

    fn s_curve(x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        let steepness = params
            .and_then(|p| p.get("steepness"))
            .copied()
            .unwrap_or(10.0);

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

    fn exponential(x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        let base = params.and_then(|p| p.get("base")).copied().unwrap_or(2.0);

        if base <= 1.0 {
            return x;
        }

        (base.powf(x) - 1.0) / (base - 1.0)
    }

    fn symmetric_power(x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        let power = params.and_then(|p| p.get("power")).copied().unwrap_or(2.0);

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

    fn power(x: f32, params: Option<&HashMap<String, f32>>) -> f32 {
        let power = params.and_then(|p| p.get("power")).copied().unwrap_or(2.0);

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
    //     pub(crate) fn low_pass(value: f32, history: &mut Vec<f32>, params: &HashMap<String, f32>) -> f32 {
    //         let cutoff = params.get("cutoff").copied().unwrap_or(0.1);

    //         if history.is_empty() {
    //             history.push(value);
    //             return value;
    //         }

    //         let alpha = cutoff.min(1.0);
    //         let filtered = alpha * value + (1.0 - alpha) * history.last().copied().unwrap_or(value);

    //         history.push(filtered);
    //         if history.len() > 50 {
    //             history.remove(0);
    //         }

    //         filtered
    //     }

    //     pub(crate) fn high_pass(value: f32, history: &mut Vec<f32>, params: &HashMap<String, f32>) -> f32 {
    //         let cutoff = params.get("cutoff").copied().unwrap_or(0.1);

    //         if history.len() < 2 {
    //             history.push(value);
    //             return value;
    //         }

    //         let alpha = cutoff.min(1.0);
    //         let filtered = alpha * (history[history.len() - 1] + value - history[history.len() - 2]);

    //         history.push(filtered);
    //         if history.len() > 50 {
    //             history.remove(0);
    //         }

    //         filtered
    //     }

    pub(crate) fn moving_average(
        value: f32,
        history: &mut std::collections::VecDeque<f32>,
        samples: usize,
    ) -> f32 {
        history.push_back(value);
        if history.len() > samples {
            history.pop_front();
        }

        let sum: f32 = history.iter().sum();
        sum / history.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linear_interpolation() {
        assert_eq!(InterpolationCurve::apply("linear", 0.5, None), 0.5);
        assert_eq!(InterpolationCurve::apply("linear", 0.0, None), 0.0);
        assert_eq!(InterpolationCurve::apply("linear", 1.0, None), 1.0);
    }

    #[test]
    fn test_quadratic_interpolation() {
        assert_eq!(InterpolationCurve::apply("quadratic", 0.5, None), 0.25);
        assert_eq!(InterpolationCurve::apply("quadratic", 0.0, None), 0.0);
        assert_eq!(InterpolationCurve::apply("quadratic", 1.0, None), 1.0);
    }

    #[test]
    fn test_smoothstep_interpolation() {
        let result = InterpolationCurve::apply("smoothstep", 0.5, None);
        assert!((result - 0.5).abs() < 0.001);
    }

    #[test]
    fn test_symmetric_power() {
        let mut params = HashMap::new();
        params.insert("power".to_string(), 2.0);

        let result = InterpolationCurve::apply("symmetric_power", 0.5, Some(&params));
        assert_eq!(result, 0.5); // Should be symmetric around 0.5

        let result_low = InterpolationCurve::apply("symmetric_power", 0.25, Some(&params));
        let result_high = InterpolationCurve::apply("symmetric_power", 0.75, Some(&params));

        // Should be symmetric
        assert!((result_low - (1.0 - result_high)).abs() < 0.001);
    }
}
