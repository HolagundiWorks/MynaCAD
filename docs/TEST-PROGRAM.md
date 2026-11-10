# Test program

Every feature of the suite is tested by a written plan, app by app and then connected. The plans, the test environment, the bug process, the bug log and the report template are in the MynaHub repository: `docs/testing/`.

| Document | What |
|---|---|
| `docs/testing/README.md` | The programme: order, rules, severity, coverage map |
| `docs/testing/TEST-ENVIRONMENT.md` | Build, install, test data, tools |
| `docs/testing/PLAN-O-DRAFT.md` | The plan for this app |
| `docs/testing/PLAN-INTEGRATION.md` | The apps working together |
| `docs/testing/BUG-PROCESS.md`, `BUG-LOG.md` | Reporting, fixing and verifying bugs |

Fixes for bugs in this repository are made here on a branch `fix/BUG-<id>-<name>`, with a regression test, as described in `BUG-PROCESS.md`. Names, files and links of the whole suite: `docs/SUITE.md`.
