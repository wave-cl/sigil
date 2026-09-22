# If you lose your key

Your account is your key. Lose the key and, until now, you lost the
account: your names, your place in every conversation, the devices you
had linked. SIP-44 lets you say in advance who takes over, and lets the
exchange carry everything across when they do.

Two ways, both signed by you while you still can:

- **A will** names a successor key. Whoever holds that key -- you, on
  another machine, or somebody you trust -- presents the will and takes
  the account. Keep the will apart from the successor's secret: together
  they *are* the account.
- **Guardians** are people you name, and a number of them it takes. When
  the key is gone, that many of them each sign for the key that succeeds
  you, and the successor presents those. No one guardian can move your
  account; you chose how many can.

Both are in **Chat › Devices**, at the foot, under *If you lose your key*:

- **A will**: paste the successor's key, press the tick, and copy the will
  it shows. Keep it where that key's secret is not.
- **Guardians**: add each guardian's key, choose how many it takes, and
  lodge it. The pane shows what is lodged; naming guardians again replaces
  it.
- **As somebody's guardian**: the account and the key that succeeds it;
  copy the vouch it shows and give it to the successor.
- **Take an account that named you**: paste the will; or the account's key
  and, under it, the vouches you were given, one per line. A short quorum
  or a will naming somebody else is refused before anything is sent.

A device linked to the account (holding its credential, not its key) is
offered the last two only, and told why.

The terminal has the same five, and is where this lived until sigil could
reach the routes (sqex 0.104.3):

```
sqex succession will <successor-key>
sqex succession policy --threshold 2 --guardian <key> --guardian <key> --guardian <key>
sqex succession vouch <account> <successor>        # as a guardian
sqex succession claim --will <will>                 # as the successor
sqex succession claim --account <old> --vouch <v> --vouch <v>
sqex succession show <account>
```

## What sigil shows

In every conversation the old key was in, a line: *X's account is now
Y* -- with the old key's own signature behind it, which any client can
check. A session still running as the old key is told where the account
went and does nothing else; link that device to the new key and it is
yours again, history and all (SIP-42 carries it once it is a sibling).

## What moves, and what does not

Names, memberships and roles, block lists, and where you are reachable
(SIP-28) all follow the successor. Epoch keys do not: the successor's
devices are new devices and get keys as any newly linked device does.
A direct message keeps its identifier, which was derived from the old
key; it stays readable, and a new one with the same person starts fresh.
The old key's mailbox and prekeys are the old key's.

Nothing here is the exchange's judgement. It verifies a signature you
made, or ones your guardians made under a policy you signed, and does
what they say; it cannot move an account nobody signed for, and it
records the proof it was shown for anybody to check.
