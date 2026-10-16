use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("could not encode a web message")]
    Encode(#[from] bincode::error::EncodeError),
    #[error("could not decode a web message")]
    Decode(#[from] bincode::error::DecodeError),
    #[error("a web message had {0} trailing bytes")]
    Trailing(usize),
    #[error("web server I/O failed")]
    Io(#[from] std::io::Error),
    #[error("could not listen on 127.0.0.1:{port}")]
    Bind {
        port: u16,
        #[source]
        source: std::io::Error,
    },
    #[error("the web client is not built: {path} is missing; run `just web-build`")]
    MissingClient {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("serving the app to a browser needs macOS or Linux")]
    Unsupported,
    #[error("could not read random bytes for the access token")]
    Random,
    #[error("the browser sent a malformed request")]
    BadRequest,
    #[error("a WebSocket frame of {0} bytes is larger than the limit")]
    FrameTooLarge(u64),
    #[error("the browser sent an unmasked WebSocket frame")]
    Unmasked,
    #[error("the browser sent an unknown WebSocket opcode {0}")]
    Opcode(u8),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
