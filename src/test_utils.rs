use num_traits::Float;

pub(crate) fn fp_approx_eq<FloatT: Float>(a: FloatT, b: FloatT) -> bool {
    (a - b).abs() < FloatT::epsilon()
}
