use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    BadArgs,
    Api,
    OcrSchema,
    Epub,
    Validation,
    Internal,
}

impl ErrorKind {
    pub const fn exit_code(self) -> i32 {
        match self {
            Self::BadArgs => 2,
            Self::Api => 3,
            Self::OcrSchema => 4,
            Self::Epub => 5,
            Self::Validation => 6,
            Self::Internal => 1,
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("{message}")]
pub struct BaegunError {
    pub kind: ErrorKind,
    pub message: String,
}

impl BaegunError {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn bad_args(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::BadArgs, message)
    }

    pub fn api(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Api, message)
    }

    pub fn ocr_schema(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::OcrSchema, message)
    }

    pub fn epub(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Epub, message)
    }

    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Validation, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ErrorKind::Internal, message)
    }
}

pub type Result<T> = std::result::Result<T, BaegunError>;
