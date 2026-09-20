//! The composer's half of a mention: what `@` means while typing, who it
//! could mean, and which of the names in the box still stand when it is sent.
//!
//! All of it is over plain data, because all of it is the part worth
//! testing: the widget that draws the list is a `Frame` of rows over what
//! [`candidates`] returns, and the send is [`mentions_in`] over the text.
//!
//! **The text carries names; the message carries keys.** Choosing somebody
//! puts `@Their-Name ` into the box as ordinary text, readable by every
//! client, and remembers the key beside the label. At send time each label
//! still present in the text becomes a `Mention` part with its key (SIP-19)
//! -- and one that was deleted from the box is not sent, because the box is
//! what the person sees.

use std::collections::HashMap;

use sqnr_core::PubKey;

use crate::session::{Member, Person};

/// The `@` being typed, if the text ends in one: the characters after it.
/// `"hi @Ad"` is `Some("Ad")`, `"@"` is `Some("")`, `"a@b.c"` is `None` --
/// an `@` inside a word is an address, not a mention.
pub fn mention_query(text: &str) -> Option<&str> {
    let start = text.rfind('@')?;
    let before = &text[..start];
    if !(before.is_empty() || before.ends_with(char::is_whitespace)) {
        return None;
    }
    let query = &text[start + 1..];
    // A space after the name is the name being finished with: the list is
    // for while it is being typed.
    if query.contains(char::is_whitespace) {
        return None;
    }
    Some(query)
}

/// Who in the room `query` could mean: everybody but us whose name,
/// handle or short key contains it, case-insensitively, by label.
pub fn candidates(
    query: &str,
    members: &[Member],
    people: &HashMap<PubKey, Person>,
    me: Option<PubKey>,
) -> Vec<(String, PubKey)> {
    let query = query.to_lowercase();
    let mut out: Vec<(String, PubKey)> = members
        .iter()
        .map(|m| m.account)
        .filter(|k| Some(*k) != me)
        .filter_map(|k| {
            let person = people.get(&k);
            let label = person
                .map(|p| p.label(&k))
                .unwrap_or_else(|| sigil_ui::message::short(&k.to_string()));
            let matches = query.is_empty()
                || label.to_lowercase().contains(&query)
                || person
                    .and_then(|p| p.handle.as_deref())
                    .is_some_and(|h| h.to_lowercase().contains(&query))
                || sigil_ui::message::short(&k.to_string())
                    .to_lowercase()
                    .contains(&query);
            matches.then_some((label, k))
        })
        .collect();
    out.sort_by(|a, b| {
        a.0.to_lowercase()
            .cmp(&b.0.to_lowercase())
            .then(a.1.cmp(&b.1))
    });
    out.dedup();
    out
}

/// Put `@label ` in place of the `@query` being typed.
pub fn complete_mention(text: &mut String, label: &str) {
    if let Some(start) = text.rfind('@') {
        text.truncate(start);
    }
    text.push('@');
    text.push_str(label);
    text.push(' ');
}

/// Which of the recorded mentions still stand: those whose `@label` is in
/// the text as a whole word. Each key once, and no more than the wire
/// allows -- the rest are text like any other.
pub fn mentions_in(text: &str, recorded: &[(String, PubKey)]) -> Vec<PubKey> {
    let mut out: Vec<PubKey> = Vec::new();
    for (label, key) in recorded {
        if out.contains(key) {
            continue;
        }
        let token = format!("@{label}");
        let mut from = 0;
        let mut found = false;
        while let Some(i) = text[from..].find(&token) {
            let at = from + i;
            let end = at + token.len();
            let before_ok = at == 0 || text[..at].ends_with(char::is_whitespace);
            let after_ok = end == text.len() || !text[end..].starts_with(char::is_alphanumeric);
            if before_ok && after_ok {
                found = true;
                break;
            }
            from = at + 1;
        }
        if found {
            out.push(*key);
        }
    }
    out.truncate(sqex_proto::message::MAX_MENTIONS);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(b: u8) -> PubKey {
        PubKey::new([b; 32])
    }

    #[test]
    fn the_query_is_the_trailing_at_token() {
        assert_eq!(mention_query("hi @Ad"), Some("Ad"));
        assert_eq!(mention_query("@"), Some(""));
        assert_eq!(mention_query("@Ada"), Some("Ada"));
        assert_eq!(mention_query("a@b.c"), None, "an address is not a mention");
        assert_eq!(mention_query("hi @Ada "), None, "finished with");
        assert_eq!(mention_query("hello"), None);
        assert_eq!(mention_query(""), None);
    }

    fn people() -> HashMap<PubKey, Person> {
        let mut p = HashMap::new();
        p.insert(
            k(2),
            Person {
                name: Some("Ada".into()),
                title: None,
                handle: Some("ada@squic.org".into()),
            },
        );
        p.insert(
            k(3),
            Person {
                name: None,
                title: None,
                handle: Some("bram@squic.org".into()),
            },
        );
        p
    }

    fn members() -> Vec<Member> {
        [1, 2, 3, 4]
            .into_iter()
            .map(|b| Member {
                account: k(b),
                admin: b == 1,
                muted: false,
            })
            .collect()
    }

    /// Everybody but me; a name, a handle or a short key matches; nothing
    /// typed yet is everybody.
    #[test]
    fn candidates_leave_me_out_and_match_name_handle_or_key() {
        let all = candidates("", &members(), &people(), Some(k(1)));
        assert_eq!(all.len(), 3, "{all:?}");
        assert!(all.iter().all(|(_, key)| *key != k(1)));
        let by_name = candidates("ad", &members(), &people(), Some(k(1)));
        assert_eq!(by_name, vec![("Ada".to_string(), k(2))]);
        let by_handle = candidates("bram", &members(), &people(), Some(k(1)));
        assert_eq!(by_handle.len(), 1);
        assert_eq!(by_handle[0].1, k(3));
        assert_eq!(
            by_handle[0].0, "bram@squic.org",
            "no name: the handle is the label"
        );
        let short = sigil_ui::message::short(&k(4).to_string());
        let by_key = candidates(&short[..3], &members(), &people(), Some(k(1)));
        assert!(by_key.iter().any(|(_, key)| *key == k(4)), "{by_key:?}");
        assert!(candidates("zzz", &members(), &people(), Some(k(1))).is_empty());
    }

    #[test]
    fn completing_replaces_the_token_being_typed() {
        let mut text = "hi @Ad".to_string();
        complete_mention(&mut text, "Ada");
        assert_eq!(text, "hi @Ada ");
        let mut text = "@".to_string();
        complete_mention(&mut text, "Bram");
        assert_eq!(text, "@Bram ");
    }

    /// A recorded mention is sent while its label is in the text as a whole
    /// word, and not otherwise: deleting the name from the box deletes the
    /// mention, and a longer name is not a shorter one.
    #[test]
    fn a_recorded_mention_is_kept_while_its_label_is_in_the_text() {
        let recorded = vec![("Ada".to_string(), k(2)), ("Adam".to_string(), k(5))];
        assert_eq!(mentions_in("hi @Ada look", &recorded), vec![k(2)]);
        assert_eq!(mentions_in("hi", &recorded), Vec::<PubKey>::new());
        assert_eq!(
            mentions_in("@Adam here", &recorded),
            vec![k(5)],
            "@Adam is not @Ada"
        );
        assert_eq!(mentions_in("@Ada m", &recorded), vec![k(2)]);
        assert_eq!(
            mentions_in("mail@Ada.example", &recorded),
            Vec::<PubKey>::new(),
            "inside a word is not a mention"
        );
        assert_eq!(mentions_in("@Ada, @Ada!", &recorded), vec![k(2)], "once");
    }

    #[test]
    fn no_more_than_the_wire_allows() {
        let recorded: Vec<(String, PubKey)> = (0..40).map(|i| (format!("p{i}"), k(i))).collect();
        let text: String = recorded.iter().map(|(l, _)| format!("@{l} ")).collect();
        assert_eq!(
            mentions_in(&text, &recorded).len(),
            sqex_proto::message::MAX_MENTIONS
        );
        let few: Vec<(String, PubKey)> = recorded[..31].to_vec();
        let text: String = few.iter().map(|(l, _)| format!("@{l} ")).collect();
        assert_eq!(mentions_in(&text, &few).len(), 31);
    }
}
