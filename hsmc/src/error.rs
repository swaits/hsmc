/// Errors returned by the `hsmc` runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HsmcError {
    /// The event queue is full. The event was not enqueued.
    QueueFull,
    /// The machine has already terminated. No further operations are valid.
    AlreadyTerminated,
}

impl core::fmt::Display for HsmcError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            HsmcError::QueueFull => f.write_str("event queue is full"),
            HsmcError::AlreadyTerminated => f.write_str("machine has already terminated"),
        }
    }
}

#[cfg(feature = "tokio")]
impl std::error::Error for HsmcError {}

#[cfg(test)]
mod tests {
    use super::*;
    extern crate alloc;
    use alloc::format;

    // Kills: <impl Display for HsmcError>::fmt -> Ok(Default::default())
    #[test]
    fn display_messages_are_specific() {
        assert_eq!(format!("{}", HsmcError::QueueFull), "event queue is full");
        assert_eq!(
            format!("{}", HsmcError::AlreadyTerminated),
            "machine has already terminated"
        );
    }
}
