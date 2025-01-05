#[derive(Debug)]
pub enum DDApiError {
    Disconnected,
    Unsupported,
    AccessDenied,
    AccessLost,
    CursorNotAvailable,
    BadParam(String),
    Unexpected(String),
}