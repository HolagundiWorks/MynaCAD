# Database connect (Tren and Falcon)

Tren and Falcon share one `.osp` file. From the suite build after 1.5.5 that file is **connected to the suite database**: Myna hosts the database service, Kivo signs the company in, and the project file stays on this PC (the local `.osp` is the shared database; nothing is pushed to Neon). Design and contract: Kivo `docs/WORKFLOW-KIVO-HUB.md` and `docs/contracts/session-broker.md`.

## What happens

1. **Open a project file.** Tren (`crates/dbconnect`, crate `aad-dbconnect`) reads the file's `meta` (project id, name, revision) and asks **Kivo** for a session over the named pipe `\\.\pipe\osas-session-broker` (`ask_session`, app `tren`, purpose `db_connect`). If the pipe is not there it starts `O-Studio.exe --broker` and waits up to 60 s. If nobody is signed in, Kivo opens its sign-in.
2. **Connect.** Tren sends `POST /v1/projects/connect` to **Myna** (`127.0.0.1:8743`, or `OSAS_DB_URL`) with the session token as a Bearer token.
3. **Save.** Each save into the `.osp` marks the project dirty; one push (`POST /v1/projects/{id}/push`) goes out about 1.5 s after the last save. Myna answers `local_only` while project files stay on this PC: nothing is sent and the status reads connected. Only if a machine opts in (`OSAS_PUSH_PROJECT_FILES=1`) does the revision go to the shared cloud copy.
4. **A refused token** is replaced once by asking Kivo again.

## What it will not do

- It never blocks the UI and never changes the project file. Drawing, saving and take-off work with Kivo or Myna missing; the state only shows in the status bar.
- It never holds a company password. The token lives in memory only and is never printed.
- It never applies a newer shared revision on its own. A newer remote (`pull`) or a conflict (`409`) is reported and both copies are kept.
- Headless runs (the command line, MCP, the tests) never start Kivo: the connection is off until the desktop shell calls `aad_dbconnect::enable()`. `AAD_DB_CONNECT=0` keeps it off in the shell too.

## Status bar

| Text | Meaning |
|---|---|
| (nothing) | no project connected |
| Database: connecting | asking Kivo, then Myna |
| Database: connected | the shared copy is current (", N to send" while revisions wait) |
| Database: sign in to Kivo | nobody is signed in, or the sign-in was closed |
| Database: offline | Kivo or Myna is not running; saves stay local and are sent when they run |
| Database: problem | the file has no project id, or Kivo refused the user's role; the tooltip says which |

The tooltip carries the one-sentence reason. The same text comes from Falcon's twin client (`Office.Shared/DbConnect`), so both apps say the same thing.

## Tests

`cargo test -p aad-dbconnect`: 15 tests against a fake Kivo and a loopback fake Myna (sign-in, refused token, 401 retry, 409, newer remote, offline, coalesced saves, disabled). `cargo test -p aad-dbconnect -- --ignored --nocapture` talks to the installed Kivo and Myna.
