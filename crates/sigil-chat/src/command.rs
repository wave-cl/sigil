//! The composer's half of a command: what `/` means while typing, which
//! commands it could mean, and what a line starting with one asks for.
//!
//! All of it is over plain data, like [`mention`](crate::mention): the
//! widget that draws the list is a `Frame` of rows over what [`candidates`]
//! returns, and the send is [`parse`] over the text.
//!
//! **A command is not a message.** A line that begins with `/` is asked of
//! this client and never posted -- and a line that begins with `//` is a
//! message that begins with `/`, sent with one of them.

/// One command, as the list names and describes it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Spec {
    /// The word after the slash.
    pub name: &'static str,
    /// What follows it, if anything, for the list: `"<text>"`, or empty.
    pub arg: &'static str,
    /// What it does, in a few words.
    pub what: &'static str,
}

impl Spec {
    /// The command as it is typed, with room for its argument.
    pub fn typed(&self) -> String {
        if self.arg.is_empty() {
            format!("/{}", self.name)
        } else {
            format!("/{} ", self.name)
        }
    }
}

/// What can be asked for, in the order the list shows it.
pub const COMMANDS: &[Spec] = &[
    Spec {
        name: "call",
        arg: "",
        what: "Ring this conversation",
    },
    Spec {
        name: "answer",
        arg: "",
        what: "Take the call ringing here",
    },
    Spec {
        name: "decline",
        arg: "",
        what: "Refuse the call ringing here",
    },
    Spec {
        name: "hangup",
        arg: "",
        what: "End the call, or cancel yours",
    },
    Spec {
        name: "mute",
        arg: "",
        what: "Say nothing out loud about this conversation",
    },
    Spec {
        name: "unmute",
        arg: "",
        what: "Say things out loud about it again",
    },
    Spec {
        name: "topic",
        arg: "<text>",
        what: "Set this conversation's topic",
    },
    Spec {
        name: "name",
        arg: "<text>",
        what: "Rename this group or channel",
    },
    Spec {
        name: "invite",
        arg: "<key>",
        what: "Bring somebody in, by their key",
    },
    Spec {
        name: "verify",
        arg: "",
        what: "Compare safety words with the person you are writing to",
    },
    Spec {
        name: "members",
        arg: "",
        what: "Who is here",
    },
    Spec {
        name: "settings",
        arg: "",
        what: "This conversation's settings",
    },
    Spec {
        name: "devices",
        arg: "",
        what: "Your devices",
    },
    Spec {
        name: "search",
        arg: "<text>",
        what: "Search what this client holds",
    },
    Spec {
        name: "new",
        arg: "",
        what: "Write to somebody, or start a group or a channel",
    },
    Spec {
        name: "profile",
        arg: "",
        what: "Your name and title, as others see them",
    },
    Spec {
        name: "reconnect",
        arg: "",
        what: "Dial the exchange again",
    },
    Spec {
        name: "help",
        arg: "",
        what: "Show these commands",
    },
];

/// What a line asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Call,
    Answer,
    Decline,
    Hangup,
    Mute,
    Unmute,
    Topic(String),
    Name(String),
    Invite(String),
    Verify,
    Members,
    Settings,
    Devices,
    Search(String),
    New,
    Profile,
    Reconnect,
    Help,
}

/// The `/` being typed, if the text is one: the characters after it, while
/// the word is still being typed. `"/ca"` is `Some("ca")`, `"/"` is
/// `Some("")`, `"/topic x"` is `None` -- the word is finished and the
/// argument is being written -- and `"hi /x"` is `None`: a slash inside a
/// message is a slash.
pub fn command_query(text: &str) -> Option<&str> {
    let rest = text.strip_prefix('/')?;
    if rest.starts_with('/') || rest.contains(char::is_whitespace) {
        return None;
    }
    Some(rest)
}

/// The commands `query` could be the start of, in the list's order.
pub fn candidates(query: &str) -> Vec<&'static Spec> {
    let query = query.to_lowercase();
    COMMANDS
        .iter()
        .filter(|c| c.name.starts_with(&query))
        .collect()
}

/// Put the command in place of the word being typed: `"/na"` becomes
/// `"/name "`, with the space when there is an argument to follow.
pub fn complete(text: &mut String, spec: &Spec) {
    text.clear();
    text.push_str(&spec.typed());
}

/// Whether `text` is to be sent as a message rather than read as a
/// command, and the message it is: `"//x"` is the message `"/x"`.
pub fn as_message(text: &str) -> Option<String> {
    match text.strip_prefix("//") {
        Some(rest) => Some(format!("/{rest}")),
        None => (!text.starts_with('/')).then(|| text.to_string()),
    }
}

/// What a line asks for. `Ok(None)`: not a command at all, send it.
/// `Err`: a command nobody knows, or one missing what it needs, in words
/// for the person who typed it.
pub fn parse(text: &str) -> Result<Option<Command>, String> {
    let text = text.trim();
    if as_message(text).is_some() {
        return Ok(None);
    }
    let line = &text[1..];
    let (word, arg) = match line.split_once(char::is_whitespace) {
        Some((w, a)) => (w, a.trim()),
        None => (line, ""),
    };
    let word = word.to_lowercase();
    let needs = |what: &str| -> Result<String, String> {
        if arg.is_empty() {
            Err(format!("/{word} needs {what}: /{word} {what}"))
        } else {
            Ok(arg.to_string())
        }
    };
    let cmd = match word.as_str() {
        "call" => Command::Call,
        "answer" => Command::Answer,
        "decline" => Command::Decline,
        "hangup" => Command::Hangup,
        "mute" => Command::Mute,
        "unmute" => Command::Unmute,
        "topic" => Command::Topic(needs("<text>")?),
        "name" => Command::Name(needs("<text>")?),
        "invite" => Command::Invite(needs("<key>")?),
        "verify" => Command::Verify,
        "members" => Command::Members,
        "settings" => Command::Settings,
        "devices" => Command::Devices,
        "search" => Command::Search(needs("<text>")?),
        "new" => Command::New,
        "profile" => Command::Profile,
        "reconnect" => Command::Reconnect,
        "help" => Command::Help,
        "" => return Err("Which command? Type / to see them.".into()),
        other => {
            return Err(format!(
                "/{other} is not a command. Type / to see them, or // to send a message that \
                 starts with a slash."
            ));
        }
    };
    Ok(Some(cmd))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A slash at the start, while the word is being typed, is a command
    /// being looked for; anywhere else it is a slash.
    #[test]
    fn the_word_after_a_leading_slash_is_the_query() {
        assert_eq!(command_query("/"), Some(""));
        assert_eq!(command_query("/ca"), Some("ca"));
        assert_eq!(command_query("/topic x"), None, "the word is finished");
        assert_eq!(command_query("hi /x"), None);
        assert_eq!(command_query("//x"), None, "a message");
        assert_eq!(command_query(""), None);
    }

    /// The list narrows by prefix, case aside, and empty is all of them.
    #[test]
    fn candidates_narrow_by_prefix() {
        assert_eq!(candidates("").len(), COMMANDS.len());
        let c: Vec<&str> = candidates("Ca").iter().map(|s| s.name).collect();
        assert_eq!(c, vec!["call"]);
        assert!(candidates("zz").is_empty());
        let d: Vec<&str> = candidates("de").iter().map(|s| s.name).collect();
        assert_eq!(d, vec!["decline", "devices"]);
    }

    /// Completing puts the whole word in, with a space only when something
    /// is to follow it.
    #[test]
    fn completing_leaves_room_for_an_argument_when_there_is_one() {
        let mut t = "/na".to_string();
        complete(&mut t, candidates("na")[0]);
        assert_eq!(t, "/name ");
        let mut t = "/ca".to_string();
        complete(&mut t, candidates("ca")[0]);
        assert_eq!(t, "/call");
    }

    /// A line is a command, a message, or a mistake -- and each is told
    /// apart, with the mistake said in words.
    #[test]
    fn a_line_is_a_command_a_message_or_a_mistake() {
        assert_eq!(parse("hello"), Ok(None));
        assert_eq!(as_message("hello").as_deref(), Some("hello"));
        assert_eq!(parse("/call"), Ok(Some(Command::Call)));
        assert_eq!(
            parse("  /CALL  "),
            Ok(Some(Command::Call)),
            "case and space aside"
        );
        assert_eq!(
            parse("/topic the release is Thursday"),
            Ok(Some(Command::Topic("the release is Thursday".into())))
        );
        assert_eq!(
            parse("/search  release "),
            Ok(Some(Command::Search("release".into())))
        );
        assert!(parse("/topic").unwrap_err().contains("needs <text>"));
        assert!(parse("/frobnicate").unwrap_err().contains("not a command"));
        assert!(parse("/").unwrap_err().contains("Which command"));
        // Two slashes: a message that starts with one.
        assert_eq!(parse("//etc/hosts"), Ok(None));
        assert_eq!(as_message("//etc/hosts").as_deref(), Some("/etc/hosts"));
        assert_eq!(as_message("/call"), None);
    }

    /// Every command in the list parses to something, and nothing parses
    /// that is not in the list: the list is the vocabulary.
    #[test]
    fn the_list_and_the_parser_agree() {
        for spec in COMMANDS {
            let line = format!("{}x", spec.typed());
            let line = if spec.arg.is_empty() {
                spec.typed()
            } else {
                line
            };
            assert!(
                matches!(parse(&line), Ok(Some(_))),
                "{line:?} is listed and does not parse"
            );
        }
    }
}
