use std::{
    collections::VecDeque,
    time::{Duration, Instant},
};

use crate::{base_num::BaseNumT, config::MIN_BASE_FREQ_HZ};

//-----------------------------------------
pub(crate) const MIN_DT: BaseNumT = 1.0e-6;
pub(crate) const fn clamp_dt_by_min_and_max_period(v: BaseNumT) -> BaseNumT {
    v.clamp(MIN_DT as BaseNumT, 1.0 / MIN_BASE_FREQ_HZ as BaseNumT)
}
pub(crate) const fn clamp_dt_by_zero_and_max_period(v: BaseNumT) -> BaseNumT {
    v.clamp(0.0 as BaseNumT, 1.0 / MIN_BASE_FREQ_HZ as BaseNumT)
}

//-----------------------------------------
#[allow(unused)]
pub(crate) struct MAWeighted;
impl MAWeighted {
    pub(crate) fn _moving_average_weighted(
        new_value: BaseNumT,
        history: &mut VecDeque<(Instant, BaseNumT)>,
        now: Instant,
        window: Duration,
    ) -> BaseNumT {
        history.push_back((now, new_value));
        let window_limit = now - window;
        while history.len() > 2 && history[1].0 < window_limit {
            history.pop_front();
        }
        if history.len() < 2 {
            return new_value;
        }
        let mut area = 0.0 as BaseNumT;
        let mut total_dt = 0.0 as BaseNumT;
        for i in 0..history.len() - 1 {
            let (t0, val) = history[i];
            let (t1, _) = history[i + 1];
            let start = t0.max(window_limit);
            let dt = clamp_dt_by_zero_and_max_period(t1.duration_since(start).as_secs_f32() as BaseNumT);

            if dt > 0.0 {
                area += val * dt;
                total_dt += dt;
            }
        }
        if total_dt > 0.0 { area / total_dt } else { new_value }
    }
}

//-----------------------------------------
#[derive(Debug, Clone, Copy)]
pub(crate) struct EmaFilter {
    prev_time: Instant,
    prev_val: BaseNumT,
}

impl Default for EmaFilter {
    fn default() -> Self {
        Self {
            prev_time: Instant::now(),
            prev_val: Default::default(),
        }
    }
}

//-----------------------------------------
impl EmaFilter {
    pub(crate) fn reset(&mut self, reset_val: BaseNumT) {
        self.prev_val = reset_val;
    }

    pub(crate) fn _new(val: BaseNumT, now: Instant) -> Self {
        Self {
            prev_time: now,
            prev_val: val,
        }
    }
    pub(crate) fn filter(&mut self, val: BaseNumT, now: Instant, tau: BaseNumT) -> BaseNumT {
        if branches::unlikely(tau <= 0.0) {
            return val;
        }
        let dt = clamp_dt_by_zero_and_max_period(now.duration_since(self.prev_time).as_secs_f32() as BaseNumT);
        self.prev_time = now;
        self.prev_val = (-(-dt / tau).exp_m1()).mul_add(val - self.prev_val, self.prev_val);
        self.prev_val
    }
}

//-----------------------------------------
#[derive(Debug, Clone)]
pub(crate) struct OneEuroFilter {
    prev_val: BaseNumT,
    prev_val_d: BaseNumT,
    prev_time: Instant,
}

impl Default for OneEuroFilter {
    fn default() -> Self {
        Self {
            prev_val: Default::default(),
            prev_val_d: Default::default(),
            prev_time: Instant::now(),
        }
    }
}

impl OneEuroFilter {
    pub(crate) fn reset(&mut self, reset_val: BaseNumT) {
        self.prev_val = reset_val;
        self.prev_val_d = reset_val;
    }

    pub(crate) fn new(val: BaseNumT, now: Instant) -> Self {
        Self {
            prev_val: val,
            prev_val_d: 0.0,
            prev_time: now,
        }
    }

    #[allow(unused)]
    pub(crate) fn get_dx_prev(&self) -> BaseNumT {
        self.prev_val_d
    }

    pub(crate) fn filter(
        &mut self,
        val: BaseNumT,
        now: Instant,
        min_cutoff: BaseNumT,
        beta: BaseNumT,
        d_cutoff: BaseNumT,
    ) -> BaseNumT {
        let dt = clamp_dt_by_min_and_max_period(now.duration_since(self.prev_time).as_secs_f32() as BaseNumT);

        debug_assert!(dt > 0.0 as BaseNumT);

        let val_d = (val - self.prev_val) / dt;
        let val_d_filtered = self.prev_val_d + Self::smoothing_factor(dt, d_cutoff) * (val_d - self.prev_val_d);

        let val_filtered = self.prev_val
            + Self::smoothing_factor(dt, min_cutoff + beta * val_d_filtered.abs()) * (val - self.prev_val);

        self.prev_val_d = val_d_filtered;
        self.prev_val = val_filtered;
        self.prev_time = now;

        val_filtered
    }

    fn smoothing_factor(dt: BaseNumT, cutoff: BaseNumT) -> BaseNumT {
        let tau = 1.0 as BaseNumT / ((2.0 * crate::base_num::BaseNumConsts::PI as BaseNumT * cutoff) as BaseNumT);
        1.0 / (1.0 + tau / dt)
    }
}
