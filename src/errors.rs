//! Lustro V1 — error codes of the C ABI.

// NOTE: If you add a variant here, you MUST also update the manual
// typedef enum definition in build.rs (generate_header fn).
/// Status code returned by the C ABI. Values are fixed.
#[repr(i32)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LustroError {
    /// Success.
    Ok = 0,
    /// Length or size argument is invalid.
    InvalidLength = 1,
    /// Required pointer is NULL or misaligned (hash functions: ranges overlap).
    InvalidPointer = 2,
    /// Reserved, not returned by V1 (`InvalidLength` is used instead).
    OutputTooSmall = 3,
    /// Reserved, not returned by V1.
    AlreadyFinalised = 4,
    /// Reserved, not returned by V1.
    VerificationFailed = 5,
    /// Panic caught at the FFI boundary.
    InternalPanic = 6,
}

impl core::fmt::Display for LustroError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            LustroError::Ok => write!(f, "ok"),
            LustroError::InvalidLength => write!(f, "invalid input length"),
            LustroError::InvalidPointer => write!(f, "invalid pointer"),
            LustroError::OutputTooSmall => write!(f, "output buffer too small"),
            LustroError::AlreadyFinalised => write!(f, "context already finalised"),
            LustroError::VerificationFailed => write!(f, "verification failed"),
            LustroError::InternalPanic => write!(f, "internal panic caught at FFI boundary"),
        }
    }
}
