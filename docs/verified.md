# Verified contacts

A shield beside a name says you compared **safety words** with that person
and they matched (SIP-41). It is your own mark: nothing the exchange says
puts it there or takes it away.

## Comparing words

From a person's message (More → *Compare safety words*), from their name in
a mention, from Members (*Verify*), or with `/verify` in a direct message: a
dialog shows six words and a QR. The words are the same on both of your
screens, or they are not. Read them to each other in person or over a call,
or one of you scans the other's code, and if they match press **They match**.
Anybody in the middle would have to make both screens lie.

The six words are the first sixty-six bits of a hash over both keys,
rendered with BIP-39's English list; they belong to the pair of you, so the
words you share with one person are not the words you share with another.
`sqex verify <key>` prints the same words from the terminal.

## What the mark means, and where it lives

The mark is on the **key**. A direct message is derived from two keys, so
nothing can change the key under a conversation; what can change is what a
**name** resolves to. If a name you verified -- `ada@squic.org` -- one day
resolves to a different key, sigil says so before letting you write to it,
with both keys in full, and does not carry the mark across.

The mark lives in this machine's store, beside your conversations, and
nowhere else. *Withdraw* in the same dialog takes it back.

## Saying so

The dialog's box, *Say at the exchange that we compared them*, lodges a
signed statement (SIP-27, claim `0x05`) that others may read. It tells the
exchange, and anyone who asks it, that the two of you spoke -- which is why
it is off unless you tick it. Nobody's statement makes a mark on your
screen; only your own comparison does.
