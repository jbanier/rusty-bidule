use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail};

use crate::types::{AgentPermissions, permission_denied_user_prompt};

pub fn expand_prompt_file_references(
    input: &str,
    permissions: &AgentPermissions,
    project_root: Option<&Path>,
) -> Result<String> {
    let references = find_references(input);
    if references.is_empty() {
        return Ok(unescape_literal_ats(input));
    }

    if !permissions.allows_filesystem_read() {
        let denied = "permission denied: inline file references require filesystem read access. Enable it with /permissions fs read or /permissions fs write, or use /yolo on.";
        let prompt = permission_denied_user_prompt(denied).unwrap_or_else(|| denied.to_string());
        bail!(prompt);
    }

    let mut expanded = String::with_capacity(input.len());
    let mut cursor = 0usize;

    for reference in references {
        expanded.push_str(&unescape_literal_ats(&input[cursor..reference.start]));
        let resolved = resolve_reference_path(&reference.raw_path, project_root)?;
        let contents = fs::read_to_string(&resolved)
            .with_context(|| format!("failed to read referenced file {}", resolved.display()))?;
        let display_path = display_reference_path(&reference.raw_path, &resolved, project_root);
        expanded.push_str(&format_file_block(&display_path, &contents));
        cursor = reference.end;
    }

    expanded.push_str(&unescape_literal_ats(&input[cursor..]));
    Ok(expanded)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileReference {
    start: usize,
    end: usize,
    raw_path: String,
}

fn find_references(input: &str) -> Vec<FileReference> {
    let mut refs = Vec::new();
    let mut iter = input.char_indices().peekable();

    while let Some((idx, ch)) = iter.next() {
        if ch == '\\' {
            let _ = iter.next_if(|(_, next)| *next == '@');
            continue;
        }
        if ch != '@' || !can_start_reference(input, idx) {
            continue;
        }

        let start = idx;
        let path_start = idx + ch.len_utf8();
        let mut end = path_start;

        while let Some(&(next_idx, next_ch)) = iter.peek() {
            if is_reference_terminator(next_ch) {
                break;
            }
            end = next_idx + next_ch.len_utf8();
            iter.next();
        }

        if end <= path_start {
            continue;
        }

        let candidate = trim_reference_suffix(&input[path_start..end]);
        if candidate.is_empty() || !looks_like_path(candidate) {
            continue;
        }

        refs.push(FileReference {
            start,
            end: path_start + candidate.len(),
            raw_path: candidate.to_string(),
        });
    }

    refs
}

fn can_start_reference(input: &str, at_index: usize) -> bool {
    match input[..at_index].chars().next_back() {
        None => true,
        Some(prev) => {
            prev.is_whitespace() || matches!(prev, '(' | '[' | '{' | '<' | '"' | '\'' | '`')
        }
    }
}

fn is_reference_terminator(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '"' | '\'' | '`')
}

fn trim_reference_suffix(candidate: &str) -> &str {
    candidate.trim_end_matches(|ch: char| {
        matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}')
    })
}

fn looks_like_path(candidate: &str) -> bool {
    candidate.contains('/')
        || candidate.contains('\\')
        || candidate.contains('.')
        || candidate.starts_with('~')
        || Path::new(candidate).is_absolute()
}

fn unescape_literal_ats(input: &str) -> String {
    input.replace("\\@", "@")
}

fn resolve_reference_path(raw_path: &str, project_root: Option<&Path>) -> Result<PathBuf> {
    let expanded = if raw_path == "~" || raw_path.starts_with("~/") {
        let home = std::env::var_os("HOME").map(PathBuf::from).ok_or_else(|| {
            anyhow::anyhow!("cannot expand '{}' because HOME is not set", raw_path)
        })?;
        if raw_path == "~" {
            home
        } else {
            home.join(&raw_path[2..])
        }
    } else {
        PathBuf::from(raw_path)
    };

    let absolute = if expanded.is_absolute() {
        expanded
    } else {
        let root = project_root.ok_or_else(|| {
            anyhow::anyhow!(
                "cannot resolve relative file reference '@{}' because the project root could not be determined",
                raw_path
            )
        })?;
        root.join(expanded)
    };

    let canonical = absolute.canonicalize().with_context(|| {
        format!(
            "failed to resolve referenced file '{}' from {}",
            raw_path,
            absolute.display()
        )
    })?;

    if !canonical.is_file() {
        bail!(
            "referenced path '{}' resolved to {}, which is not a regular file",
            raw_path,
            canonical.display()
        );
    }

    Ok(canonical)
}

fn display_reference_path(raw_path: &str, resolved: &Path, project_root: Option<&Path>) -> String {
    if Path::new(raw_path).is_absolute() || raw_path == "~" || raw_path.starts_with("~/") {
        return resolved.display().to_string();
    }
    if let Some(root) = project_root
        && let Ok(relative) = resolved.strip_prefix(root)
    {
        return relative.display().to_string();
    }
    raw_path.to_string()
}

fn format_file_block(path: &str, contents: &str) -> String {
    let mut block = String::new();
    block.push_str(&format!("[file: {path}]\n```text\n"));
    block.push_str(contents);
    if !contents.ends_with('\n') {
        block.push('\n');
    }
    block.push_str("```");
    block
}

#[cfg(test)]
mod tests {
    use std::fs;

    use tempfile::tempdir;

    use crate::types::{AgentPermissions, FilesystemAccess};

    use super::expand_prompt_file_references;

    fn read_perms() -> AgentPermissions {
        AgentPermissions {
            allow_network: false,
            filesystem: FilesystemAccess::ReadOnly,
            yolo: false,
        }
    }

    #[test]
    fn expands_single_relative_reference() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "hello").unwrap();

        let expanded =
            expand_prompt_file_references("Use @note.md", &read_perms(), Some(dir.path())).unwrap();

        assert_eq!(expanded, "Use [file: note.md]\n```text\nhello\n```");
    }

    #[test]
    fn expands_multiple_references() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "A").unwrap();
        fs::write(dir.path().join("b.txt"), "B").unwrap();

        let expanded = expand_prompt_file_references(
            "Combine @a.txt and @b.txt",
            &read_perms(),
            Some(dir.path()),
        )
        .unwrap();

        assert!(expanded.contains("[file: a.txt]"));
        assert!(expanded.contains("[file: b.txt]"));
    }

    #[test]
    fn keeps_literal_escaped_reference_text() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "hello").unwrap();

        let expanded = expand_prompt_file_references(
            "Type \\@note.md literally",
            &read_perms(),
            Some(dir.path()),
        )
        .unwrap();

        assert_eq!(expanded, "Type @note.md literally");
    }

    #[test]
    fn leaves_email_like_text_untouched() {
        let dir = tempdir().unwrap();
        let expanded = expand_prompt_file_references(
            "Reach me at ops@example.com",
            &read_perms(),
            Some(dir.path()),
        )
        .unwrap();

        assert_eq!(expanded, "Reach me at ops@example.com");
    }

    #[test]
    fn supports_absolute_paths() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("abs.txt");
        fs::write(&path, "ABS").unwrap();

        let expanded = expand_prompt_file_references(
            &format!("Use @{}", path.display()),
            &read_perms(),
            Some(dir.path()),
        )
        .unwrap();

        assert!(expanded.contains("[file: "));
        assert!(expanded.contains("abs.txt"));
    }

    #[test]
    fn rejects_missing_files() {
        let dir = tempdir().unwrap();
        let err = expand_prompt_file_references("Use @missing.md", &read_perms(), Some(dir.path()))
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("failed to resolve referenced file")
        );
    }

    #[test]
    fn rejects_when_filesystem_read_is_disabled() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "hello").unwrap();
        let perms = AgentPermissions {
            allow_network: false,
            filesystem: FilesystemAccess::None,
            yolo: false,
        };

        let err =
            expand_prompt_file_references("Use @note.md", &perms, Some(dir.path())).unwrap_err();

        assert!(err.to_string().contains("filesystem read access"));
        assert!(err.to_string().contains("Enable it and retry?"));
        assert!(err.to_string().contains("`/permissions fs read`"));
    }

    #[test]
    fn requires_project_root_for_relative_paths() {
        let err = expand_prompt_file_references("Use @note.md", &read_perms(), None).unwrap_err();

        assert!(
            err.to_string()
                .contains("project root could not be determined")
        );
    }

    #[test]
    fn trims_sentence_punctuation_from_reference() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("note.md"), "hello").unwrap();

        let expanded =
            expand_prompt_file_references("Read @note.md.", &read_perms(), Some(dir.path()))
                .unwrap();

        assert_eq!(expanded, "Read [file: note.md]\n```text\nhello\n```.");
    }

    #[test]
    fn preserves_repo_relative_display_for_nested_files() {
        let dir = tempdir().unwrap();
        fs::create_dir_all(dir.path().join("docs")).unwrap();
        fs::write(dir.path().join("docs/ref.md"), "nested").unwrap();

        let expanded =
            expand_prompt_file_references("Use @docs/ref.md", &read_perms(), Some(dir.path()))
                .unwrap();

        assert!(expanded.contains("[file: docs/ref.md]"));
    }

    #[test]
    fn accepts_tilde_expansion() {
        let home = tempdir().unwrap();
        let original_home = std::env::var_os("HOME");
        unsafe {
            std::env::set_var("HOME", home.path());
        }
        fs::write(home.path().join("tilde.txt"), "home").unwrap();

        let expanded =
            expand_prompt_file_references("Use @~/tilde.txt", &read_perms(), None).unwrap();

        assert!(expanded.contains("[file: "));
        assert!(expanded.contains("tilde.txt"));

        match original_home {
            Some(value) => unsafe {
                std::env::set_var("HOME", value);
            },
            None => unsafe {
                std::env::remove_var("HOME");
            },
        }
    }
}
