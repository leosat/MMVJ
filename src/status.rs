use std::ops::DerefMut;
use strum_macros::{Display, EnumIter, EnumString};

const DEFAULT_MAX_HAY_ENTRIES: usize = 5;

#[allow(unused)]
pub(crate) trait StatusIface {
    fn status_get_retention_policy(&self) -> StatusRetentionPolicy;
    fn status_get_kind(&self) -> StatusKind;
    fn status_is_actual(&self) -> bool;
    fn status_can_be_dropped(&self) -> bool;
    fn status_message_ref(&self) -> &str;
    fn status_acknowledge(&mut self) -> bool;
    fn status_set_retention_policy(&mut self, policy: StatusRetentionPolicy);
    fn status_set_kind(&mut self, kind: StatusKind);
}

pub(crate) trait WithStatus {
    type StatusT: StatusIface;
}

pub(crate) trait WithStatusAccessor: WithStatus {
    type StatusAccessorT<'s>
    where
        Self: 's;
    fn status_hay_accessor(&self) -> Self::StatusAccessorT<'_>;
}

#[allow(unused)]
pub(crate) trait WithStatusUtils: WithStatusAccessor {
    const MAX_HAY_ENTRIES: usize;
    fn set_status(&self, status: Self::StatusT, ensure_unique_message: bool, global_log_as: &'static [GlobalLogAs]);
    fn clear_status_hay(&self);
    fn clear_expired_status(&self);
}

// ----------------------------
#[derive(Default, Clone, Copy, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Display)]
pub(crate) enum StatusRetentionPolicy {
    #[default]
    Permanent,
    Temporary {
        expiring_at: std::time::Instant,
    },
    #[allow(unused)]
    Interactive {
        acknowledged: bool,
        post_ack_timeout: std::time::Duration,
    },
}

#[allow(unused)]
impl StatusRetentionPolicy {
    pub(crate) fn is_permanent(&self) -> bool {
        matches!(self, Self::Permanent)
    }
    pub(crate) fn is_temporary(&self) -> bool {
        matches!(self, Self::Temporary { .. })
    }
    pub(crate) fn is_interactive(&self) -> bool {
        matches!(self, Self::Interactive { .. })
    }
    pub(crate) fn is_actual(&self) -> bool {
        !self.can_be_dropped()
    }
    pub(crate) fn can_be_dropped(&self) -> bool {
        match self {
            StatusRetentionPolicy::Permanent => false,
            StatusRetentionPolicy::Temporary { expiring_at } => *expiring_at <= std::time::Instant::now(),
            StatusRetentionPolicy::Interactive {
                acknowledged,
                post_ack_timeout,
            } => *acknowledged && post_ack_timeout.is_zero(),
        }
    }
}

#[derive(Default, Clone, Copy, Debug, PartialOrd, Ord, PartialEq, Eq, Hash, Display, EnumIter, EnumString)]
pub(crate) enum StatusKind {
    #[default]
    Healthy,
    Misconfigured,
    Malfunctioning,
    AttentionRequired,
}

// -----------------
#[derive(Default, Clone, Debug, PartialOrd, Ord, PartialEq, Eq, Hash)]
pub(crate) struct Status {
    kind: StatusKind,
    retention_policy: StatusRetentionPolicy,
    message: String,
}

impl Status {
    #[allow(unused)]
    #[must_use]
    pub(crate) fn make_healhty_kind(mut self) -> Self {
        self.kind = StatusKind::Healthy;
        self
    }

    #[allow(unused)]
    #[must_use]
    pub(crate) fn make_misconfigured_kind(mut self) -> Self {
        self.kind = StatusKind::Misconfigured;
        self
    }

    #[allow(unused)]
    #[must_use]
    pub(crate) fn make_malfunctioning_kind(mut self) -> Self {
        self.kind = StatusKind::Malfunctioning;
        self
    }

    #[allow(unused)]
    #[must_use]
    pub(crate) fn make_attention_required_kind(mut self) -> Self {
        self.kind = StatusKind::AttentionRequired;
        self
    }

    #[allow(unused)]
    #[must_use]
    pub(crate) fn make_permanent(mut self) -> Self {
        self.retention_policy = StatusRetentionPolicy::Permanent;
        self
    }

    #[must_use]
    #[allow(unused)]
    pub(crate) fn make_temporary(mut self, timeout: std::time::Duration) -> Self {
        self.retention_policy = StatusRetentionPolicy::Temporary {
            expiring_at: std::time::Instant::now() + timeout,
        };
        self
    }

    #[must_use]
    #[allow(unused)]
    pub(crate) fn make_interactive(mut self, post_ack_timeout: std::time::Duration) -> Self {
        self.retention_policy = StatusRetentionPolicy::Interactive {
            acknowledged: false,
            post_ack_timeout,
        };
        self
    }

    #[allow(unused)]
    #[must_use]
    pub(crate) fn with_message(mut self, message: &str) -> Self {
        self.message = message.into();
        self
    }

    pub(crate) fn new(kind: StatusKind, retention_policy: StatusRetentionPolicy, message: String) -> Self {
        Self {
            kind,
            retention_policy,
            message,
        }
    }
}

impl StatusIface for Status {
    fn status_get_retention_policy(&self) -> StatusRetentionPolicy {
        self.retention_policy
    }

    fn status_set_retention_policy(&mut self, policy: StatusRetentionPolicy) {
        self.retention_policy = policy;
    }

    fn status_message_ref(&self) -> &str {
        &self.message
    }

    fn status_get_kind(&self) -> StatusKind {
        self.kind
    }

    fn status_set_kind(&mut self, kind: StatusKind) {
        self.kind = kind;
    }

    fn status_acknowledge(&mut self) -> bool {
        if let StatusRetentionPolicy::Interactive {
            acknowledged,
            post_ack_timeout,
        } = &mut self.retention_policy
        {
            if post_ack_timeout.is_zero() {
                *acknowledged = true;
                true
            } else {
                *self = Self::new(
                    self.kind,
                    StatusRetentionPolicy::Temporary {
                        expiring_at: std::time::Instant::now() + *post_ack_timeout,
                    },
                    self.status_message_ref().into(),
                );
                false
            }
        } else {
            false
        }
    }

    fn status_is_actual(&self) -> bool {
        self.retention_policy.is_actual()
    }

    fn status_can_be_dropped(&self) -> bool {
        self.retention_policy.can_be_dropped()
    }
}

// -------------------
//

#[derive(Default, Clone, Debug)]
pub(crate) struct StatusHayShared(pub(crate) std::sync::Arc<std::sync::Mutex<StatusHay>>);
pub(crate) type StatusHaySharedAccessor<'s> = std::sync::MutexGuard<'s, StatusHay>;
pub(crate) type StatusHay = Vec<crate::status::Status>;

impl PartialEq for StatusHayShared {
    fn eq(&self, _other: &Self) -> bool {
        false
    }
}

#[derive(Default, Clone, Copy)]
pub(crate) enum GlobalLogAs {
    #[default]
    None,
    #[allow(unused)]
    Warn,
    #[allow(unused)]
    Err,
    #[allow(unused)]
    ConsoleRaw,
}

// ------------------
impl<T> WithStatusUtils for T
where
    T: WithStatusAccessor<StatusT = Status>,
    for<'a> Self::StatusAccessorT<'a>: DerefMut<Target = StatusHay>,
    // or, limiting to mutex guard, for<'a> T: 'a + WithStatusAccessor<StatusT = Status, StatusAccessorT<'a> = StatusHaySharedAccessor<'a>>,
{
    const MAX_HAY_ENTRIES: usize = DEFAULT_MAX_HAY_ENTRIES;
    fn set_status(&self, status: Self::StatusT, ensure_unique_message: bool, global_log_as: &'static [GlobalLogAs]) {
        let mut hay = self.status_hay_accessor();

        if ensure_unique_message && hay.iter().find(|s| s.message == status.message).is_some() {
            return;
        }

        // if hay is full uniqueness is not guaranteed, still it's better to log/display the message.
        for log_as in global_log_as {
            match log_as {
                GlobalLogAs::None => {}
                GlobalLogAs::Warn => log::warn!("{}: {}", status.kind, status.message),
                GlobalLogAs::Err => log::error!("{}: {}", status.kind, status.message),
                GlobalLogAs::ConsoleRaw => println!("{}: {}", status.kind, status.message),
            }
        }

        if hay.len() >= Self::MAX_HAY_ENTRIES / 2 {
            hay.retain(|s| !s.retention_policy.can_be_dropped())
        }

        if hay.len() < Self::MAX_HAY_ENTRIES {
            hay.push(status);
        }
    }

    fn clear_status_hay(&self) {
        self.status_hay_accessor().clear();
    }

    fn clear_expired_status(&self) {
        self.status_hay_accessor()
            .retain(|s| !s.retention_policy.can_be_dropped());
    }
}

// ------------------
#[macro_export]
macro_rules! implement_status_for {
    ($type:path, $($tail:tt)+) => {
        impl WithStatus for $type {
            type StatusT = Status;
        }

        impl WithStatusAccessor for $type {
            type StatusAccessorT<'s> = StatusHaySharedAccessor<'s>;
            fn status_hay_accessor(&self) -> Self::StatusAccessorT<'_> {
                self$( $tail )+.0.lock().unwrap()
            }
        }
    };
}

#[cfg(test)]
#[test]
fn test_status() {
    const MAX_EXPECTED_STATUS_EXPIRATION_TIMEOUT_SEC: u64 = 5;
    use crate::schemas_transform::TfmSeqCfg;

    testing_logger::setup();

    let obj = TfmSeqCfg::default();
    let num_statuses = set_test_statuses_for(&obj);

    assert!(obj.status_hay_accessor().iter().len() == num_statuses);
    std::thread::sleep(std::time::Duration::from_secs(
        MAX_EXPECTED_STATUS_EXPIRATION_TIMEOUT_SEC,
    ));
    obj.clear_expired_status();
    assert!(obj.status_hay_accessor().iter().len() < num_statuses);
    testing_logger::validate(|captured_logs| {
        let warnings = captured_logs
            .iter()
            .filter(|c| c.level == log::Level::Warn)
            .collect::<Vec<&testing_logger::CapturedLog>>();
        let errors = captured_logs
            .iter()
            .filter(|c| c.level == log::Level::Error)
            .collect::<Vec<&testing_logger::CapturedLog>>();

        assert_eq!(warnings.len(), 1);
        assert_eq!(errors.len(), 1);
        let msg = format!("{}: Everything is Ok... I am expiring...!", StatusKind::Healthy);
        assert_eq!(warnings[0].body, msg);
        assert_eq!(errors[0].body, msg);
    });
}

#[allow(unused)]
pub(crate) fn set_test_statuses_for(this: &impl WithStatusUtils<StatusT = Status>) -> usize {
    let mut num_statuses = 0;

    let mut set_status = |status, global_log| {
        this.set_status(status, true, global_log);
        num_statuses += 1;
    };

    set_status(
        Status::default()
            .make_attention_required_kind()
            .make_interactive(std::time::Duration::ZERO)
            .with_message("I am interactive, press acknowledge and I will immediately disappear."),
        Default::default(),
    );

    set_status(
        Status::default()
            .make_attention_required_kind()
            .make_interactive(std::time::Duration::from_secs(2))
            .with_message("Everything is Ok! Disappearing after 2 seconds after acknowledged."),
        &[GlobalLogAs::ConsoleRaw],
    );

    set_status(
        Status::default()
            .make_healhty_kind()
            .make_temporary(std::time::Duration::from_secs(4))
            .with_message("Everything is Ok... I am expiring...!"),
        &[GlobalLogAs::ConsoleRaw, GlobalLogAs::Err, GlobalLogAs::Warn],
    );

    num_statuses
}
