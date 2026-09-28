// NOTE: If you add a variant here, you MUST also update the manual
// typedef enum definition in build.rs (generate_header fn).
#[repr(i32)]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LustroError {
    Ok = 0,
    InvalidLength = 1,
    InvalidPointer = 2,
    OutputTooSmall = 3,
    AlreadyFinalised = 4,
    VerificationFailed = 5,
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
