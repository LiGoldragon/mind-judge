# mind-judge — architecture

`mind-judge` is the Mind-specific text/model edge adapter for judgment calls.

It sits between Mind's typed judge contract and provider-facing adapter
mechanics. It reads prompt/config data from `mind-judge-config`, consumes
`signal-mind-judge`, and uses `judge` for provider/proxy mechanics.

## Service shape

The target service shape is socket-activated or semi-persistent: `mind-judge`
wakes on Unix socket activity, may stay warm briefly, and exits when idle. This
adapter must not be documented or treated as a Mind core daemon. It is a
text/model edge adapter.

## Boundary

Owned here:

- adapter configuration records;
- prompt/config path selection;
- request lowering from `signal-mind-judge` into provider calls through `judge`;
- response parsing back into `signal-mind-judge` replies;
- local adapter process and socket-activation integration when implemented.

Not owned here:

- provider-generic mechanics, which belong in `judge`;
- Mind storage/admission logic, which belongs in `mind`;
- prompt prose, which belongs in `mind-judge-config`;
- eval fixtures/results, which belong in `mind-tests`;
- `agent-daemon` integration.
