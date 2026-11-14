//! Logging to stderr and to `neoviz.log` beside the settings file.
//!
//! Windows release builds have no console, so the file is the only place
//! their log ends up. It's overwritten on each launch.

use std::{
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
};

const FILE_NAME: &str = "neoviz.log";

/// Starts logging (neoViz at `info`, other crates at `warn`; `RUST_LOG` overrides).
/// Returns the log file's path if it could be created.
pub fn init(config_dir: Option<&Path>) -> Option<PathBuf> {
    let file = config_dir.and_then(open_log_file);
    let path = file.as_ref().map(|(path, _)| path.clone());
    let tee = Tee {
        file: file.map(|(_, file)| file),
    };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn,neoviz=info"))
        .target(env_logger::Target::Pipe(Box::new(tee)))
        .init();
    path
}

fn open_log_file(dir: &Path) -> Option<(PathBuf, File)> {
    let path = dir.join(FILE_NAME);
    match fs::create_dir_all(dir).and_then(|()| File::create(&path)) {
        Ok(file) => Some((path, file)),
        Err(e) => {
            // Logging isn't set up yet; stderr is all there is.
            eprintln!("can't create {}: {e}", path.display());
            None
        }
    }
}

/// Writes everything to stderr and, if open, the log file.
struct Tee {
    file: Option<File>,
}

impl Write for Tee {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        // stderr may not exist (Windows GUI app); the file is what matters.
        let _ = io::stderr().write_all(buf);
        if let Some(file) = &mut self.file
            && file.write_all(buf).is_err()
        {
            // Stop trying after the first failure (e.g. disk full).
            self.file = None;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        let _ = io::stderr().flush();
        if let Some(file) = &mut self.file {
            file.flush()?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tee_writes_the_whole_buffer_to_the_file() {
        let dir = std::env::temp_dir().join(format!("neoviz-log-test-{}", std::process::id()));
        let (path, file) = open_log_file(&dir).unwrap();
        let mut tee = Tee { file: Some(file) };
        tee.write_all(b"hello\n").unwrap();
        tee.flush().unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello\n");
        fs::remove_dir_all(&dir).unwrap();
    }
}
