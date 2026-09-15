//! The emoji this person sends most, remembered on this machine.
//!
//! The picker's "Frequently used" row. A count per emoji string, in a text
//! file beside `accounts.json` — one line each, `count<TAB>emoji` — because
//! it is a habit of the person at the keyboard and not a fact about any
//! identity or exchange: the same file whichever account is being drawn, as
//! the roster is.
//!
//! Read tolerantly and written on every change, warning rather than failing
//! either way. A lost or corrupt file is an empty row, which is where
//! everyone starts.

use std::collections::HashMap;
use std::path::PathBuf;

/// Counts of emoji sent, and where they are kept.
#[derive(Debug, Default)]
pub struct Frequent {
    counts: HashMap<String, u32>,
    /// `None` keeps nothing on disk — tests, and a machine with nowhere to
    /// put it.
    path: Option<PathBuf>,
}

impl Frequent {
    /// Where the counts are kept, if this machine has anywhere to put them.
    pub fn default_path() -> Option<PathBuf> {
        dirs::data_local_dir().map(|d| d.join("sigil").join("emoji.txt"))
    }

    /// The counts this machine remembers.
    pub fn load() -> Frequent {
        match Frequent::default_path() {
            Some(path) => Frequent::at(path),
            None => Frequent::default(),
        }
    }

    /// Counts kept at `path`, read now if it exists.
    pub fn at(path: PathBuf) -> Frequent {
        let counts = match std::fs::read_to_string(&path) {
            Ok(text) => parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => HashMap::new(),
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "cannot read the emoji counts");
                HashMap::new()
            }
        };
        Frequent {
            counts,
            path: Some(path),
        }
    }

    /// Counts kept nowhere. Tests.
    pub fn in_memory() -> Frequent {
        Frequent::default()
    }

    /// One more of `emoji` sent. Written to disk before returning.
    pub fn bump(&mut self, emoji: &str) {
        if emoji.is_empty() {
            return;
        }
        *self.counts.entry(emoji.to_string()).or_default() += 1;
        self.save();
    }

    /// The `n` most sent, most first; ties by the emoji string so the order
    /// is the same from one frame to the next.
    pub fn top(&self, n: usize) -> Vec<String> {
        let mut all: Vec<(&String, &u32)> = self.counts.iter().collect();
        all.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        all.into_iter().take(n).map(|(e, _)| e.clone()).collect()
    }

    fn save(&self) {
        let Some(path) = &self.path else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            tracing::warn!(path = %parent.display(), error = %e, "cannot make the settings directory");
            return;
        }
        let mut lines: Vec<(&String, &u32)> = self.counts.iter().collect();
        lines.sort_by(|a, b| b.1.cmp(a.1).then_with(|| a.0.cmp(b.0)));
        let text: String = lines.iter().map(|(e, n)| format!("{n}\t{e}\n")).collect();
        if let Err(e) = std::fs::write(path, text) {
            tracing::warn!(path = %path.display(), error = %e, "cannot write the emoji counts");
        }
    }
}

/// `count<TAB>emoji` per line; a line that is not that is skipped.
fn parse(text: &str) -> HashMap<String, u32> {
    text.lines()
        .filter_map(|line| {
            let (n, e) = line.split_once('\t')?;
            let n: u32 = n.trim().parse().ok()?;
            let e = e.trim();
            (!e.is_empty()).then(|| (e.to_string(), n))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_most_sent_come_first_and_ties_are_stable() {
        let mut f = Frequent::in_memory();
        for e in ["b", "a", "b", "c", "a", "b"] {
            f.bump(e);
        }
        assert_eq!(f.top(5), vec!["b", "a", "c"]);
        assert_eq!(f.top(2), vec!["b", "a"]);
        f.bump("");
        assert_eq!(f.top(5), vec!["b", "a", "c"]);
    }

    #[test]
    fn counts_survive_a_round_trip_through_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("deep").join("emoji.txt");
        let mut f = Frequent::at(path.clone());
        f.bump("\u{1f389}");
        f.bump("\u{1f389}");
        f.bump("\u{1f44d}");
        let again = Frequent::at(path);
        assert_eq!(again.top(5), vec!["\u{1f389}", "\u{1f44d}"]);
    }

    #[test]
    fn a_corrupt_file_is_an_empty_row_and_a_good_line_in_it_still_counts() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("emoji.txt");
        std::fs::write(&path, "not a count\tx\n\n3\t\u{1f602}\nnine\n\t\n").unwrap();
        let f = Frequent::at(path.clone());
        assert_eq!(f.top(5), vec!["\u{1f602}"]);
        std::fs::write(&path, [0xff, 0xfe, 0x00]).unwrap();
        assert_eq!(Frequent::at(path).top(5), Vec::<String>::new());
    }
}
