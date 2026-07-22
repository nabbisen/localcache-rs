use super::*;

#[test]
fn missing_utf8_path_uses_the_exact_supplied_key() {
    let path = Path::new("relative/missing.txt");
    let resolved = resolve_path_key(path).unwrap();
    assert!(!resolved.exists());
    assert_eq!(resolved.path(), path);
    assert_eq!(resolved.key(), "relative/missing.txt");
}

#[cfg(unix)]
#[test]
fn missing_non_utf8_path_is_rejected() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = PathBuf::from(OsString::from_vec(vec![b'x', 0xff]));
    assert!(matches!(
        resolve_path_key(&path),
        Err(LocalFileCacheError::InvalidPath { path: rejected }) if rejected == path
    ));
}
