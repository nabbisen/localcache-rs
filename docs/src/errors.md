# Error Handling

All fallible operations return `Result<_, LocalFileCacheError>`.

```rust
#[derive(Debug, thiserror::Error)]
pub enum LocalFileCacheError {
    #[error("database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serialization error: {0}")]
    Serialization(#[from] Box<bincode::ErrorKind>),

    #[error("file does not exist: {path}")]
    FileNotFound { path: PathBuf },

    #[error("unsupported feature: {0}")]
    UnsupportedFeature(String),

    #[error("invalid path: {path}")]
    InvalidPath { path: PathBuf },
}
```

## Notes

- `LocalFileCacheError` implements `std::error::Error`, so it is compatible
  with `Box<dyn Error>` and crates such as `anyhow`.
- `rusqlite::Error` and `std::io::Error` are wrapped automatically via `From`.
- `FileNotFound` is returned when `set` or `get_if_fresh` is called for a
  path that does not exist on disk.  `get_if_fresh` converts this silently to
  `Ok(None)`.
- `InvalidPath` is returned for paths that cannot be represented as UTF-8
  strings.
