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

/// The presets available, the order they play in, and where we are.
///
/// In shuffle mode the order is a random permutation, so stepping forward
/// visits every preset once before any repeats, and stepping back returns to
/// the previous one. Otherwise it's the sorted (folder) order.
#[derive(Debug, Clone)]
pub struct Playlist {
    presets: Vec<PathBuf>,
    order: Vec<usize>,
    /// Index into `order` of the preset playing now.
    pos: Option<usize>,
    shuffle: bool,
    rng: Rng,
}

impl Playlist {
    pub fn new(presets: Vec<PathBuf>, shuffle: bool, seed: u32) -> Self {
        let mut p = Self {
            order: (0..presets.len()).collect(),
            presets,
            pos: None,
            shuffle,
            rng: Rng::new(seed),
        };
        if shuffle {
            p.reshuffle(None);
        }
        p
    }

    pub fn len(&self) -> usize {
        self.presets.len()
    }

    pub fn current(&self) -> Option<&Path> {
        self.pos.map(|i| self.presets[self.order[i]].as_path())
    }

    /// Switches between shuffled and sorted order, keeping the current preset.
    pub fn set_shuffle(&mut self, shuffle: bool) {
        if shuffle == self.shuffle {
            return;
        }
        self.shuffle = shuffle;
        let current = self.pos.map(|i| self.order[i]);
        if shuffle {
            self.reshuffle(current);
        } else {
            self.order = (0..self.presets.len()).collect();
        }
        self.pos = current.and_then(|c| self.order.iter().position(|&i| i == c));
    }

    /// Moves `direction` (+1 next, -1 previous) and returns the new preset.
    /// The first move starts somewhere random.
    pub fn step(&mut self, direction: isize) -> Option<&Path> {
        let n = self.presets.len();
        if n == 0 {
            return None;
        }
        let next = match self.pos {
            // A shuffled order is already random; folder order starts anywhere.
            None if self.shuffle => 0,
            None => self.rng.next_u32() as usize % n,
            Some(i) if direction >= 0 && i + 1 == n && self.shuffle => {
                // Finished a full pass: new random order, not starting with this one.
                let current = self.order[i];
                self.reshuffle(None);
                if n > 1 && self.order[0] == current {
                    self.order.swap(0, n - 1);
                }
                0
            }
            Some(i) => (i as isize + direction.signum()).rem_euclid(n as isize) as usize,
        };
        self.pos = Some(next);
        self.current()
    }

    /// Fisher-Yates shuffle of the play order; `keep_first` goes to the front.
    fn reshuffle(&mut self, keep_first: Option<usize>) {
        self.order = (0..self.presets.len()).collect();
        for i in (1..self.order.len()).rev() {
            let j = self.rng.next_u32() as usize % (i + 1);
            self.order.swap(i, j);
        }
        if let Some(first) = keep_first
            && let Some(at) = self.order.iter().position(|&i| i == first)
        {
            self.order.swap(0, at);
        }
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

    fn names(n: usize) -> Vec<PathBuf> {
        (0..n).map(|i| PathBuf::from(format!("p{i:03}"))).collect()
    }

    #[test]
    fn empty_playlist_has_nothing_to_play() {
        let mut p = Playlist::new(Vec::new(), true, 1);
        assert_eq!(p.len(), 0);
        assert!(p.step(1).is_none());
        assert!(p.current().is_none());
    }

    #[test]
    fn sorted_mode_steps_in_folder_order_and_wraps() {
        let mut p = Playlist::new(names(3), false, 1);
        let first = p.step(1).unwrap().to_path_buf();
        let i: usize = first.to_string_lossy()[1..].parse().unwrap();
        assert_eq!(p.step(1).unwrap(), names(3)[(i + 1) % 3]);
        assert_eq!(p.step(-1).unwrap(), first);
    }

    #[test]
    fn shuffle_visits_every_preset_once_per_pass() {
        let mut p = Playlist::new(names(50), true, 9);
        let mut seen: Vec<PathBuf> = (0..50).map(|_| p.step(1).unwrap().to_path_buf()).collect();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 50);
    }

    #[test]
    fn shuffle_is_not_the_sorted_order() {
        let mut p = Playlist::new(names(50), true, 9);
        let order: Vec<PathBuf> = (0..50).map(|_| p.step(1).unwrap().to_path_buf()).collect();
        assert_ne!(order, names(50));
    }

    #[test]
    fn previous_goes_back_to_the_last_preset() {
        let mut p = Playlist::new(names(20), true, 4);
        let a = p.step(1).unwrap().to_path_buf();
        let b = p.step(1).unwrap().to_path_buf();
        assert_ne!(a, b);
        assert_eq!(p.step(-1).unwrap(), a);
        assert_eq!(p.step(1).unwrap(), b);
    }

    #[test]
    fn a_new_pass_never_repeats_the_last_preset() {
        for seed in 1..40 {
            let mut p = Playlist::new(names(3), true, seed);
            let mut last = p.step(1).unwrap().to_path_buf();
            for _ in 0..12 {
                let next = p.step(1).unwrap().to_path_buf();
                assert_ne!(next, last, "seed {seed}");
                last = next;
            }
        }
    }

    #[test]
    fn toggling_shuffle_keeps_the_current_preset() {
        let mut p = Playlist::new(names(30), true, 2);
        p.step(1);
        p.step(1);
        let now = p.current().unwrap().to_path_buf();
        p.set_shuffle(false);
        assert_eq!(p.current().unwrap(), now);
        p.set_shuffle(true);
        assert_eq!(p.current().unwrap(), now);
    }

    #[test]
    fn single_preset_always_plays() {
        let mut p = Playlist::new(names(1), true, 3);
        assert_eq!(p.step(1).unwrap(), names(1)[0]);
        assert_eq!(p.step(1).unwrap(), names(1)[0]);
        assert_eq!(p.step(-1).unwrap(), names(1)[0]);
    }
}
