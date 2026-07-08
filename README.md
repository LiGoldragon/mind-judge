# mind-judge

Mind judge edge adapter executable and library.

`mind-judge` consumes `signal-mind-judge`, reads prompt/config data from a
configured `mind-judge-config` checkout or package path, and will call model
providers through `judge`. It does not depend on `agent-daemon` and is not a
Mind core daemon.
