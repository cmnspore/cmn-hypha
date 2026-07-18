use substrate::CmnEndpoint;

pub(super) fn dist_git_url(entry: &substrate::SporeDist) -> Option<&str> {
    entry.git_url()
}

pub(super) fn dist_git_ref(entry: &substrate::SporeDist) -> Option<&str> {
    entry.git_ref()
}

pub(super) fn dist_has_type(entry: &substrate::SporeDist, expected: &str) -> bool {
    entry.kind.as_str() == expected
}

pub(super) fn build_archive_url_from_endpoint(
    endpoint: &CmnEndpoint,
    hash: &str,
) -> Result<String, crate::HyphaError> {
    endpoint.resolve_url(hash).map_err(|e| {
        crate::HyphaError::new(
            "url_error",
            format!(
                "Invalid archive endpoint for format {:?}: {}",
                endpoint.format, e
            ),
        )
    })
}

pub(super) fn build_archive_delta_url_from_endpoint(
    endpoint: &CmnEndpoint,
    hash: &str,
    old_hash: &str,
) -> Result<Option<String>, crate::HyphaError> {
    endpoint.resolve_delta_url(hash, old_hash).map_err(|e| {
        crate::HyphaError::new(
            "url_error",
            format!(
                "Invalid archive delta endpoint for format {:?}: {}",
                endpoint.format, e
            ),
        )
    })
}

/// Validate a bond directory segment used under `.cmn/bonds/`.
pub(super) fn is_safe_bond_dir_name(name: &str) -> bool {
    substrate::is_safe_local_path_segment(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bond_dir_validation_checks_safety_without_requiring_canonical_slug_format() {
        assert!(is_safe_bond_dir_name("Foo"));
        assert!(is_safe_bond_dir_name("foo_bar"));
        assert!(is_safe_bond_dir_name("b3.hash"));

        assert!(!is_safe_bond_dir_name(""));
        assert!(!is_safe_bond_dir_name(".."));
        assert!(!is_safe_bond_dir_name("bad/name"));
        assert!(!is_safe_bond_dir_name("bad\\name"));
        assert!(!is_safe_bond_dir_name("bad name"));
        assert!(!is_safe_bond_dir_name("bad\x01name"));
    }
}
