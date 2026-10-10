# sultrakey

**English** · [Bahasa Indonesia](README.id.md)

`.env` files with encrypted values, plus a launcher that runs an application in any language
(Node.js, Spring Boot, Go, Rust, and so on) with the decrypted values in its environment.

- One static binary with no dependencies. Runs on Rocky Linux 8/9, Ubuntu, and CentOS 7, plus
  Windows and macOS for developer laptops.
- Encryption uses [age](https://age-encryption.org) X25519, with no home-made cryptography. In an
  emergency, values can still be decrypted with the official `age` CLI.
- Applications need no changes: they read environment variables as usual.
- In a terminal, the marks `✓`, `!`, `✗`, and the `Fix:` label are colored. When output is piped to a
  script or log, the text is identical without color. Set `NO_COLOR=1` to turn color off.

A complete guide for the infra team (server installation, setup, pm2, systemd, Docker) is in
[docs/runbook-infra.md](docs/runbook-infra.md) (in Indonesian).

## Installation

Windows (PowerShell, no Administrator rights needed):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/sultrakey/releases/latest/download/install.ps1 | iex"
```

Linux / macOS:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh        # laptop
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh   # server → /usr/bin
```

Manual installation: download the binary from the [releases page](https://github.com/hmrnsp/sultrakey/releases/latest),
then run `./sultrakey-<target> install` (use `sudo` on a server). From source: `cargo install --path .`.

| System                   | Install location                                                              |
| ------------------------ | ----------------------------------------------------------------------------- |
| Linux/macOS with sudo    | `/usr/bin/sultrakey` (plus the key folder `/etc/sultrakey`)                   |
| Linux/macOS without sudo | `~/.local/bin/sultrakey`                                                      |
| Windows                  | `%LOCALAPPDATA%\Programs\sultrakey\sultrakey.exe` (added to the user's PATH)  |

## Quick start

1. The application repository holds `.env.example` (every variable, with empty values or non-secret
   defaults). The real `.env` must never go into git. Add `.env` and `.env.lock` to `.gitignore`. Do not
   use the pattern `.env*`, which would also ignore `.env.example`.
2. `sultrakey init <app>` creates a key pair and the `.env` from the template.
3. `sultrakey setup` asks for the variables that are still empty, then saves them encrypted.
4. `sultrakey check` makes sure every variable is filled and can be decrypted.
5. `sultrakey run -- <command>` decrypts the values, puts them in the environment, then runs the
   application.

On a Windows laptop:

```powershell
cd C:\projects\example-app
sultrakey init example-app
sultrakey setup
sultrakey run -- npm run dev
```

On Linux/macOS, creating a key needs sudo, laptops included. Keys always live in `/etc/sultrakey/`:

```sh
sudo sultrakey init example-app --owner $USER
sultrakey setup
sultrakey run -- npm run dev
```

## The `setup` screen

![The sultrakey setup screen](docs/Screenshot.png)

In a terminal, `setup` shows every empty variable on one screen. The variable list is on the left, and
the selected variable is on the right.

- Three labels sit under the variable name:
  - `SECRET` / `VISIBLE`: how it is typed (shown as stars and typed twice, or shown as is and typed once).
  - `REQUIRED` / `OPTIONAL`: must be filled, or may be left empty.
  - `ENCRYPTED` / `PLAIN`: how it is stored in `.env`.
- The description (shown dim) comes from the comment above the variable in the template, followed by the
  default value (`Default: ...`) if there is one. A variable without a comment shows
  `No description for this key yet.`
- While you type `@file-path`, the screen tells you right away whether the file exists. The file's
  contents are never shown.
- Enter keeps the value and moves to the next variable. After the last variable, a `Review before saving`
  list appears. Secret values in that list always show as `********`.
- `.env` is written only after `[ Save ]` is chosen. Esc or Ctrl+C cancels without changing anything.
- Variables you skip (moving with ↑↓ without pressing Enter) stay empty.
- F1 shows what the labels mean, the annotations (`@plain`, `@optional`, `@masking`), and every key
  binding. Scroll it with ↑↓; any other key closes it.

## Template

```env
# Application port
# @plain
PORT=8899
# Redis host, without http:// and port. Example: 10.10.1.20
REDIS_HOST=
# Password from the Redis admin. Leave empty if there is none.
# @optional
REDIS_PASSWORD=
# SSL certificate from the network team (fill from a file)
SSL_CERT=
```

- The comment right above a variable becomes its description in `setup`.
- A value in the template becomes the default in `setup`:
  - Visible variables: the default is already on the input line. Enter = use it, Backspace = change it.
    If you clear the whole line and press Enter, an `@optional` variable becomes empty, while a required
    variable is refused until it is filled. Without a terminal, empty visible variables that are not
    sent get the default.
  - Secret variables: the template's value is **ignored** and must be typed, also without a terminal.
    So an example password such as `DB_PASSWORD=secret` is never saved.
  - **Clear example values on visible variables** (for example `SMTP_HOST=smtp.example.com`). Otherwise
    the example is saved as the real value and `check` still passes.
- A value longer than one line is filled from a file: in `setup` type `@/path/to/file`, or use
  `sultrakey set KEY --file <path>`. Type `@@` for a value that really starts with `@`.
- The template's order and comments are copied into `.env`, annotations included. That is why `check`
  and `run` do not need the template.

### Annotations

Annotations are comments above a variable, in the comment block attached to it (no blank line in
between). Without annotations, a variable is **secret and required**: encrypted, and it must be filled
before `run`.

| Annotation    | Effect                                                                                                                                                                                                                              | Without it                                                                                                                                                  |
| ------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `# @plain`    | The value is stored **as is**, not encrypted (label `PLAIN`). For values that are not secret, such as a port or a log level.                                                                                                       | The value is encrypted (`enc:...`, label `ENCRYPTED`). A plain value typed by hand into this variable makes `check` and `run` fail until `setup` encrypts it. |
| `# @optional` | The variable **may be empty** (label `OPTIONAL`). `check` and `run` still work, and the application gets the variable with an empty value. In `setup`, clear the whole line, then press Enter to leave it empty.                    | The variable is required (label `REQUIRED`). `check` and `run` fail (exit code 78) while it is empty.                                                       |
| `# @masking`  | What you type shows as `*` and is typed **twice** in `setup` and `set` (label `SECRET`). The template's default is ignored. For secrets whose name does not look secret, for example `DATABASE_URL=postgres://user:password@host/db`. | How it is typed follows the variable's name (see below).                                                                                                    |

Writing rules:

- Several annotations may share one line (`# @plain @optional`) or sit on separate lines.
- A comment line that starts with `@` may hold annotations only. Unknown annotations (for example the
  typo `# @optinal`) are refused with their line number, so a secret can never silently end up plain.
  The old name `@masked` is refused too; use `@masking`.
- A comment that does not start with `@` is an ordinary description, even with an `@` in the middle
  (`# email @ office`).

How a variable is typed without `@masking`: it still shows `*` when one part of its name (split on `_`,
any case) is `PASSWORD`, `PASSWD`, `PASS`, `PWD`, `SECRET`, `TOKEN`, `AUTH`, or `SALT`. For example,
`REDIS_PASSWORD` shows `*`, while `API_KEY` and `PASSPORT_URL` show as is (add `@masking` if `API_KEY`
must show `*`). How a variable is typed does not affect encryption: only `@plain` decides that.

Example combinations:

| Template                                 | Stored    | While typing                 | May be empty |
| ---------------------------------------- | --------- | ---------------------------- | ------------ |
| `REDIS_HOST=`                            | encrypted | shown as is, once            | no           |
| `REDIS_PASSWORD=`                        | encrypted | `*`, twice                   | no           |
| `# @optional`<br>`REDIS_PASSWORD=`       | encrypted | `*`, twice                   | yes          |
| `# @plain`<br>`PORT=8899`                | as is     | shown as is, `8899` prefilled | no           |
| `# @plain @optional`<br>`LOG_LEVEL=info` | as is     | shown as is, `info` prefilled | yes          |
| `# @masking`<br>`DATABASE_URL=`          | encrypted | `*`, twice                   | no           |

### Other markers

| Marker                         | Where                                                | Meaning                                                                                                                                                                                                                                  |
| ------------------------------ | ---------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SULTRAKEY_APP=<app>`          | First lines of `.env` (written by `init`)            | Application name; picks the key file `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`). Not allowed in the template.                                                                                                 |
| `SULTRAKEY_PUBLIC_KEY=age1...` | First lines of `.env` (written by `init`)            | Public key used to encrypt. Must match the key file; otherwise `check` fails.                                                                                                                                                            |
| `SULTRAKEY_*`                  | Variable names                                       | Prefix reserved for sultrakey. Apart from the two lines above, variables with this prefix are refused in `.env` and the template. Environment variables with this prefix are also never passed to the application by `run`.               |
| `enc:...`                      | Values in `.env`                                     | An encrypted value. Do not edit by hand; replace it with `sultrakey set KEY`.                                                                                                                                                            |
| `@/path/to/file`               | An answer in `setup` (or `KEY=@file` on stdin)       | The value is read from the file's contents, for multi-line values such as certificates. Delete the file afterwards. Without the `@`, the path is saved as plain text (fine for `PUBLIC_KEY_PATH=keys/public_key.pem`).                    |
| `@@...`                        | An answer in `setup`                                 | A value that really starts with `@`. `@@abc` is saved as `@abc`.                                                                                                                                                                         |

The resulting `.env`:

```env
SULTRAKEY_APP=example-app
SULTRAKEY_PUBLIC_KEY=age1...
# Application port
# @plain
PORT=8899
# Redis host, without http:// and port. Example: 10.10.1.20
REDIS_HOST=enc:YWdlLWVuY3J5cHRpb24...
...
```

## Commands

| Command                                                | What it does                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------------------ | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init <app> [--owner user[:group]]`                    | Creates the key if missing (never replaces one). Creates `.env` from the template, or takes over an old plain `.env` by encrypting its values.                                                                                                                                                                                              |
| `setup`                                                | Brings `.env` in line with the template, encrypts plain values in secret variables, then shows every empty variable on one screen (secret variables show `*`). Without a terminal: reads `KEY=value` lines from stdin.                                                                                                                        |
| `set <KEY> [--stdin \| --file F]`                      | Replaces one value. Values are never taken from arguments.                                                                                                                                                                                                                                                                                    |
| `list`                                                 | Shows variable names and their status; values are never shown. In a terminal: a colored table with `STORED` (`ENCRYPTED`/`PLAIN`) and `TYPED` (`SECRET`/`VISIBLE`) columns, plus a summary of the variables that still need `setup`. When piped to a script (`\| grep`, `> file`): the plain `KEY  STATUS` columns, as before.            |
| `check`                                                | Verifies that every required variable is filled, every `enc:` can be decrypted, the key matches, and the key file's permissions are safe. In a terminal: a table of results per variable and for the key file. Problems are always printed as `✗ ...` and `Fix: ...` lines on stderr.                                                          |
| `run [--env F] -- <cmd> [args]`                        | Runs `check`, then runs the application with the values in its environment.                                                                                                                                                                                                                                                                   |
| `install` / `update [--check] [-y]` / `uninstall [-y]` | Installs, updates, or removes the binary.                                                                                                                                                                                                                                                                                                     |

Global options: `--env` (default `./.env`), `--template` (default `./.env.example`), and `--key-file`.

Where the key file is looked for, in order:

1. `--key-file`
2. `SULTRAKEY_KEY_FILE`
3. `$CREDENTIALS_DIRECTORY/sultrakey.key` (systemd `LoadCredential=`, needs systemd ≥ 247: Rocky 9, Ubuntu 22.04+)
4. `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`)

Exit codes: `0` success, `64` wrong usage, `78` configuration problem, `1` anything else. Exit code
`78` makes pm2 (`stop_exit_codes: [78]`) and systemd (`RestartPreventExitStatus=78`) stop trying to
restart the application.

## How `run` works

- Linux/macOS: uses `exec`. The application takes over sultrakey's PID, so pm2/systemd watch the real
  application, and signals and exit codes go straight to it.
- Windows: the application runs as a child process, and its exit code is passed on.
  - Ctrl+C reaches the application.
  - If the terminal is closed forcibly, the application stops with it (Job Object), so ports are not
    left held.
  - `.cmd`/`.bat` commands (`npm`, `npx`, `pnpm`, `yarn`, `mvnw`) are found through `PATHEXT`.
- Values from `.env` win over the existing environment. On a clash, sultrakey prints a warning that
  contains the variable name only.
- Empty `@optional` variables are sent as empty strings.
- `SULTRAKEY_*` variables are not passed to the application.

## Reading values in the application

| Application              | How to read                                                         |
| ------------------------ | ------------------------------------------------------------------- |
| Express / Node.js        | `process.env.REDIS_HOST`                                            |
| SvelteKit (adapter-node) | `import { env } from '$env/dynamic/private'` → `env.REDIS_HOST`     |
| Spring Boot              | `spring.data.redis.host=${REDIS_HOST}` in `application.properties` |
| Go                       | `os.Getenv("REDIS_HOST")`                                           |
| Rust                     | `std::env::var("REDIS_HOST")`                                       |

Example commands: `sultrakey run -- node dist/main.js`, `sultrakey run -- node build` (SvelteKit),
`sultrakey run -- java -jar app.jar`, `sultrakey run -- mvnw.cmd spring-boot:run`, and
`sultrakey run -- go run .`.

**Warnings:**

- **Do not load `.env` yourself in override mode**, for example `dotenv` with `override: true`.
  The application would read the `enc:...` text as the value. Plain `dotenv` (without override) is
  still safe, because it does not replace values set by sultrakey.
- **Frameworks that read environment variables at build time** (SvelteKit `$env/static/*`, Vite
  `import.meta.env`, Next.js `NEXT_PUBLIC_*`) bake values into the build. When the application runs,
  it is too late for sultrakey to fill them, and those values can end up in the browser. Read secrets
  only on the server, at run time (SvelteKit: `$env/dynamic/private`).
- **pm2 cluster mode** cannot run sultrakey. Use fork mode (see the runbook).

## Emergency: decrypting a value without sultrakey

```sh
echo '<base64 text after enc:>' | base64 -d | age -d -i /etc/sultrakey/<app>.key
```

The key file uses the standard age identity format (`AGE-SECRET-KEY-1...`). A lost key means every
value must be entered again, so always keep a backup of the key somewhere offline.

## Development

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                                   # the age compatibility test runs when the `age` CLI is installed
```

- Integration tests use temporary folders (`SULTRAKEY_KEY_DIR`, `SULTRAKEY_INSTALL_DIR`) and a local
  HTTP server (`SULTRAKEY_UPDATE_URL`). Tests never touch the real `/etc/sultrakey` or PATH.
- CI (`.github/workflows/ci.yml`) runs fmt, clippy, and tests on Linux x86/ARM, Windows, and macOS;
  checks the MSRV (1.89) and shellcheck; and smoke-tests the musl binary in a `centos:7` container.
- Releasing: bump `version` in `Cargo.toml`, commit, then run `git tag v0.1.1 && git push origin v0.1.1`.
  The `release.yml` workflow builds binaries for 5 targets, `SHA256SUMS`, `manifest.json`, `install.sh`,
  and `install.ps1`, then publishes them on GitHub Releases.
- Moving to the office GitLab: build with `SULTRAKEY_RELEASE_BASE=<release address>`, publish the same
  files, and adjust the URL shape in `src/update/http.rs` if it differs. The GitLab project must be
  **Public**. "Internal" still needs a login, so `update` and the install scripts would fail.
- The repository must stay public. On a private repository, release downloads need a token.

Code layout:

```
src/envfile/   parser, writer, and template ↔ .env sync
src/crypto.rs  age X25519 + base64
src/keyfile.rs locating, reading, and creating key files
src/commands/  one file per command
src/input/     prompts, the setup screen (ratatui), and @file answers
src/launch/    exec (Unix) / child process + Job Object (Windows)
src/install/   install location and binary replacement
src/update/    manifest, checksum, HTTPS downloads
```

Out of scope for v1: key rotation, a command that prints plain values, and secret manager integration.
