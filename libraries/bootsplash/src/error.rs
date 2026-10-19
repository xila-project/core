#[derive(Debug)]
pub enum Error {
    Graphics(graphics::Error),
}

impl From<graphics::Error> for Error {
    fn from(error: graphics::Error) -> Self {
        Error::Graphics(error)
    }
}

pub type Result<T> = core::result::Result<T, Error>;
