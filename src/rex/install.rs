//! Which `rex` rexia runs: for Restart Server, `rex skill`, and the installed version.
//! The one on PATH by default, or a build of your own, such as one in a source checkout.
use sha2::{Digest, Sha256};
use std::{
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::RwLock,
};

/// The chosen build, or `None` for the `rex` on PATH.
static CHOSEN: RwLock<Option<PathBuf>> = RwLock::new(None);

/// Run `rex` from `location` from now on, or the one on PATH with `None`.
pub fn choose(location: Option<PathBuf>) {
    *CHOSEN.write().expect("rex installation") = location;
}

/// The program to run: `$REXIA_REX` (for tests), the chosen build, or `rex` on PATH.
pub fn program() -> String {
    if let Ok(rex) = std::env::var("REXIA_REX") {
        return rex;
    }
    match CHOSEN.read().expect("rex installation").as_deref() {
        Some(location) => resolve(location)
            .unwrap_or_else(|| location.to_path_buf())
            .display()
            .to_string(),
        None => "rex".into(),
    }
}

fn executable(path: &Path) -> bool {
    std::fs::metadata(path).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// The `rex` executable a location names: the file itself, or a `rex` in a directory or
/// in the build folders of a source checkout.
pub fn resolve(location: &Path) -> Option<PathBuf> {
    if executable(location) {
        return Some(location.to_path_buf());
    }
    ["rex", "bin/rex", "build/rex", "dist/rex", "out/rex"]
        .iter()
        .map(|rest| location.join(rest))
        .find(|candidate| executable(candidate))
}

/// The `rex` on PATH.
pub fn on_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join("rex"))
        .find(|candidate| executable(candidate))
}

/// Whether an installation is ready to run, and what it is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Installation {
    pub path: PathBuf,
    pub version: String,
    pub commit: Option<String>,
    pub built: Option<String>,
    pub sha256: String,
}

/// Check that `rex` at `location` (or on PATH, with `None`) runs: its version, commit
/// and the SHA-256 of the executable. Why not, otherwise.
pub fn check(location: Option<&Path>) -> Result<Installation, String> {
    let path = match location {
        Some(location) => resolve(location).ok_or_else(|| {
            format!(
                "No rex executable at {}, or in its bin, build, dist or out folders",
                location.display()
            )
        })?,
        None => on_path().ok_or("There's no rex on PATH")?,
    };
    let output = std::process::Command::new(&path)
        .arg("version")
        .output()
        .map_err(|e| format!("{} doesn't run: {e}", path.display()))?;
    if !output.status.success() {
        return Err(format!("{} version failed", path.display()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let mut lines = text.lines();
    let first = lines.next().unwrap_or_default().trim();
    let version = first
        .strip_prefix("rex ")
        .ok_or_else(|| format!("{} isn't rex: it says {first:?}", path.display()))?
        .to_owned();
    let field = |name: &str| {
        text.lines()
            .find_map(|l| l.trim().strip_prefix(name).map(|v| v.trim().to_owned()))
    };
    let bytes =
        std::fs::read(&path).map_err(|e| format!("Couldn't read {}: {e}", path.display()))?;
    let sha256 = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(Installation {
        path,
        version,
        commit: field("commit:"),
        built: field("built:"),
        sha256,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn checkouts_resolve_to_their_build_and_are_checked() {
        let dir = tempfile::tempdir().unwrap();
        assert!(resolve(dir.path()).is_none());
        assert!(
            check(Some(dir.path()))
                .unwrap_err()
                .starts_with("No rex executable")
        );
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let rex = bin.join("rex");
        std::fs::write(
            &rex,
            "#!/bin/sh\necho 'rex 0.1.0-dev'\necho 'commit: abc123'\necho 'built: today'\n",
        )
        .unwrap();
        std::fs::set_permissions(&rex, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert_eq!(resolve(dir.path()), Some(rex.clone()));
        let found = check(Some(dir.path())).unwrap();
        assert_eq!(found.path, rex);
        assert_eq!(found.version, "0.1.0-dev");
        assert_eq!(found.commit.as_deref(), Some("abc123"));
        assert_eq!(found.sha256.len(), 64);
        // Something that runs but isn't rex says so.
        std::fs::write(&rex, "#!/bin/sh\necho hello\n").unwrap();
        assert!(check(Some(&rex)).unwrap_err().contains("isn't rex"));
    }
}
