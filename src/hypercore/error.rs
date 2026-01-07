//! Error types for HyperCore operations.
//!
//! This module provides structured error types for all HyperCore operations,
//! making it easier to handle specific error cases programmatically.

use std::fmt;

use alloy::signers::Error as SignerError;

/// Error type for HyperCore operations.
///
/// Covers all error cases that can occur when interacting with the Hyperliquid API,
/// from network issues to API rejections to signing failures.
#[derive(Debug)]
pub enum Error {
    /// Network or HTTP transport error.
    ///
    /// This includes connection failures, timeouts, DNS resolution errors, etc.
    Network(reqwest::Error),

    /// API returned an error response.
    ///
    /// The exchange rejected the request with an error message.
    /// Common causes: invalid parameters, insufficient balance, rate limiting.
    Api(String),

    /// Failed to serialize or deserialize JSON data.
    ///
    /// This usually indicates a mismatch between the SDK and API versions,
    /// or malformed data from the server.
    Json(serde_json::Error),

    /// Failed to sign a transaction or action.
    ///
    /// This can occur if the private key is invalid or signing fails.
    Signing(SignerError),

    /// Invalid order parameters.
    ///
    /// The order price, size, or other parameters don't meet exchange requirements.
    /// Common causes: price not on tick, size below minimum, leverage too high.
    InvalidOrder {
        /// Description of what's wrong with the order
        message: String,
    },

    /// WebSocket connection error.
    ///
    /// Failed to establish or maintain WebSocket connection for real-time data.
    WebSocket(String),

    /// Invalid address format.
    ///
    /// The provided Ethereum address is not valid hex or has wrong checksum.
    InvalidAddress(String),

    /// Timeout waiting for a response.
    ///
    /// The operation took too long and was cancelled.
    Timeout,

    /// Other error not covered by specific variants.
    ///
    /// This is a catch-all for unexpected errors. If you see this frequently,
    /// please report it as it may warrant its own variant.
    Other(String),
}

impl Error {
    /// Returns true if this error is retryable.
    ///
    /// Network timeouts and transient errors may succeed on retry.
    /// API rejections and validation errors should not be retried.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use hypersdk::hypercore::Error;
    /// # fn example(err: Error) {
    /// if err.is_retryable() {
    ///     // Retry with exponential backoff
    /// } else {
    ///     // Log and fail permanently
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        matches!(
            self,
            Error::Network(_) | Error::Timeout | Error::WebSocket(_)
        )
    }

    /// Returns true if this is a network-related error.
    #[must_use]
    pub fn is_network_error(&self) -> bool {
        matches!(self, Error::Network(_) | Error::Timeout)
    }

    /// Returns true if this is an API rejection.
    #[must_use]
    pub fn is_api_error(&self) -> bool {
        matches!(self, Error::Api(_))
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Network(e) => write!(f, "Network error: {}", e),
            Error::Api(e) => write!(f, "API error: {}", e),
            Error::Json(e) => write!(f, "JSON error: {}", e),
            Error::Signing(e) => write!(f, "Signing error: {}", e),
            Error::InvalidOrder { message } => write!(f, "Invalid order: {}", message),
            Error::WebSocket(e) => write!(f, "WebSocket error: {}", e),
            Error::InvalidAddress(e) => write!(f, "Invalid address: {}", e),
            Error::Timeout => write!(f, "Operation timed out"),
            Error::Other(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Network(e) => Some(e),
            Error::Json(e) => Some(e),
            Error::Signing(e) => Some(e),
            _ => None,
        }
    }
}

// Conversion implementations for ergonomic error handling

impl From<reqwest::Error> for Error {
    fn from(e: reqwest::Error) -> Self {
        if e.is_timeout() {
            Error::Timeout
        } else {
            Error::Network(e)
        }
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Json(e)
    }
}

impl From<SignerError> for Error {
    fn from(e: SignerError) -> Self {
        Error::Signing(e)
    }
}

impl From<url::ParseError> for Error {
    fn from(e: url::ParseError) -> Self {
        Error::Other(format!("URL parse error: {}", e))
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::Other(format!("IO error: {}", e))
    }
}

// Allow converting anyhow errors to our error type for compatibility
impl From<anyhow::Error> for Error {
    fn from(e: anyhow::Error) -> Self {
        Error::Other(e.to_string())
    }
}

/// Error type for batch operations that failed.
///
/// Contains the IDs of the orders/actions that failed and the error message.
///
/// # Type Parameter
///
/// - `T`: The ID type (e.g., `Cloid`, `u64`, `OidOrCloid`)
///
/// # Example
///
/// ```rust
/// use hypersdk::hypercore::ActionError;
///
/// fn handle_batch_error(err: ActionError<u64>) {
///     println!("Failed order IDs: {:?}", err.ids());
///     println!("Error: {}", err.message());
/// }
/// ```
#[derive(Debug, Clone)]
pub struct ActionError<T> {
    /// The IDs of orders/actions that encountered the error
    pub(crate) ids: Vec<T>,
    /// The error message from the exchange
    pub(crate) err: String,
}

impl<T> ActionError<T> {
    /// Creates a new ActionError.
    pub fn new(ids: Vec<T>, err: String) -> Self {
        Self { ids, err }
    }

    /// Returns the error message.
    pub fn message(&self) -> &str {
        &self.err
    }

    /// Returns the failed IDs.
    pub fn ids(&self) -> &[T] {
        &self.ids
    }

    /// Consumes the error and returns the IDs.
    pub fn into_ids(self) -> Vec<T> {
        self.ids
    }
}

impl<T> fmt::Display for ActionError<T>
where
    T: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}, ids: {:?}", self.err, self.ids)
    }
}

impl<T> std::error::Error for ActionError<T> where T: fmt::Display + fmt::Debug {}
