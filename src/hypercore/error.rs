use std::fmt;

#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct ApiError(pub String);

#[derive(Debug, Clone)]
pub struct ActionError<T> {
    pub(crate) ids: Vec<T>,
    pub(crate) err: String,
}

impl<T> ActionError<T> {
    pub fn new(ids: Vec<T>, err: String) -> Self {
        Self { ids, err }
    }

    pub fn message(&self) -> &str {
        &self.err
    }

    pub fn ids(&self) -> &[T] {
        &self.ids
    }

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
