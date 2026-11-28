//! Why there is no result.

use crate::grid::Place;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Failed {
    /// There is none for this input.
    Unavailable(Reason),
    /// The caller's check said so.
    Cancelled,
    /// A fault of the crate itself: a panic in it, or in the caller's check.
    Internal,
}

/// The caller's check: whether to stop. It is asked between pieces of work, on any thread.
pub type Cancel<'a> = &'a (dyn Fn() -> bool + Sync);

/// `f`, whose panics are [`Failed::Internal`].
pub fn guarded<T>(f: impl FnOnce() -> Result<T, Failed>) -> Result<T, Failed> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or(Err(Failed::Internal))
}

pub fn unavailable<T>(reason: Reason) -> Result<T, Failed> {
    Err(Failed::Unavailable(reason))
}

/// [`Failed::Cancelled`] when `cancel` says so.
pub fn go_on(cancel: Cancel) -> Result<(), Failed> {
    if cancel() {
        Err(Failed::Cancelled)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
#[non_exhaustive]
pub enum Reason {
    NoSeeds,
    /// No point of the grid is in the air and in sight of a seed.
    NothingReached { seeds: usize },
    /// More points would be gathered than the limit.
    TooManyPoints { limit: usize },
    /// A number of the named input is not finite, or out of its range.
    Input(&'static str),
    /// More of the named than there may be.
    TooMany { what: &'static str, limit: usize },
    /// The texture would be larger than a side may be.
    TooLarge { dimensions: [u32; 3] },
    /// The texture would take more bytes than the limit.
    TooManyBytes { bytes: u64, limit: u64 },
    /// The light at a point is not in a half float's range.
    OutOfRange { at: Place },
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Reason::NoSeeds => write!(f, "no seeds"),
            Reason::NothingReached { seeds } => write!(f, "no point of the grid is reached from {seeds} seeds"),
            Reason::TooManyPoints { limit } => write!(f, "more than {limit} points"),
            Reason::Input(what) => write!(f, "a number of the {what} is out of its range"),
            Reason::TooMany { what, limit } => write!(f, "more than {limit} {what}"),
            Reason::TooLarge { dimensions } => write!(f, "{dimensions:?} points: the texture would be too large"),
            Reason::TooManyBytes { bytes, limit } => write!(f, "a texture of {bytes} bytes, {limit} may be"),
            Reason::OutOfRange { at } => write!(f, "the light at {at:?} is not in a half float's range"),
        }
    }
}

impl fmt::Display for Failed {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Failed::Unavailable(reason) => write!(f, "unavailable: {reason}"),
            Failed::Cancelled => write!(f, "cancelled"),
            Failed::Internal => write!(f, "a fault in the crate"),
        }
    }
}

impl std::error::Error for Failed {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panics_are_failures() {
        assert_eq!(guarded::<()>(|| panic!("expected here")), Err(Failed::Internal));
        assert_eq!(guarded(|| Ok(3)), Ok(3));
        assert!(guarded(|| go_on(&|| true)) == Err(Failed::Cancelled) && go_on(&|| false).is_ok());
    }
}
