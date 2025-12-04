cfg_if::cfg_if! {
    if #[cfg(feature="base_num_f64")] {
        pub(crate) type BaseNumT = f64;
        pub(crate) type BaseAtomicT = atomic_float::AtomicF64;
        pub(crate) use std::f64::consts as BaseNumConsts;
    } else {
        pub(crate) type BaseNumT = f32;
        pub(crate) type BaseAtomicT = atomic_float::AtomicF32;
        pub(crate) use std::f32::consts as BaseNumConsts;
    }
    // NB: supporting e.g. fixed-point will require more changes than just switching types.
}
