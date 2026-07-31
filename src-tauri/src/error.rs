use serde::Serialize;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ErrorPayload {
    pub code: &'static str,
    pub message: String,
    pub recoverable: bool,
}

#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("{0}")]
    InvalidInput(String),
    #[error("{0}")]
    NotFound(String),
    #[error("{0}")]
    PermissionDenied(String),
    #[error("{0}")]
    Conflict(String),
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Platform(String),
    #[error("{0}")]
    Task(String),
    #[error("任务已取消。")]
    Cancelled,
}

impl AppError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput(_) => "invalid_input",
            Self::NotFound(_) => "not_found",
            Self::PermissionDenied(_) => "permission_denied",
            Self::Conflict(_) => "conflict",
            Self::Io(_) => "io_error",
            Self::Json(_) => "invalid_data",
            Self::Platform(_) => "platform_error",
            Self::Task(_) => "task_error",
            Self::Cancelled => "cancelled",
        }
    }

    pub fn recoverable(&self) -> bool {
        !matches!(self, Self::InvalidInput(_) | Self::Platform(_))
    }
}

impl Serialize for AppError {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        ErrorPayload {
            code: self.code(),
            message: self.to_string(),
            recoverable: self.recoverable(),
        }
        .serialize(serializer)
    }
}

pub type AppResult<T> = Result<T, AppError>;
