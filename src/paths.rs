use std::path::{Path, PathBuf};

pub fn discover_project_root() -> Option<PathBuf> {
    let current_dir = std::env::current_dir().ok()?;
    current_dir
        .ancestors()
        .find(|candidate| looks_like_project_root(candidate))
        .map(Path::to_path_buf)
}

fn looks_like_project_root(candidate: &Path) -> bool {
    candidate.join("Cargo.toml").is_file() && candidate.join("src").is_dir()
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use tempfile::tempdir;

    use super::looks_like_project_root;

    #[test]
    fn detects_project_root_shape() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("Cargo.toml"), "[package]\nname='x'\n").unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();

        assert!(looks_like_project_root(dir.path()));
        assert!(!looks_like_project_root(Path::new(
            "/definitely/not/a/project/root"
        )));
    }
}
