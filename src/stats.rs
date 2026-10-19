use crate::base_num::BaseNumT;

pub(crate) trait StatsIface: std::fmt::Debug {
    type In;
    type Out: Into<BaseNumT>;
    fn with_threshold_from(self, threshold: Self::In) -> Self;
    fn with_mean_alpha(self, alpha: Self::In) -> Self;
    fn record(&self, v: Self::In);
    fn get_mean(&self) -> Self::Out;
    #[allow(unused)]
    fn get_max(&self) -> Self::Out;
    #[allow(unused)]
    fn get_min(&self) -> Self::Out;
    #[allow(unused)]
    fn get_last(&self) -> Self::Out;
}
