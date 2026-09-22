//! Every character sigil *types* is one the font actually has.
//!
//! # Why a scan and not a list
//!
//! `conversation_row`'s `the_characters_we_type_are_in_the_font` is the older
//! form of this: a hand-written `TYPED` list, which only ever covers what
//! somebody remembered to put on it. It has caught the trap twice -- a tick,
//! a double tick and a turned arrow shipping as tofu boxes, and the filled
//! and hollow discs before them -- and it missed the third, in sigil-android,
//! whose Phone tab marked each capability with a filled or hollow disc. The
//! filled one is not in egui's bundled font, so every capability the phone
//! actually had drew as a box: the one state somebody opens that pane to
//! confirm read as a rendering fault. Nothing failed, because the
//! accessibility tree carries the *string*, which was correct, so every text
//! assertion passed. Only looking at the phone found it.
//!
//! So this reads every sigil crate's source, takes every non-ASCII character
//! inside a string literal, and asks a real `egui::Context` whether the fonts
//! sigil runs with have it. A literal added tomorrow is covered without
//! anybody adding it anywhere. The older list stays: it also asserts that the
//! characters sigil *does* type are present, which is the other half.
//!
//! # What it deliberately skips
//!
//! **Test code**, whose fixtures are *meant* to contain characters the font
//! lacks: `short()` is checked against Japanese and emoji precisely because
//! it must cut them on a character boundary. Everything under a `tests/`
//! directory, and everything after a file's first top-level `#[cfg(test)]`.
//!
//! **The emoji table**, which is data for a picture loader rather than words
//! anybody types -- `sigil-emoji` holds every emoji there is, and by
//! construction almost none of them is in a text font. That it falls back to
//! drawing them as text without a loader is known, and has a test of its own.
//! Named by path and asserted to exist, so a rename cannot quietly widen the
//! scan to two and a half thousand hits again.
//!
//! # What it does not cover
//!
//! It reads characters, so an escape goes straight past it. Checked, not
//! assumed: the control was run both ways, and the escaped form passed while
//! the pasted one failed. That is the right trade for now, because nobody
//! writes a disc as an escape; somebody pastes the character in, which is
//! exactly how this shipped three times. If an escape ever does it, the
//! answer is to decode them here rather than to go back to a list.
use std::path::{Path, PathBuf};

/// Emoji, as data for a picture loader. See the module comment.
const EMOJI_TABLE: &str = "sigil-emoji/src/table.rs";

/// The source files this scans, so a failure can name one.
///
/// Product code only: a `tests` directory is fixtures, and a fixture with a
/// character the font lacks is usually the point of the fixture.
fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n == "tests") {
                continue;
            }
            sources(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs")
            && !path.to_string_lossy().ends_with(EMOJI_TABLE)
        {
            out.push(path);
        }
    }
}

/// The non-ASCII characters inside string literals, with the line each is on.
///
/// Deliberately crude: it drops `//` comments, then takes what is between
/// unescaped double quotes. Comments are dropped because this file, and the
/// ones that record the trap, *discuss* the characters that are missing --
/// and a check that flags its own explanation is a check people switch off.
/// A raw string or a `"` inside a char literal could confuse it; if it ever
/// does, the failure names the file and the line, which is enough to see.
fn typed(source: &str) -> Vec<(usize, char)> {
    // A file's unit tests are fixtures too. Everything from the first
    // top-level `#[cfg(test)]` on.
    let source = match source.find("\n#[cfg(test)]") {
        Some(i) => &source[..i],
        None => source,
    };
    let mut found = Vec::new();
    for (n, line) in source.lines().enumerate() {
        let code = match line.find("//") {
            Some(i) => &line[..i],
            None => line,
        };
        let mut in_string = false;
        let mut escaped = false;
        for c in code.chars() {
            if escaped {
                escaped = false;
                continue;
            }
            match c {
                '\\' if in_string => escaped = true,
                '"' => in_string = !in_string,
                _ if in_string && !c.is_ascii() => found.push((n + 1, c)),
                _ => {}
            }
        }
    }
    found
}

#[test]
fn every_character_in_a_string_literal_is_in_the_font() {
    // Every crate in the workspace, not just this one: the fault this
    // exists for was in a crate that had no such check.
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/sigil has a parent")
        .to_path_buf();
    let mut files = Vec::new();
    sources(&root, &mut files);
    assert!(
        std::fs::metadata(root.join(EMOJI_TABLE)).is_ok(),
        "{EMOJI_TABLE} is skipped by name and is not there any more: either it \
         moved, in which case fix the name, or it is gone, in which case delete \
         the exception rather than leaving a hole in the scan"
    );
    assert!(
        files.len() > 40,
        "only {} source files found under {}: the scan is pointed at the wrong \
         place and would pass whatever the code said",
        files.len(),
        root.display()
    );

    let mut checked = 0usize;
    let mut missing: Vec<String> = Vec::new();
    let ctx = egui::Context::default();
    // The pass's output has to be taken and cleared: dropping a
    // `TexturesDelta` with the font atlas in it panics on the way out, which
    // reads as a failure of whatever the test was doing.
    let mut out = ctx.run_ui(egui::RawInput::default(), |ui| {
        let id = egui::FontId::proportional(14.0);
        for file in &files {
            let Ok(source) = std::fs::read_to_string(file) else {
                continue;
            };
            for (line, c) in typed(&source) {
                checked += 1;
                if !ui.ctx().fonts_mut(|f| f.has_glyph(&id, c)) {
                    missing.push(format!(
                        "{}:{line}: U+{:04X} {c:?} is drawn and the font does not \
                         have it, so it draws as a tofu box -- paint it instead \
                         (see sigil_ui::dot, and sigil::icon)",
                        file.display(),
                        c as u32
                    ));
                }
            }
        }
        // The control. sigil's own strings do contain non-ASCII -- the em
        // dashes and ellipses in its sentences -- so a scan that examined
        // nothing would be a pass that meant nothing.
        // A floor against the skips above swallowing the scan rather than
        // narrowing it -- not a claim about how much prose there is. Product
        // code types three non-ASCII characters in all (an em dash, an
        // ellipsis and a middle dot) across 78 sites, which is itself worth
        // knowing: sigil is nearly all ASCII, and the ones it does type are
        // exactly the ones the older list already names.
        assert!(
            checked > 40,
            "only {checked} non-ASCII characters were found in string literals, \
             and product code has 78 of them: the skips are swallowing the scan"
        );
        // And that the instrument can say no: a character known not to be
        // in the font must be reported as missing.
        assert!(
            !ui.ctx().fonts_mut(|f| f.has_glyph(&id, '●')),
            "U+25CF '●' is in the font now. Good news, and this test's \
             negative control is gone: pick another character that is not."
        );
    });
    out.textures_delta.clear();

    assert!(
        missing.is_empty(),
        "{} character(s) are typed and cannot be drawn:\n{}",
        missing.len(),
        missing.join("\n")
    );
}

/// A run of spaces in the middle of a sentence the app says.
///
/// # Why this is here
///
/// Rust's `\` at the end of a line joins the next one and eats its
/// indentation, which is how every long message in this workspace is
/// written. Lose the backslash -- a scripted edit, a paste, a merge -- and
/// the literal keeps the indentation instead: the sentence still compiles,
/// still passes every test that looks for a substring near its start, and
/// draws with a hole in it. Four had shipped that way when this was written,
/// one of them 43 spaces wide, in a refusal the operator console shows and
/// in the two sentences that tell somebody a key is not a key.
///
/// Nothing legitimate in this workspace puts three spaces between two words,
/// which is what makes this cheap. If something ever needs to -- a column of
/// figures in a literal -- the answer is to build it rather than to write it
/// out, because egui does not draw runs of spaces the way a terminal does
/// anyway.
#[test]
fn nothing_the_app_says_has_a_hole_in_it() {
    /// Four. Two is a sentence break in older prose, and three is
    /// `sigil-update-tool` printing an indented shell line for somebody to
    /// copy -- the one place in the workspace that means it. Every hole
    /// found so far has been at least five, and the widest was forty-three.
    const RUN: usize = 4;

    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/sigil has a parent")
        .to_path_buf();
    let mut files = Vec::new();
    sources(&root, &mut files);
    assert!(
        files.len() > 40,
        "only {} source files found: the scan is pointed at the wrong place",
        files.len()
    );

    let mut holes: Vec<String> = Vec::new();
    let mut literals = 0usize;
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for (line, text) in strings(&source) {
            literals += 1;
            if let Some(run) = hole(&text, RUN) {
                holes.push(format!(
                    "{}:{line}: {run} spaces in the middle of \"{text}\" -- a \
                     line continuation lost its backslash and the indentation \
                     is in the sentence now",
                    file.display()
                ));
            }
        }
    }
    assert!(
        literals > 200,
        "only {literals} string literals were found, and this workspace has \
         thousands: the scan is seeing almost nothing"
    );
    // The instrument can say no.
    assert_eq!(
        hole("a lost     backslash", RUN),
        Some(5),
        "the scan does not see a hole it is shown"
    );
    assert_eq!(hole("two  spaces", RUN), None, "two spaces is not a hole");
    assert_eq!(
        hole("#   gh secret set", RUN),
        None,
        "an indented shell line is not a hole; see RUN"
    );
    assert_eq!(hole("     leading", RUN), None, "an indent is not a hole");
    assert_eq!(
        hole("a line\n     indented", RUN),
        None,
        "indentation after a newline the literal asks for is not a hole"
    );

    assert!(
        holes.is_empty(),
        "{} sentence(s) are drawn with a gap in them:\n{}",
        holes.len(),
        holes.join("\n")
    );
}

/// A typewriter dash in a sentence the app says.
///
/// The comments in this workspace write `--`, and the prose the app draws
/// writes `—`, which the font has and the phone showed beside a `--` in the
/// next paragraph on the same pane. Seven sentences had the comment's dash
/// in them when this was written. A literal that says `--` is either that,
/// or a flag for somebody to type on a command line -- which is what the
/// spaces are for: `--flag` has none, and a dash between words has one on
/// each side.
#[test]
fn nothing_the_app_says_has_a_typewriter_dash_in_it() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("crates/sigil has a parent")
        .to_path_buf();
    let mut files = Vec::new();
    sources(&root, &mut files);
    let mut dashes: Vec<String> = Vec::new();
    let mut literals = 0usize;
    for file in &files {
        let Ok(source) = std::fs::read_to_string(file) else {
            continue;
        };
        for (line, text) in strings(&source) {
            literals += 1;
            if text.contains(" -- ") {
                dashes.push(format!("{}:{line}: \"{text}\"", file.display()));
            }
        }
    }
    assert!(literals > 200, "only {literals} string literals were found");
    assert!(
        dashes.is_empty(),
        "{} sentence(s) are drawn with a typewriter dash; the font has an em dash:\n{}",
        dashes.len(),
        dashes.join("\n")
    );
}

/// A run of `least` or more spaces with a word character on both sides, if
/// there is one, and how long it is.
///
/// Leading and trailing runs are not holes: a literal that starts or ends
/// with spaces is joining onto something. Nor is a run against a newline the
/// literal asks for -- that is the next line's indentation, written on
/// purpose.
fn hole(text: &str, least: usize) -> Option<usize> {
    let bytes: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == ' ' {
            let start = i;
            while i < bytes.len() && bytes[i] == ' ' {
                i += 1;
            }
            let run = i - start;
            let before = start.checked_sub(1).map(|j| bytes[j]);
            let after = bytes.get(i).copied();
            if run >= least
                && matches!(before, Some(c) if c != '\n')
                && matches!(after, Some(c) if c != '\n')
            {
                return Some(run);
            }
        } else {
            i += 1;
        }
    }
    None
}

/// The contents of each string literal, with the line it starts on.
///
/// As crude as `typed` above and for the same reasons, with one thing it
/// cannot be crude about: a literal continued with a trailing `\` spans
/// lines, and those are exactly the ones at risk -- a hole is what is left
/// where the continuation was. So an open literal is carried on **only**
/// when the line it is on ends with a backslash, which is what Rust means by
/// one; an open literal at the end of any other line is this scanner having
/// lost its place, on a `\'"\'` or a raw string, and it is dropped rather
/// than swallowing the rest of the file.
///
/// Rust drops the continuation line's indentation, so this does too -- and
/// then a hole is a run of spaces that survived *inside* the text, which is
/// the fault itself whether or not rustfmt has since joined the lines.
fn strings(source: &str) -> Vec<(usize, String)> {
    let source = match source.find("\n#[cfg(test)]") {
        Some(i) => &source[..i],
        None => source,
    };
    let mut out = Vec::new();
    let mut open: Option<(usize, String)> = None;
    for (n, line) in source.lines().enumerate() {
        // A continuation line starts where Rust starts it, and a `//` inside
        // an open literal is part of it (a URL), not a comment.
        let code = match &open {
            Some(_) => line.trim_start(),
            None => match line.find("//") {
                Some(i) => &line[..i],
                None => line,
            },
        };
        let mut text = open.take();
        let mut chars = code.chars();
        while let Some(c) = chars.next() {
            match (&mut text, c) {
                (Some((_, t)), '\\') => match chars.next() {
                    // The continuation itself: nothing of it is in the text.
                    None => {}
                    // A line break the literal *asks* for. Kept as one,
                    // because what follows it is indentation of a new line
                    // and not a gap in a sentence -- taking the `n` as a
                    // letter would make `\n    foo` read as a four-space
                    // hole.
                    Some('n' | 'r' | 't') => t.push('\n'),
                    Some(e) => t.push(e),
                },
                (Some((at, t)), '"') => {
                    out.push((*at, std::mem::take(t)));
                    text = None;
                }
                (Some((_, t)), _) => t.push(c),
                (None, '"') => text = Some((n + 1, String::new())),
                (None, _) => {}
            }
        }
        // Carried on only where Rust would carry it on.
        open = match text {
            Some(held) if code.trim_end().ends_with('\\') => Some(held),
            _ => None,
        };
    }
    out
}
