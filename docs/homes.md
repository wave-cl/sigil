# Conversations that live elsewhere

A channel is ordered by one exchange -- the one that made it -- and other
exchanges may hold a copy (SIP-35). Until now a copy was read-only where it
was held: to say something in a channel that lived at `squic.org`, you had
to be connected to `squic.org`.

Now you post where you are. A conversation that lives at another exchange
is named with where it lives -- `general@trunk.exchange` beside your own
`general` -- in the list, the bar and the directory, so two rooms called
the same thing on two exchanges read as two rooms. When one is open the
bar says so too -- *lives at trunk.exchange* -- and what you
write is carried to that exchange, ordered there, receipted there, and
comes back to the copy you are reading within a second. You cannot tell
from the answer whether the exchange you posted at ordered the message or
carried it, and you are not meant to.

## How it works

SIP-43. Your exchange asks the origin to order the post over the peering
link it already holds -- the one it pulls the copy through -- and hands the
origin's answer back to you unaltered. Nothing is stored on the way and no
number is assigned on the way: a post the origin has not ordered is not a
post. Your client signs the message under the origin's key, not under the
exchange it is connected to, because that is the key the message belongs
to; a replica cannot re-sign it, and a replica that altered it has sent
something the origin refuses.

When the origin cannot be reached, you are told so at once -- *this
conversation lives at another exchange, which cannot be reached right now;
nothing was sent* -- and your draft stands. Nothing is queued: a queue that
failed later would have to invent the failure after you were told success.

## What it means for you

- **Follow a conversation from home.** An operator who replicates a channel
  from another exchange gives its members a place to read *and* write it
  without a second connection.
- **Joining, inviting, renaming and rotating still happen at the origin.**
  They are rare and administrative; posting is what you do all day, and
  posting is what travels.
- **One device, one chain.** What your device signs for a channel is kept
  by the origin's key whichever exchange you reach the channel through, so
  posting from home and posting at the origin continue one history.
