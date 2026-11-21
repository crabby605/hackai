#![forbid(unsafe_code)]
use serde::{Deserialize, Serialize};
use std::path::{Component, PathBuf};
use unicode_normalization::UnicodeNormalization;

#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CanonicalRoot(PathBuf);
#[derive(Clone, Debug, Eq, PartialEq, Hash)]
pub struct CanonicalCandidate(PathBuf);
impl CanonicalRoot {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }
}
impl CanonicalCandidate {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RelPath(String);
impl RelPath {
    pub fn parse(input: &str) -> Result<Self, PathError> {
        let normalized: String = input.nfc().collect();
        if normalized.is_empty()
            || normalized.starts_with('/')
            || normalized.starts_with('\\')
            || normalized.contains(':')
            || normalized.chars().any(|c| {
                c.is_control() || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            })
        {
            return Err(PathError::Invalid);
        }
        let pieces: Vec<_> = normalized.split('/').collect();
        if pieces
            .iter()
            .any(|p| p.is_empty() || *p == "." || *p == "..")
        {
            return Err(PathError::Invalid);
        }
        Ok(Self(normalized))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Hash, Serialize, Deserialize)]
pub enum ProjectPath {
    InProject(RelPath),
    OutsideProject,
}
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path is not a safe project-relative path")]
    Invalid,
}

pub fn strip_to_project(root: &CanonicalRoot, candidate: &CanonicalCandidate) -> ProjectPath {
    let Ok(stripped) = candidate.0.strip_prefix(&root.0) else {
        return ProjectPath::OutsideProject;
    };
    let mut parts = Vec::new();
    for part in stripped.components() {
        match part {
            Component::Normal(p) => match p.to_str() {
                Some(s) => parts.push(s),
                None => return ProjectPath::OutsideProject,
            },
            _ => return ProjectPath::OutsideProject,
        }
    }
    RelPath::parse(&parts.join("/"))
        .map(ProjectPath::InProject)
        .unwrap_or(ProjectPath::OutsideProject)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_name_is_not_prefix_match() {
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/x/foo".into()),
                &CanonicalCandidate::new("/x/foo-bar/a".into())
            ),
            ProjectPath::OutsideProject
        );
    }
    #[test]
    fn strips_components() {
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/x/foo".into()),
                &CanonicalCandidate::new("/x/foo/foo/a.rs".into())
            ),
            ProjectPath::InProject(RelPath::parse("foo/a.rs").unwrap_or_else(|_| unreachable!()))
        );
    }
    #[test]
    fn rejects_escape_and_bidi() {
        assert!(RelPath::parse("../a").is_err());
        assert!(RelPath::parse("a/\u{202e}x").is_err());
    }
    proptest::proptest! { #[test] fn accepted_paths_never_have_parent(s in "[a-zA-Z0-9._/-]{1,50}") { if let Ok(p) = RelPath::parse(&s) { proptest::prop_assert!(!p.as_str().split('/').any(|x| x == "..")); } } }

    #[test]
    fn windows_different_drive_is_outside_project() {
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new(r#"C:\proj"#.into()),
                &CanonicalCandidate::new(r#"D:\proj\main.rs"#.into())
            ),
            ProjectPath::OutsideProject
        );
    }

    #[test]
    fn wsl_different_drive_is_outside_project() {
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/mnt/c/proj".into()),
                &CanonicalCandidate::new("/mnt/d/proj/main.rs".into())
            ),
            ProjectPath::OutsideProject
        );
    }

    #[test]
    fn canonical_candidate_under_root_is_safe() {
        // Post-canonicalization (symlink already resolved): no filesystem I/O here.
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/home/user/proj".into()),
                &CanonicalCandidate::new("/home/user/proj/src/lib.rs".into())
            ),
            ProjectPath::InProject(RelPath::parse("src/lib.rs").unwrap_or_else(|_| unreachable!()))
        );
    }

    #[test]
    fn case_distinct_components() {
        // Foo and foo are distinct components; case-folding lives outside this crate.
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/x/Foo".into()),
                &CanonicalCandidate::new("/x/Foo/foo.txt".into())
            ),
            ProjectPath::InProject(RelPath::parse("foo.txt").unwrap_or_else(|_| unreachable!()))
        );
        assert_eq!(
            strip_to_project(
                &CanonicalRoot::new("/x/Foo".into()),
                &CanonicalCandidate::new("/x/foo/foo.txt".into())
            ),
            ProjectPath::OutsideProject
        );
    }
}
