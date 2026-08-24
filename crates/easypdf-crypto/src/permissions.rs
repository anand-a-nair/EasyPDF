//! Document permission flags.

/// Permissions declared by an encrypted document.
///
/// **These are advisory.** Any tool holding the decryption key can ignore them,
/// and so could this one. EasyPDF honors them; the documentation says plainly
/// that they are not a security control. Presenting them as enforcement would
/// be dishonest — see `ideas/06-features.md`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Permissions {
    /// Printing is permitted.
    pub print: bool,
    /// High-resolution printing is permitted.
    pub print_high_quality: bool,
    /// Content modification is permitted.
    pub modify: bool,
    /// Text and graphics extraction is permitted.
    pub copy: bool,
    /// Annotation and form filling are permitted.
    pub annotate: bool,
    /// Filling existing form fields is permitted.
    pub fill_forms: bool,
    /// Extraction for accessibility is permitted.
    ///
    /// Defaults to allowed regardless of other flags: denying assistive
    /// technology is a use of the flag we will not honor.
    pub accessibility: bool,
    /// Page assembly (insert, rotate, delete) is permitted.
    pub assemble: bool,
}

impl Default for Permissions {
    /// Everything permitted — the correct default for an unencrypted document.
    fn default() -> Self {
        Self {
            print: true,
            print_high_quality: true,
            modify: true,
            copy: true,
            annotate: true,
            fill_forms: true,
            accessibility: true,
            assemble: true,
        }
    }
}

impl Permissions {
    /// The most restrictive set a document can declare.
    ///
    /// Accessibility stays enabled deliberately.
    #[must_use]
    pub fn restricted() -> Self {
        Self {
            print: false,
            print_high_quality: false,
            modify: false,
            copy: false,
            annotate: false,
            fill_forms: false,
            accessibility: true,
            assemble: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_permits_everything() {
        let p = Permissions::default();
        assert!(p.print && p.modify && p.copy && p.annotate && p.assemble);
    }

    #[test]
    fn accessibility_survives_maximum_restriction() {
        // A deliberate policy choice: we do not honor a flag whose only effect
        // is to deny assistive technology.
        assert!(Permissions::restricted().accessibility);
    }

    #[test]
    fn restricted_denies_the_rest() {
        let p = Permissions::restricted();
        assert!(!p.print && !p.modify && !p.copy && !p.assemble);
    }
}
