use serde::ser::SerializeStruct;
use serde::Serialize;

/// Errors crossing the IPC boundary are reduced to a stable `code` (translated
/// by the UI) plus a few safe parameters; internal details only go to stderr.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("not found: {0}")]
    NotFound(String),
    #[error("already installed")]
    AlreadyInstalled,
    #[error("operation already in progress")]
    Busy,
    #[error("port {port} in use")]
    PortInUse { port: u16, suggestion: Option<u16> },
    #[error("service is running")]
    ServiceRunning,
    #[error("module has no service")]
    NoService,
    #[error("no build for this platform")]
    UnsupportedPlatform,
    #[error("registry signature verification failed: {0}")]
    Signature(String),
    #[error("checksum mismatch")]
    Checksum,
    #[error("unsafe archive entry: {0}")]
    UnsafeArchive(String),
    #[error("network error: {0}")]
    Network(String),
    #[error("failed to start process: {0}")]
    Spawn(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    #[error("internal error: {0}")]
    Internal(String),
}

pub type AppResult<T> = Result<T, AppError>;

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::NotFound(_) => "NOT_FOUND",
            Self::AlreadyInstalled => "ALREADY_INSTALLED",
            Self::Busy => "BUSY",
            Self::PortInUse { .. } => "PORT_IN_USE",
            Self::ServiceRunning => "SERVICE_RUNNING",
            Self::NoService => "NO_SERVICE",
            Self::UnsupportedPlatform => "UNSUPPORTED_PLATFORM",
            Self::Signature(_) => "SIGNATURE",
            Self::Checksum => "CHECKSUM",
            Self::UnsafeArchive(_) => "UNSAFE_ARCHIVE",
            Self::Network(_) => "NETWORK",
            Self::Spawn(_) => "SPAWN",
            Self::Io(_) | Self::Internal(_) => "INTERNAL",
        }
    }
}

impl From<reqwest::Error> for AppError {
    fn from(e: reqwest::Error) -> Self {
        Self::Network(e.to_string())
    }
}

impl From<serde_json::Error> for AppError {
    fn from(e: serde_json::Error) -> Self {
        Self::Internal(e.to_string())
    }
}

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        eprintln!("[stackyard] {self}");
        let mut st = s.serialize_struct("AppError", 3)?;
        st.serialize_field("code", self.code())?;
        match self {
            Self::PortInUse { port, suggestion } => {
                st.serialize_field("port", port)?;
                st.serialize_field("suggestion", suggestion)?;
            }
            Self::NotFound(what) | Self::InvalidInput(what) => {
                st.serialize_field("detail", what)?;
            }
            _ => {}
        }
        st.end()
    }
}
