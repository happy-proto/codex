/// The running package's release version, resolved without invalidating shared crates.
#[cfg(not(test))]
pub(crate) fn cli_version() -> &'static str {
    codex_build_info::BuildInfo::package_version()
}

// Keep UI fixtures independent of the alpha version assigned during packaging.
#[cfg(test)]
pub(crate) fn cli_version() -> &'static str {
    "0.0.0"
}
