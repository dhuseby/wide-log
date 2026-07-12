pub mod context;
pub mod error;
pub mod guard;
pub mod key;
pub(crate) mod log;
pub mod value;
pub mod wide_event;

#[cfg(feature = "tokio")]
pub mod middleware;

pub use error::Error;
pub use guard::WideEventGuard;
pub use key::Key;
pub use value::Value;
pub use wide_event::WideEvent;

pub use wide_log_macros::wide_log;

#[cfg(feature = "tokio")]
pub mod __re_exports {
    pub use tokio;
    pub use tower;
}