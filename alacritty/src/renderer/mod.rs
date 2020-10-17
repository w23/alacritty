pub use glyph::*;
use shade::*;

mod atlas;
mod glyphrect;
mod grid;
mod math;
mod shade;
mod solidrect;
mod texture;

#[cfg(feature = "live-shader-reload")]
mod filewatch;

pub mod glyph;
pub mod rects;
pub mod simple;

#[derive(Debug)]
pub enum Error {
    ShaderCreation(ShaderCreationError),
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::ShaderCreation(err) => err.source(),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::ShaderCreation(err) => {
                write!(f, "There was an error initializing the shaders: {}", err)
            }
        }
    }
}

impl From<ShaderCreationError> for Error {
    fn from(val: ShaderCreationError) -> Self {
        Error::ShaderCreation(val)
    }
}
