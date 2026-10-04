# Teleport disconnect repair

The client report from 4 October 2026 at 17:48:31 identifies a rejected `set_objective` packet: `An objective with the name 'redpiler_status' already exists!` The server's later `UnexpectedEof` warning reports the closed connection; it does not identify the original client error. Other EOF warnings may have different causes.

Each plot sends a create-objective packet when a player enters. Leaving previously reset the score entries but kept the objective on the client. `/tp <player>` leaves and re-enters a plot even when both players are already in the same plot, causing a duplicate create and a Minecraft protocol disconnect.

Plot departure now sends objective removal (mode 1), which also clears its scores and sidebar association. Destination entry can then create the objective normally. Tick speed, animations and packet decoding are unchanged.

Regression check:

```text
cargo build --locked
py tools/run_scoreboard_smoke.py
```

Use the Node dependencies documented in `tools/README.md`. The runner uses a temporary world on localhost port 25585. Its two clients track objective existence and reject duplicate creation, updates before creation and unmatched removal. It reproduced the duplicate-objective failure before the repair.

After the repair it covers repeated same-plot and cross-plot teleports, a missing target, self-teleport at 100 TPS, coordinate teleport within the same plot and reconnect. The server stops cleanly afterward. This validates objective lifecycle; it does not establish the cause of every EOF warning in the supplied server log.
