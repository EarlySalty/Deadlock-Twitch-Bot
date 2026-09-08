mod dataset;
mod files;
mod runner;

pub use crate::hub::{EvalResponse, LocalClient};
pub use dataset::{Dataset, Variant};
pub use runner::{run, Config};

#[derive(Debug, Clone, Copy)]
pub struct EvalError(pub &'static str);
pub type Result<T> = std::result::Result<T, EvalError>;

impl std::fmt::Display for EvalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Replay-Fehler: {}", self.0)
    }
}
impl std::error::Error for EvalError {}
