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
