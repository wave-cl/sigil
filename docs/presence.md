# Presence

The dot on the corner of a mark says whether somebody is there:

- **green, filled — active**: connected, and at the keyboard.
- **amber, filled — away**: connected, and nobody has touched the machine
  for five minutes.
- **hollow — offline**: not connected, or not for long enough to say.

Filled or hollow carries the state on its own, for anybody who cannot tell
the two colours apart, and the word is on the mark for a screen reader. A
pointer over a mark learns when they were last there.

It is drawn on the other person's mark in the list, on every member's in
the Members view, before the name in the bar of a direct message, and on
your own mark at the right of the bar -- which is what everybody else sees
of you. With your link to the exchange down, your own mark carries the
link's word instead: "reconnecting…", "offline".

## How it is known

SIP-4 liveness beacons. Every half minute sigil tells the exchange it is
here, and whether anybody is at the machine; every half minute it asks the
exchange about the people it talks to -- the other party of every direct
message and the members of the open conversation, a few at a time, at most
sixty-four. The exchange answers with when it last heard from them and the
interval they promised; three intervals missed is offline, as SIP-4
advises, and one missed is not.

Away is this machine's judgement: on macOS, five minutes since the last
keypress or click anywhere on the desktop; elsewhere, five minutes since
sigil's own window saw one while it was in front. Somebody back at the
keyboard is said to be back at once, not at the next beat.

## What it discloses

A beat tells the exchange, and through it anyone who can reach the
exchange, that this identity is connected and whether somebody is at it --
every half minute, for as long as sigil runs. That is SIP-4's disclosure,
and it is the price of anybody knowing you are there. An exchange from
before sqex 0.60.0 does not carry *away*: it refuses the bit, sigil beats
plainly to it, and the people reading it see active or offline only.
