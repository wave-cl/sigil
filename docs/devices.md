# Your other devices

A second device linked to your account starts with what the exchange still
keeps and nothing older. The rest of a conversation -- everything past the
exchange's retention window -- was on the first device's disk and nowhere
else, and could not be fetched again: an epoch key's envelope is served
once, so a message the exchange pruned and one device opened existed in one
place.

sigil now hands that history between your own devices. While two of them
are running, they meet through the exchange and each takes from the other
what it lacks: the messages, the keys that open them, and the files they
name. Nothing is asked of you. When something arrives, a note says how
much: *Synced 12 messages and 2 files across 3 conversations from your
other device.* When nothing needs to move, nothing is said.

## How it works

SIP-42. Every ten seconds, sigil offers the exchange a session toward each
of your other devices (SIP-12). The exchange says nothing about a device
that has not offered one back -- not even that it exists -- and joins the
two the moment both have. The session it joins is keyed by the two devices
and cannot be read by the exchange; it relays ciphertext and holds nothing.

Each side first shows a credential from your account naming its own key
(SIP-20), judged by the exchange's clock and against the exchange's device
list *today*: a device you revoked is not a sibling, whatever it still
holds. Then each says what it has -- per conversation, the range of signed
entries and how many keys -- and each asks for what the other holds beyond
its own. Entries travel as the exchange served them, with their authors'
signatures and the exchange's receipts, and the receiving device checks
both exactly as it checks a fetch. A sibling's word counts for nothing:
what does not verify is refused, silently to the sibling, and nothing it
sends moves the fetch cursor.

Once an hour per device, or at once when a device is linked or revoked.
A sync is resumable by construction -- each ask carries where the asking
side has got to -- so one cut short is finished by the next.

## What it means for you

- **Link a second device and keep it running now and then.** It becomes
  the copy of every conversation the exchange has since forgotten, and the
  only backup there can be of keys that were served once.
- **Only what a device kept signed travels.** sigil keeps every entry it
  fetches with a receipt from the day it learned to (v0.1.26). Conversations
  read before that are on the disc that read them and do not sync; from
  here on, they do.
- **A revoked device keeps what it had**, as it always did, and syncs with
  nobody again.

From a terminal, `sqex-chat device sync` runs the same exchange by hand and
prints what moved.
