# skills — mind-judge

- Depend on `judge`, `signal-mind-judge`, and an external
  `mind-judge-config` path/package.
- Direct provider calls go through `judge`; do not depend on `agent-daemon`.
- Socket-activated/semi-persistent service work must describe this process as
  an adapter, not a core daemon.
- Do not run provider-backed live tests during scaffold work.
