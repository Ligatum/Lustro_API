//! Lustro V1 — error codes of the C ABI.

// NOTE: If you add a variant here, you MUST also update the manual
// typedef enum definition in build.rs (generate_header fn).
/// Status code returned by the C ABI. Values are fixed.
#[repr(i32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

impl LustroError {
    /// Maps a raw ABI value back to its variant.
    pub const fn from_code(code: i32) -> Option<Self> {
        match code {
            0 => Some(LustroError::Ok),
            1 => Some(LustroError::InvalidLength),
            2 => Some(LustroError::InvalidPointer),
            3 => Some(LustroError::OutputTooSmall),
            4 => Some(LustroError::AlreadyFinalised),
            5 => Some(LustroError::VerificationFailed),
            6 => Some(LustroError::InternalPanic),
            _ => None,
        }
    }

    /// Short description, same text as `Display`.
    pub fn as_str(&self) -> &'static str {
        let s = self.as_nul_str();
        &s[..s.len() - 1]
    }

    // Single source of the messages; the trailing NUL is for lustro_strerror().
    pub(crate) const fn as_nul_str(&self) -> &'static str {
        match self {
            LustroError::Ok => "ok\0",
            LustroError::InvalidLength => "invalid input length\0",
            LustroError::InvalidPointer => "invalid pointer\0",
            LustroError::OutputTooSmall => "output buffer too small\0",
            LustroError::AlreadyFinalised => "context already finalised\0",
            LustroError::VerificationFailed => "verification failed\0",
            LustroError::InternalPanic => "internal panic caught at FFI boundary\0",
        }
    }
}

impl core::fmt::Display for LustroError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::error::Error for LustroError {}
