//! Repo reference parsing and Windows-safe folder naming.
//!
//! Cloned repos live in `{Owner}__{Repo}` folders. GitHub owner names can only
//! contain letters, digits and single hyphens, so the first `__` is always the
//! separator, even when the owner or repo itself contains hyphens.

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRef {
    pub host: String,
    pub owner: String,
    pub repo: String,
}

impl RepoRef {
    pub fn new(host: &str, owner: &str, repo: &str) -> Self {
        RepoRef { host: host.to_lowercase(), owner: owner.to_string(), repo: repo.to_string() }
    }

    pub fn is_github(&self) -> bool {
        self.host == "github.com"
    }

    pub fn clone_url(&self) -> String {
        format!("https://{}/{}/{}.git", self.host, self.owner, self.repo)
    }

    pub fn web_url(&self) -> String {
        format!("https://{}/{}/{}", self.host, self.owner, self.repo)
    }

    /// Case-insensitive identity, used to match a re-clone to its old metadata.
    pub fn key(&self) -> String {
        format!("{}/{}/{}", self.host, self.owner, self.repo).to_lowercase()
    }

    pub fn folder_name(&self) -> String {
        folder_name(&self.owner, &self.repo)
    }
}

fn valid_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
}

fn strip_git_suffix(s: &str) -> &str {
    s.strip_suffix(".git").unwrap_or(s)
}

/// Accepts `owner/repo`, `host/owner/repo`, `https://host/owner/repo(.git)(/...)`,
/// `git@host:owner/repo(.git)` and `ssh://git@host/owner/repo(.git)`.
pub fn parse_repo_input(input: &str) -> Option<RepoRef> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }

    let (host, path) = if let Some(rest) = s
        .strip_prefix("https://")
        .or_else(|| s.strip_prefix("http://"))
        .or_else(|| s.strip_prefix("ssh://"))
        .or_else(|| s.strip_prefix("git://"))
    {
        let rest = rest.split(['?', '#']).next().unwrap_or("");
        let (authority, path) = rest.split_once('/')?;
        let host = authority.rsplit('@').next()?.split(':').next()?;
        (host.to_string(), path.to_string())
    } else if let Some((left, path)) = s.split_once(':').filter(|(l, _)| l.contains('@')) {
        // scp-like: git@github.com:owner/repo.git
        let host = left.rsplit('@').next()?;
        (host.to_string(), path.to_string())
    } else {
        let parts: Vec<&str> = s.split('/').filter(|p| !p.is_empty()).collect();
        match parts.len() {
            2 => ("github.com".to_string(), s.to_string()),
            3 if parts[0].contains('.') => (parts[0].to_string(), parts[1..].join("/")),
            _ => return None,
        }
    };

    let mut segs = path.split('/').filter(|p| !p.is_empty());
    let owner = segs.next()?;
    let repo = strip_git_suffix(segs.next()?);
    if host.is_empty() || !valid_segment(owner) || !valid_segment(repo) {
        return None;
    }
    Some(RepoRef::new(&host, owner, repo))
}

const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Make a single path component valid on Windows (and therefore everywhere):
/// no reserved characters, no trailing dots or spaces, no device names.
pub fn sanitize_component(name: &str) -> String {
    let mut out: String = name
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '/' | '\\' | '|' | '?' | '*' => '_',
            c if (c as u32) < 0x20 => '_',
            c => c,
        })
        .collect();
    while out.ends_with('.') || out.ends_with(' ') {
        out.pop();
    }
    let trimmed = out.trim_start().to_string();
    let mut out = if trimmed.is_empty() { "_".to_string() } else { trimmed };
    let stem = out.split('.').next().unwrap_or("").to_ascii_uppercase();
    if RESERVED.contains(&stem.as_str()) {
        out.push('_');
    }
    out
}

pub fn folder_name(owner: &str, repo: &str) -> String {
    sanitize_component(&format!("{owner}__{repo}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(owner: &str, repo: &str) -> Option<RepoRef> {
        Some(RepoRef::new("github.com", owner, repo))
    }

    #[test]
    fn parses_common_forms() {
        assert_eq!(parse_repo_input("Yoinky-Prj/Main-Web"), r("Yoinky-Prj", "Main-Web"));
        assert_eq!(parse_repo_input("https://github.com/Yoinky-Prj/Main-Web.git"), r("Yoinky-Prj", "Main-Web"));
        assert_eq!(parse_repo_input("https://github.com/openclaw/openclaw/tree/main/src"), r("openclaw", "openclaw"));
        assert_eq!(parse_repo_input("git@github.com:HKCC-KTFC/KTFC-Stack.git"), r("HKCC-KTFC", "KTFC-Stack"));
        assert_eq!(parse_repo_input("ssh://git@github.com/a/b.git"), r("a", "b"));
        assert_eq!(parse_repo_input("github.com/a/b"), r("a", "b"));
        assert_eq!(parse_repo_input("https://user:tok@github.com/a/b?x=1"), r("a", "b"));
        assert_eq!(
            parse_repo_input("https://gitlab.com/group/proj"),
            Some(RepoRef::new("gitlab.com", "group", "proj"))
        );
        // Repo names with dots survive parsing; only the folder name is cleaned.
        assert_eq!(
            parse_repo_input("https://github.com/Yoinky-Prj/Project-E.M.B.E.R..git"),
            r("Yoinky-Prj", "Project-E.M.B.E.R.")
        );
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_repo_input(""), None);
        assert_eq!(parse_repo_input("just-a-name"), None);
        assert_eq!(parse_repo_input("a/b/c/d"), None);
        assert_eq!(parse_repo_input("https://github.com/onlyowner"), None);
        assert_eq!(parse_repo_input("a b/c"), None);
    }

    #[test]
    fn folder_names_are_windows_safe() {
        assert_eq!(folder_name("Yoinky-Prj", "Project-E.M.B.E.R."), "Yoinky-Prj__Project-E.M.B.E.R");
        assert_eq!(folder_name("Yoinky-Prj", "Main-Web"), "Yoinky-Prj__Main-Web");
        assert_eq!(sanitize_component("a:b*c"), "a_b_c");
        assert_eq!(sanitize_component("con"), "con_");
        assert_eq!(sanitize_component("nul.txt"), "nul.txt_");
        assert_eq!(sanitize_component("  name . . "), "name");
        assert_eq!(sanitize_component("..."), "_");
    }

    #[test]
    fn key_is_case_insensitive() {
        let a = parse_repo_input("Yoinky-Prj/Main-Web").unwrap();
        let b = parse_repo_input("https://GitHub.com/yoinky-prj/main-web.git").unwrap();
        assert_eq!(a.key(), b.key());
    }
}
