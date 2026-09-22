# How a call connects

A direct-message call goes straight between the two people on it when it
can, and through the exchange when it cannot. Both paths carry the same
audio, sealed under the same kind of key; which one a call took is written
in its bar -- **direct** or **via exchange** -- and hovering the second
says why.

## The two paths

**Direct (SIP-25).** After the call is answered, each side asks the
exchange to introduce it to the other, from a port of its own. Once both
have asked, the exchange tells each the address it saw the other come
from, and one moment for both to begin. Both punch a hole towards each
other; the lower key dials, the higher listens, each admitting only the
other; and over the connection they have just made they agree a session
key the exchange was never party to. From then on the audio travels
without the exchange in the path.

**Via exchange (SIP-12, SIP-13).** The call is a room on the exchange,
which relays each frame to the other side. It cannot read them, and it
works behind any kind of NAT, because neither side needs to be reachable.

A call tries the first and falls back to the second when the other side
did not ask to be introduced (an older client, or one with the setting
off), or when the introduction was made and the hole did not open --
symmetric NAT and most carrier-grade NAT do this, since they allocate a
fresh external port per destination and the port the exchange saw is not
the one the peer can reach. Either way there is a call; the bar says
which. Falling back takes at most about fifteen seconds, and only when
this side asked and the other did not.

## What it discloses, and the switch

An introduction gives each person the other's public address: that is the
mechanism, and it cannot be had without. The exchange sees who asked to
be introduced to whom, as it already sees who is calling whom.

The Desktop pane has **Connect calls directly when possible**, on by
default. Off, this side neither asks for an introduction nor says in its
invitations that it will, and every call is relayed. The invitation
carries the caller's intention (SIP-36's `direct` bit) so that a callee
who cannot or will not is never waited on.

## What stays the same

The signalling is SIP-36 unchanged: the invitation, ringing, accepting,
declining, hanging up, and the record of how the call ended are the same
whichever path the audio took. Group calls and calls across exchanges
(SIP-39) are always relayed.

## Reading the field test

SIP-25 is Draft until a direct call is shown to survive real NAT. A call
from two homes whose bar says **direct** is that evidence; one that says
**via exchange** with a reason naming the NAT is the expected other
outcome, and still a call. `sqex/docs/sip25-field-test.md` has the
procedure.

## Somebody at another exchange

A call is placed from the conversation with the person being called, and
that is as true of somebody at another exchange as of anybody else. Reach
them first: *New conversation*, and their name where you would put a key —
`ada@b.test`. A key is base58 and base58 has no `@` in it, so the two cannot
be confused. The conversation opens at whichever exchange the two of you are
introduced at (SIP-60), reached through your home, and its call button places
the call there: the invitation goes into the conversation, their session
rings wherever it is, and both of you join on the connections your sessions
already hold (`a_call_inside_a_conversation_across_exchanges_connects` proves
the round trip against two federated exchanges). There is no field to type a
name into on its own — there was, on a Calls app no window loads — because a
call that has a conversation belongs in it.

Your exchange can also carry a call *for a name* to theirs without any
conversation (SIP-39's bridge, `sigil_net::spawn_cross_call`), which is how
one arrives from the command line, and how one arrives here from an exchange
whose client did it that way.

Neither exchange hears the call. The session key is derived over the two
identities and the two ephemerals, the same way a call at one exchange is,
and no exchange on the path — near or far — ever holds it.

A call carried *to* your exchange rings here like any other — who is
calling, their key in full, Answer and Decline — and says it is from another
exchange, because that is the one thing about it that is different: it is a
caller your exchange cannot vouch for, which is why the key is the thing to
read. Answering opens a session back toward them on the connection you
already hold, and your exchange matches it to the call it is holding.
Declining tells them so, rather than leaving them ringing until their
exchange gives up on yours.
