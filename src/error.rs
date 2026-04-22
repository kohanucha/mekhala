#![allow(dead_code)]

use std::fmt;

#[derive(Debug)]
pub enum Error {
    Config(String),
    Database(String),
    Whitelist(String),
    Server(String),
    Network(String),
    Parse(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Config(msg) => write!(f, "Config error: {}", msg),
            Error::Database(msg) => write!(f, "Database error: {}", msg),
            Error::Whitelist(msg) => write!(f, "Whitelist error: {}", msg),
            Error::Server(msg) => write!(f, "Server error: {}", msg),
            Error::Network(msg) => write!(f, "Network error: {}", msg),
            Error::Parse(msg) => write!(f, "Parse error: {}", msg),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::env::VarError> for Error {
    fn from(e: std::env::VarError) -> Self {
        Error::Config(e.to_string())
    }
}

impl From<sled::Error> for Error {
    fn from(e: sled::Error) -> Self {
        Error::Database(e.to_string())
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::Parse(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;