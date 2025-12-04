use lasso::{Key, ThreadedRodeo};
use std::sync::{Arc, LazyLock};
static INTERNER: LazyLock<Arc<ThreadedRodeo>> = LazyLock::new(Default::default);

pub(crate) fn get_interned_str(id: usize) -> Option<&'static str> {
    let key = lasso::Spur::try_from_usize(id).unwrap_or_default();
    if INTERNER.contains_key(&key) {
        Some(INTERNER.resolve(&key))
    } else {
        None
    }
}

#[allow(unused)]
pub(crate) fn intern_str(str: &str) -> usize {
    INTERNER.get_or_intern(str).into_usize()
}
