# Software Library catalog

One file per category (DESIGN.md §10.5, D-055). Each `[[app]]` entry:

| key | meaning |
|---|---|
| `name` | the name shown in LIBRARY and the Software Library, and typed after `INSTALL` |
| `packages` | the Debian / Raspberry Pi packages it installs (the first is "the" package) |
| `alias` | more names for `INSTALL`, and commands RetroDOS knows it by |
| `run` | the command that starts it (its `.desktop` entry's icon is shown once installed) |
| `blurb` | one line about it |
| `note` | optional: shown after it is installed (memory use, how to start it, …) |

Every package must exist for arm64 in trixie (main, contrib) or the
Raspberry Pi archive: `tools/check-catalog.sh` checks.
