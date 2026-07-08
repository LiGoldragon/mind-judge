# Agent guidance — mind-judge

Read `ARCHITECTURE.md` before editing.

This repo is a text/model edge adapter, not a core daemon. Do not add Mind
storage, agent-daemon dependencies, provider secrets, or prompt prose. Keep
prompts in `mind-judge-config`, contract records in `signal-mind-judge`, and
provider-generic mechanics in `judge`.
