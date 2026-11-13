//! Finding MilkDrop presets on disk and choosing which one plays next.

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result};

use crate::viz::Rng;

/// Stop scanning after this many presets (a guard against pointing at `/`).
const MAX_PRESETS: usize = 100_000;
const EXTENSION: &str = "milk";

/// All `.milk` files under `dir` (any depth), sorted. Symlinked folders are
/// not followed, so loops can't trap the scan.
pub fn scan(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut found = Vec::new();
    let mut pending = vec![dir.to_path_buf()];
    let mut first = true;
    while let Some(current) = pending.pop() {
        let entries = match fs::read_dir(&current) {
            Ok(entries) => entries,
            // The top folder must be readable; unreadable subfolders are skipped.
            Err(e) if first => {
                return Err(e).with_context(|| format!("reading {}", current.display()));
            }
            Err(e) => {
                log::debug!("skipping {}: {e}", current.display());
                continue;
            }
        };
        first = false;
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                pending.push(path);
            } else if is_preset(&path) {
                found.push(path);
                if found.len() >= MAX_PRESETS {
                    log::warn!("stopped after {MAX_PRESETS} presets");
                    pending.clear();
                    break;
                }
            }
        }
    }
    found.sort();
    Ok(found)
}

fn is_preset(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(EXTENSION))
}

/// Expands a leading `~` to `home`, as a shell would. Other paths are unchanged.
pub fn expand_home(path: &str, home: Option<&Path>) -> PathBuf {
    let path = path.trim();
    match (path.strip_prefix('~'), home) {
        (Some(""), Some(home)) => home.to_path_buf(),
        (Some(rest), Some(home)) if rest.starts_with('/') => home.join(&rest[1..]),
        _ => PathBuf::from(path),
    }
}

/// A preset's name for display: its file name without `.milk`.
pub fn display_name(path: &Path) -> String {
    path.file_stem().map_or_else(
        || path.display().to_string(),
        |s| s.to_string_lossy().into_owned(),
    )
}

/// The presets available and which one is playing.
#[derive(Debug, Clone)]
pub struct Playlist {
    presets: Vec<PathBuf>,
    current: Option<usize>,
    rng: Rng,
}

impl Playlist {
    pub fn new(presets: Vec<PathBuf>, seed: u32) -> Self {
        Self {
            presets,
            current: None,
            rng: Rng::new(seed),
        }
    }

    pub fn len(&self) -> usize {
        self.presets.len()
    }

    /// Moves to a random preset other than the current one.
    pub fn random(&mut self) -> Option<&Path> {
        let n = self.presets.len();
        if n == 0 {
            return None;
        }
        let mut pick = self.rng.next_u32() as usize % n;
        if n > 1 && Some(pick) == self.current {
            pick = (pick + 1) % n;
        }
        self.current = Some(pick);
        Some(&self.presets[pick])
    }

    /// Moves `direction` (+1 / -1) through the sorted list, wrapping.
    pub fn step(&mut self, direction: isize) -> Option<&Path> {
        let n = self.presets.len() as isize;
        if n == 0 {
            return None;
        }
        let next = match self.current {
            Some(i) => (i as isize + direction.signum()).rem_euclid(n),
            None => 0,
        } as usize;
        self.current = Some(next);
        Some(&self.presets[next])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("neoviz-presets-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn touch(path: &Path) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, "").unwrap();
    }

    #[test]
    fn scan_finds_presets_at_any_depth_and_ignores_other_files() {
        let dir = temp_dir("scan");
        touch(&dir.join("a.milk"));
        touch(&dir.join("Sub/b.MILK"));
        touch(&dir.join("Sub/Deeper/c.milk"));
        touch(&dir.join("Sub/readme.txt"));
        touch(&dir.join("texture.jpg"));

        let found = scan(&dir).unwrap();
        let names: Vec<String> = found.iter().map(|p| display_name(p)).collect();
        assert_eq!(names.len(), 3);
        assert!(names.contains(&"b".to_owned()));
        assert!(found.windows(2).all(|w| w[0] <= w[1]), "sorted");
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn scanning_a_missing_folder_is_an_error() {
        let dir = temp_dir("missing").join("nope");
        assert!(scan(&dir).is_err());
    }

    #[test]
    fn tilde_expands_to_home() {
        let home = Some(Path::new("/home/me"));
        assert_eq!(
            expand_home("~/presets", home),
            Path::new("/home/me/presets")
        );
        assert_eq!(expand_home("  ~  ", home), Path::new("/home/me"));
    }

    #[test]
    fn other_paths_are_left_alone() {
        let home = Some(Path::new("/home/me"));
        assert_eq!(expand_home("/abs/presets", home), Path::new("/abs/presets"));
        assert_eq!(
            expand_home("~other/presets", home),
            Path::new("~other/presets")
        );
        assert_eq!(expand_home("~/x", None), Path::new("~/x"));
    }

    #[test]
    fn display_name_drops_the_extension() {
        assert_eq!(
            display_name(Path::new("/x/Geiss - Feedback.milk")),
            "Geiss - Feedback"
        );
    }

    #[test]
    fn empty_playlist_has_nothing_to_play() {
        let mut p = Playlist::new(Vec::new(), 1);
        assert_eq!(p.len(), 0);
        assert!(p.random().is_none());
        assert!(p.step(1).is_none());
    }

    #[test]
    fn step_starts_at_the_first_preset_and_wraps() {
        let mut p = Playlist::new(vec!["a".into(), "b".into(), "c".into()], 1);
        assert_eq!(p.step(1).unwrap(), Path::new("a"));
        assert_eq!(p.step(-1).unwrap(), Path::new("c"));
        assert_eq!(p.step(1).unwrap(), Path::new("a"));
    }

    #[test]
    fn random_never_repeats_the_current_preset() {
        let mut p = Playlist::new(vec!["a".into(), "b".into()], 7);
        let mut last = p.random().unwrap().to_path_buf();
        for _ in 0..50 {
            let next = p.random().unwrap().to_path_buf();
            assert_ne!(next, last);
            last = next;
        }
    }

    #[test]
    fn single_preset_is_always_picked() {
        let mut p = Playlist::new(vec!["only".into()], 3);
        assert_eq!(p.random().unwrap(), Path::new("only"));
        assert_eq!(p.random().unwrap(), Path::new("only"));
    }
}
