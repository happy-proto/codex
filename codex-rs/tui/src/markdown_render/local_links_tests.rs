//! Tests for local-link parsing and root-preserving separator normalization.

use super::trim_trailing_local_path_separator;
use pretty_assertions::assert_eq;

#[test]
#[cfg(unix)]
fn explicit_file_urls_resolve_paths_without_including_locations() {
    let cwd = Some(std::path::Path::new("/workspace"));
    for (target, expected) in [
        ("/workspace/a%20b.rs:12:3", "file:///workspace/a%20b.rs"),
        ("./src/file.rs#L12", "file:///workspace/src/file.rs"),
        (
            "file:///workspace/a%20b.rs#L12",
            "file:///workspace/a%20b.rs",
        ),
        (
            "/workspace/file.rs#literal",
            "file:///workspace/file.rs%23literal",
        ),
    ] {
        assert_eq!(
            super::local_link_url(target, cwd).unwrap().as_str(),
            expected
        );
    }
    assert!(super::local_link_url("./file.rs", None).is_none());
    assert!(super::local_link_url("/workspace/a%1Bb.rs", cwd).is_none());
    assert!(super::local_link_url("C:/workspace/file.rs", cwd).is_none());
}

#[test]
fn trailing_separator_trimming_preserves_local_roots() {
    let paths = ["/", "//", "C:/", "dir/", "//server/share/", "C:/dir/"];
    assert_eq!(
        paths.map(trim_trailing_local_path_separator),
        ["/", "//", "C:/", "dir", "//server/share", "C:/dir"]
    );
}
