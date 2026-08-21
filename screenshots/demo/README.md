# Demo fixture

`demo-data` impersonates the `tickerbar` binary and prints a fixed, invented
watchlist — crypto majors, US stocks, an index, a commodity, a Treasury rate, a
forex pair and an ARS-quoted CEDEARs group — chosen to show every state at once:
up/down/flat rows, a closed market, a stale quote, an errored row, intraday
ranges, two currencies on the section headers, and the watchlist-average summary.
It contains no real portfolio data.

Put it first on PATH and either frontend feeds itself from it:

```bash
PATH="$PWD/screenshots/demo:$PATH" waybar          # waybar module + tooltip
PATH="$PWD/screenshots/demo:$PATH" omarchy-shell   # Quickshell bar widget + panel
```

`--output json` prints the structured document, anything else prints the waybar
module JSON for the same data, and `--no-color[=all|bar|tooltip]` / `NO_COLOR`
behave as in the real CLI — including an unknown value, which produces the same
argument-error document. Colors are not reimplemented: the fixture asks the real
binary (found on PATH with this directory removed) for its resolved `palette`, and
falls back to the built-in One Dark when it is not installed. All timestamps are
computed at run time. This fixture
is documentation tooling only — it is not part of the build, the install target
or the test suite.
