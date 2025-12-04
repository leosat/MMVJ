use num_traits::Float;

pub(crate) fn fp_approx_eq<FloatT: Float>(a: FloatT, b: FloatT) -> bool {
    if a == b {
        return true;
    }

    if a.is_nan() || b.is_nan() {
        return false;
    }

    let diff = (a - b).abs();
    let scale = a.abs().max(b.abs());
    let four = FloatT::from(4.0).unwrap();

    let tolerance = scale.max(FloatT::one()) * FloatT::epsilon() * four;
    diff <= tolerance
}
