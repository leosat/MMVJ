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

pub(crate) struct TestingLoggerHelper;

impl TestingLoggerHelper {
    #[must_use]
    pub(crate) fn init() -> Self {
        testing_logger::setup();
        Self
    }
    pub(crate) fn print_testing_logger_captured_logs() {
        testing_logger::validate(|logs| {
            logs.iter().for_each(|l| {
                println!("{} {} {}", &l.target, l.level, &l.body);
            })
        });
    }
}

impl Drop for TestingLoggerHelper {
    fn drop(&mut self) {
        Self::print_testing_logger_captured_logs()
    }
}
