//! Admitted license list.

/// SPDX identifiers admitted by default. Every entry is an OSI-approved
/// license. Models with custom "open-weight" terms are not admitted here.
/// Changing this list is a policy decision.
pub const ALLOWED_SPDX_LICENSES: &[&str] = &[
    "Apache-2.0",
    "BSD-2-Clause",
    "BSD-3-Clause",
    "ISC",
    "MIT",
    "MPL-2.0",
];

/// Returns true when `spdx` is exactly one of the admitted identifiers.
/// Matching is case-sensitive and does not parse expressions.
pub fn is_allowed_license(spdx: &str) -> bool {
    ALLOWED_SPDX_LICENSES.contains(&spdx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_identifiers_are_admitted() {
        assert!(is_allowed_license("Apache-2.0"));
        assert!(is_allowed_license("MIT"));
    }

    #[test]
    fn custom_terms_and_other_spellings_are_refused() {
        assert!(!is_allowed_license("apache-2.0"));
        assert!(!is_allowed_license("Llama-3.2-Community"));
        assert!(!is_allowed_license("Gemma-Terms"));
        assert!(!is_allowed_license("MIT OR Apache-2.0"));
        assert!(!is_allowed_license(""));
    }
}
