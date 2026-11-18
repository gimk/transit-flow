# street_cross: careful walkers and cars in the way

## Short answer
With the code that is committed now (d782761), the exact cases are: none.
I measured seeds 1 to 4 again with extra output on every crossing start.
Result: 97 crossing starts, 0 starts with a car in the way or too close, 97 of 103 walkers arrived.

## Why the test still allows 10%
- The allowance (missed x 10 <= starts) was written first, when the test failed with
  "seed 2: car AgentId(8) is in the way". At that time the rule "a fully attentive agent
  always notices a visible thing within 40 m" did not exist.
  A careful walker then missed a visible car in a glance (far periphery, 45% chance).
- After the user decision (B) I added the sure-notice rule. The misses went away.
  I did not tighten the test back. The allowance is now slack that is not used.
- I do not have the traits, zone or outcome of that first miss: the run was changed
  before I recorded them. I will not guess them.

## Does it agree with the rule?
Yes. The rule is: fully attentive (field of view 1.0: attention 1, risk 0, which is the
careful archetype), visible, within 40 m, not behind the gaze and not in a blind spot,
then always noticed. Hidden cars and cars out of the field are still missed, and the
walker can still hear them. In these 4 seeds no start had such a case.

## Limits of this answer
- 4 seeds, 97 starts. A miss with probability below about 3% per start could be absent.
- The test counts a start as bad if the car front is past the crossing line, or the car
  reaches the line before the walker has crossed (6 m / walking speed).
  A hidden car (van) is not in this street, so hidden cases are not tested here.
  Hidden cases are tested in street_meet.

## Proposal (no code change made)
Tighten the test to "missed == 0" (the careful walker is fully attentive, so the rule
says never). If the user prefers, I can do it. Instrumentation used for this answer was
temporary and is removed (git status clean).
