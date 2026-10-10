# sultrakey

`.env` files with encrypted values, and a launcher that runs an application in any language
(Node.js, Spring Boot, Go, Rust, ...) with the decrypted values in its environment.

- One static binary, no dependencies. Runs on Rocky Linux 8/9, Ubuntu, and CentOS 7, plus
  Windows and macOS for developer laptops.
- Encryption uses [age](https://age-encryption.org) X25519, with no home-made cryptography.
  Values can be decrypted in an emergency with the official `age` CLI.
- Applications need no changes: they read environment variables as usual.

Guide for the infra team (servers, pm2, systemd, Docker): [docs/runbook-infra.md](docs/runbook-infra.md).

## Install

Windows (PowerShell, no Administrator needed):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/sultrakey/releases/latest/download/install.ps1 | iex"
```

Linux / macOS:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh        # laptop
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh   # server → /usr/bin
```

By hand: download the binary from the [releases page](https://github.com/hmrnsp/sultrakey/releases/latest),
then run `./sultrakey-<target> install` (`sudo` on a server). From source: `cargo install --path .`.

| System | Install location |
| --- | --- |
| Linux/macOS with sudo | `/usr/bin/sultrakey` (+ key folder `/etc/sultrakey`) |
| Linux/macOS without sudo | `~/.local/bin/sultrakey` |
| Windows | `%LOCALAPPDATA%\Programs\sultrakey\sultrakey.exe` (added to the user PATH) |

## Upgrading to 0.5

Version 0.5 changes two things that can break an existing setup:

1. **`@masked` is renamed to `@masking`.** Replace `# @masked` with `# @masking` in **both**
   `.env.example` and `.env` **before** updating. A file that still says `@masked` is refused, so
   `check` and `run` fail and the application does not start. The error names the file and line:

   ```
   ✗ .env line 7: annotation '@masked' was renamed to @masking; replace it in this file.
   ```

2. **All output is now in English.** For example `Solusi:` is now `Fix:`, and `list` shows
   `encrypted` instead of `terenkripsi`. Scripts that search for the old text need updating.

## Quick start

1. The application repo holds `.env.example` (every key, with empty values or non-secret defaults).
   The real `.env` never goes into git (add `.env` to `.gitignore`; not the pattern `.env*`, which also
   keeps `.env.example` out of git).
2. `sultrakey init <app>` creates a key pair and the `.env` from the template.
3. `sultrakey setup` asks for the keys that are still empty, then saves them encrypted.
4. `sultrakey check` makes sure every key is filled and can be decrypted.
5. `sultrakey run -- <command>` decrypts the values, puts them in the environment, then runs the
   application.

On a Windows laptop:

```powershell
cd C:\projects\api
sultrakey init api
sultrakey setup
sultrakey run -- npm run dev
```

On Linux/macOS, creating a key needs sudo, laptops included. Keys always live in `/etc/sultrakey/`:

```sh
sudo sultrakey init api --owner $USER
sultrakey setup
sultrakey run -- npm run dev
```

## The `setup` screen

In a terminal, `setup` shows every empty key on one screen. The key list is on the left, the
selected key on the right.

- Three labels under the key name: `SECRET`/`VISIBLE` (how it is typed), `REQUIRED`/`OPTIONAL`, and
  `ENCRYPTED`/`PLAIN` (how it is stored).
- The description comes from the comment above the key in the template, followed by the default
  value, if any.
- While you type `@file-path`, the screen tells you right away whether the file exists. The file's
  contents are never shown.
- Enter keeps the value and moves to the next key. After the last key, a list appears to review
  before saving. Secret values in that list always show as `********`.
- `.env` is written only after choosing Save. Esc or Ctrl+C cancels without changing anything.
- Keys you skip (moving with ↑↓ without Enter) stay empty.
- F1 shows what the labels mean and every key binding.

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

- The comment right above a key becomes its help text in `setup`.
- A value in the template becomes the default in `setup`:
  - Visible keys: the default is already on the input line. Enter = use it, Backspace = change it.
    Clear it all, then Enter: an `@optional` key becomes empty, a required key is refused until
    filled. Without a terminal, empty visible keys that are not sent get the default.
  - Secret keys: the template's value is **ignored** and must be typed, also without a terminal. So an
    example password such as `DB_PASSWORD=secret` is never saved.
  - **Clear example values on visible keys** (for example `SMTP_HOST=smtp.example.com`). Otherwise the
    example is saved as the real value and `check` still passes.
- Values longer than one line are filled from a file: in `setup` type `@/path/to/file`, or use
  `sultrakey set KEY --file <path>`. Type `@@` for a value that really starts with `@`.
- The template's order and comments are copied into `.env`, annotations included. That is why `check`
  and `run` do not need the template.

### Annotations (tags)

Annotations are comments above a key, in the comment block attached to it (no blank line in between).
Without annotations a key is **secret and required**: encrypted, and it must be filled before `run`.

| Annotation | Effect | Without it |
| --- | --- | --- |
| `# @plain` | The value is stored **plain**, not encrypted. For values that are not secret, such as a port or log level. | The value is encrypted (`enc:...`). A plain value typed by hand into this key makes `check` and `run` fail until `setup` encrypts it. |
| `# @optional` | The key **may be empty**. `check` and `run` still work, and the application gets the variable with an empty value. In `setup`, clear the line, then Enter = leave it empty. | The key is required. `check` and `run` fail (exit 78) while it is empty. |
| `# @masking` | Typing shows `*` and the value is typed **twice** in `setup` and `set`. The template's default is ignored. For secrets whose name does not look secret, for example `DATABASE_URL=postgres://user:password@host/db`. | How the key is typed follows its name (see below). |

Writing rules:

- Several annotations may share one line (`# @plain @optional`) or use separate lines.
- A comment line that starts with `@` may hold annotations only. Unknown annotations (for example the
  typo `# @optinal`) are refused with their line number, so a secret can never silently end up plain.
  The old name `@masked` is refused too, with a message saying it was renamed to `@masking`.
- A comment that does not start with `@` is plain help text, even with an `@` in the middle
  (`# email @ office`).

Typing without `@masking`: a key still shows `*` when one part of its name (split on `_`, any case) is
`PASSWORD`, `PASSWD`, `PASS`, `PWD`, `SECRET`, `TOKEN`, `AUTH`, or `SALT`. For example `REDIS_PASSWORD`
shows `*`; `API_KEY` and `PASSPORT_URL` are visible (add `@masking` if `API_KEY` must show `*`). How a
key is typed does not affect encryption: only `@plain` decides that.

Combinations:

| Template | Stored | While typing | May be empty |
| --- | --- | --- | --- |
| `REDIS_HOST=` | encrypted | visible, once | no |
| `REDIS_PASSWORD=` | encrypted | `*`, twice | no |
| `# @optional`<br>`REDIS_PASSWORD=` | encrypted | `*`, twice | yes |
| `# @plain`<br>`PORT=8899` | plain | visible, `8899` prefilled | no |
| `# @plain @optional`<br>`LOG_LEVEL=info` | plain | visible, `info` prefilled | yes |
| `# @masking`<br>`DATABASE_URL=` | encrypted | `*`, twice | no |

### Other markers

| Marker | Where | Meaning |
| --- | --- | --- |
| `SULTRAKEY_APP=<app>` | First lines of `.env` (written by `init`) | Application name; picks the key file `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`). Not allowed in the template. |
| `SULTRAKEY_PUBLIC_KEY=age1...` | First lines of `.env` (written by `init`) | Public key used to encrypt. Must match the key file; otherwise `check` fails. |
| `SULTRAKEY_*` | Key names | Prefix reserved for sultrakey. Apart from the two lines above, keys with this prefix are refused in `.env` and the template. Environment variables with this prefix are never passed to the application by `run`. |
| `enc:...` | Values in `.env` | An encrypted value. Do not edit by hand; replace it with `sultrakey set KEY`. |
| `@/path/to/file` | An answer in `setup` (or `KEY=@file` on stdin) | The value is read from the file, for multi-line values such as certificates. Delete the file afterwards. Without `@`, the path is saved as plain text (fine for `PUBLIC_KEY_PATH=keys/public_key.pem`). |
| `@@...` | An answer in `setup` | A value that really starts with `@`. `@@abc` is saved as `@abc`. |

The resulting `.env`:

```env
SULTRAKEY_APP=api
SULTRAKEY_PUBLIC_KEY=age1...
# Application port
# @plain
PORT=8899
# Redis host, without http:// and port. Example: 10.10.1.20
REDIS_HOST=enc:YWdlLWVuY3J5cHRpb24...
...
```

## Commands

| Command | What it does |
| --- | --- |
| `init <app> [--owner user[:group]]` | Create the key if missing (never replaces one). Create `.env` from the template, or take over an old plain `.env` by encrypting its values. |
| `setup` | Bring `.env` in line with the template, encrypt plain values in secret keys, then show every empty key on one screen (secret keys show `*`). Without a terminal: read `KEY=value` lines from stdin. |
| `set <KEY> [--stdin \| --file F]` | Replace one value. Values are never taken from arguments. |
| `list` | Key names and their status. Values are never shown. |
| `check` | Every required key filled, every `enc:` decryptable, the key matches, key file permissions are safe. |
| `run [--env F] -- <cmd> [args]` | `check`, then run the application with the values in its environment. |
| `install` / `update [--check] [-y]` / `uninstall [-y]` | Install, update, or remove the binary. |

Global options: `--env` (default `./.env`), `--template` (default `./.env.example`), `--key-file`.

Where the key is looked for, in order:

1. `--key-file`
2. `SULTRAKEY_KEY_FILE`
3. `$CREDENTIALS_DIRECTORY/sultrakey.key` (systemd `LoadCredential=`, needs systemd ≥ 247: Rocky 9, Ubuntu 22+)
4. `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`)

Exit codes: `0` success, `64` wrong usage, `78` configuration problem, `1` anything else. Exit `78`
makes pm2 (`stop_exit_codes: [78]`) and systemd (`RestartPreventExitStatus=78`) stop restarting.

## How `run` works

- Linux/macOS: `exec`. The application takes over sultrakey's PID, so pm2/systemd watch the real
  application, and signals and exit codes go straight to it.
- Windows: the application runs as a child process, and its exit code is passed on.
  - Ctrl+C reaches the application.
  - If the terminal is closed forcibly, the application dies with it (Job Object), so ports are not
    left held.
  - `.cmd`/`.bat` commands (`npm`, `npx`, `pnpm`, `yarn`, `mvnw`) are found through `PATHEXT`.
- Values from `.env` win over the existing environment. On a clash, sultrakey prints a warning with
  the key name only.
- Empty `@optional` keys are sent as empty strings.
- `SULTRAKEY_*` variables are not passed to the application.

## Reading values in the application

| Application | How to read |
| --- | --- |
| Express / Node.js | `process.env.REDIS_HOST` |
| SvelteKit (adapter-node) | `import { env } from '$env/dynamic/private'` → `env.REDIS_HOST` |
| Spring Boot | `spring.data.redis.host=${REDIS_HOST}` in `application.properties` |
| Go | `os.Getenv("REDIS_HOST")` |
| Rust | `std::env::var("REDIS_HOST")` |

Example commands: `sultrakey run -- node dist/main.js`, `sultrakey run -- node build` (SvelteKit),
`sultrakey run -- java -jar app.jar`, `sultrakey run -- mvnw.cmd spring-boot:run`, `sultrakey run -- go run .`.

**Warnings:**

- **Do not load `.env` yourself in override mode**, for example `dotenv` with `override: true`.
  The application would read the `enc:...` text as the value. Plain `dotenv` (without override) is
  still safe, because it does not replace values set by sultrakey.
- **Frameworks that read env at build time** (SvelteKit `$env/static/*`, Vite `import.meta.env`,
  Next.js `NEXT_PUBLIC_*`) bake values into the build. When the application runs, it is too late for
  sultrakey to fill them, and those values can end up in the browser. Read secrets only on the server,
  at run time (SvelteKit: `$env/dynamic/private`).
- **pm2 cluster mode** cannot run sultrakey. Use fork mode (see the runbook).

## Emergency: decrypt a value without sultrakey

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
- CI (`.github/workflows/ci.yml`) runs fmt, clippy, and tests on Linux x86/ARM, Windows, and macOS,
  checks MSRV 1.89, runs shellcheck, and smoke-tests the musl binary in a `centos:7` container.
- Releasing: bump `version` in `Cargo.toml`, commit, then `git tag v0.1.1 && git push origin v0.1.1`.
  The `release.yml` workflow builds binaries for 5 targets, `SHA256SUMS`, `manifest.json`, `install.sh`,
  and `install.ps1`, then publishes them on GitHub Releases.
- Moving to the office GitLab: build with `SULTRAKEY_RELEASE_BASE=<release address>`, publish the same
  files, and adjust the URL shape in `src/update/http.rs` if it differs. The GitLab project must be
  **Public**. "Internal" still needs a login, so `update` and the install scripts would fail.
- The repo must stay public. In a private repo, release downloads need a token.

Code layout:

```
src/envfile/   parser, writer, and template ↔ .env sync
src/crypto.rs  age X25519 + base64
src/keyfile.rs locating, reading, and creating key files
src/commands/  one file per command
src/input/     prompts, the setup screen (ratatui), and @file answers
src/launch/    exec (Unix) / child process + Job Object (Windows)
src/install/   install location and binary replacement (adapted from lopi)
src/update/    manifest, checksum, HTTPS downloads
```

Out of scope for v1: key rotation, a command that prints plain values, and secret manager integration.
