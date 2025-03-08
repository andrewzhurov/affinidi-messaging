use std::fmt::Debug;
use tracing::debug;
pub trait Debuggable: Sized + Debug {
    fn dbg(self) -> Self {
        debug!("{:?}", self);
        self
    }
}

impl<T> Debuggable for T where T: Sized + Debug {}
