#[derive(Debug)]
pub enum DDApiError {
    Disconnected,
    Unsupported,
    AccessDenied,
    AccessLost,
    TimeOut,
    CursorNotAvailable,
    BadParam(String),
    Unexpected(String),
}