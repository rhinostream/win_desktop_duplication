#[derive(Debug)]
pub enum DDApiError {
    Disconnected,
    Unsupported,
    AccessDenied,
    AccessLost,
    BadParam(String),
    Unexpected(String),
}