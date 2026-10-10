# sultrakey runbook for the infra team

`sultrakey` keeps application passwords in the `.env` file, encrypted. When the application starts,
`sultrakey` decrypts those passwords and hands them to the application. The application's code does
not need to change.

Ground rules:

- Every error message has two lines: the problem (`✗ ...`) and the command that fixes it (`Fix: ...`).
- `sultrakey` never shows a password.
- The key file (`/etc/sultrakey/<app>.key`) must be backed up. **A lost key means every password must
  be entered again.**

## 1. Install sultrakey on a server

Servers that can reach github.com:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh
sultrakey --version
```

Servers without internet:

1. Download `sultrakey-x86_64-unknown-linux-musl` (ARM: `sultrakey-aarch64-unknown-linux-musl`) from
   the releases page on another computer.
2. Copy the file to the server, for example with `scp`.
3. Run:

   ```sh
   chmod +x sultrakey-x86_64-unknown-linux-musl
   sudo ./sultrakey-x86_64-unknown-linux-musl install
   ```

The program is now in `/usr/bin/sultrakey` and the key folder `/etc/sultrakey` exists.
This one binary runs on Rocky Linux 8/9, Ubuntu, and CentOS 7.

## 2. Set up a new application

Example: the application `lakupandai` in `/opt/lakupandai`, run by the Linux user `lakupandai`.
The application folder must contain `.env.example` (brought by the developers through git).
Values filled in `.env.example` become defaults for visible keys. If a value is a fake example
(for example `SMTP_HOST=smtp.example.com`), ask the developers to clear it first. Values on secret
keys are always ignored.

```sh
cd /opt/lakupandai
sudo sultrakey init lakupandai --owner lakupandai    # create the key + .env
sudo sultrakey setup                                  # fill everything that is still empty
sudo -u lakupandai sultrakey check                   # make sure everything is fine
```

`setup` shows every empty key on one screen: the list on the left, the selected key on the right.

- Under each key name, three labels say how it works:
  - `SECRET` or `VISIBLE`: secret keys (names containing PASSWORD, SECRET, TOKEN, and so on, or
    marked `@masking`) show `*` and are typed **twice**. Other keys are visible while typed.
  - `REQUIRED` or `OPTIONAL`: an optional key may be left empty.
  - `ENCRYPTED` or `PLAIN`: how the value is stored in `.env`.
- For visible keys, the default is already on the input line. Press Enter to use it, or delete it
  with Backspace and type a new value. Clear it all, then Enter, to leave an optional key empty.
- Secret keys have no default: they are always typed.
- Values longer than one line (certificates, key files) are filled from a file: type `@/tmp/cert.pem`.
  The screen says right away whether the file exists. Delete the file afterwards.
- Enter moves to the next key. After the last key, every answer is shown for review (secret keys stay
  hidden as `********`). Choose `[ Save ]` to save, or `[ Back to edit ]` to change an answer: pick
  the key with ↑↓ in the list.
- Press Esc or Ctrl+C to stop. Nothing is saved until you choose Save.
- F1 shows what every label and key does.

**Back up** `/etc/sultrakey/lakupandai.key` right away, somewhere safe and offline.

## 3. Change one password

```sh
sudo sultrakey set DB_PASSWORD                            # typed twice
sudo sultrakey set SSL_CERT --file /tmp/cert.pem          # from a file; delete the file afterwards
sudo -u lakupandai sultrakey check
```

Then restart the application so it uses the new password.

See the keys and their status (passwords are never shown):

```sh
sultrakey list
```

## 4. Running the application

The pattern is the same for every application: put `sultrakey run --` in front of the usual start
command.

### pm2 (fork mode)

```js
// ecosystem.config.js
module.exports = {
  apps: [{
    name: 'lakupandai',
    cwd: '/opt/lakupandai',
    script: '/usr/bin/sultrakey',
    args: 'run -- node dist/main.js',
    interpreter: 'none',
    exec_mode: 'fork',
    instances: 1,
    stop_exit_codes: [78],
    // other settings (logs, max_memory_restart, ...) stay as usual
  }]
};
```

- `stop_exit_codes: [78]`: when a password is missing or the key is wrong, pm2 stops instead of
  restarting over and over. This needs a recent pm2; check with `pm2 --version`.
- Run pm2 as the application user (`lakupandai`), because only that user may read the key.
- **Do not** run `sultrakey run -- pm2 start ...`. That way pm2 keeps the decrypted passwords, and
  `pm2 save` writes them **in plain text** to `~/.pm2/dump.pm2`.

### pm2 cluster mode

pm2 cluster mode cannot run `sultrakey`, because it only accepts JavaScript files.

1. First check whether the application really needs cluster mode. Run `pm2 list` or `pm2 monit`
   during busy hours.
   - CPU per instance is low (under ~50%): move to fork mode with 1 process, as above.
   - CPU of one instance is often near 100%: use systemd + `pm2-runtime` (try it on one server first):

     ```ini
     ExecStart=/usr/bin/sultrakey run -- /usr/bin/pm2-runtime start ecosystem.config.js
     ```

2. Another option: several fork processes on different ports, with nginx spreading the load.

Side effects of cluster mode that often go unnoticed:

- Memory is multiplied by the number of instances.
- Scheduled jobs (cron) run several times.
- In-memory data (such as login sessions) is not shared between instances.
- WebSockets need extra configuration.

### systemd

```ini
# /etc/systemd/system/lakupandai.service
[Unit]
Description=Lakupandai
After=network.target

[Service]
User=lakupandai
WorkingDirectory=/opt/lakupandai
ExecStart=/usr/bin/sultrakey run --env /opt/lakupandai/.env -- /usr/bin/java -jar /opt/lakupandai/app.jar
Restart=on-failure
RestartPreventExitStatus=78

[Install]
WantedBy=multi-user.target
```

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now lakupandai
journalctl -u lakupandai -n 50        # if it fails, the ✗ and Fix lines are here
```

### Docker

```dockerfile
COPY sultrakey-x86_64-unknown-linux-musl /usr/local/bin/sultrakey
USER 1001
ENTRYPOINT ["sultrakey", "run", "--"]
CMD ["node", "dist/main.js"]
```

```yaml
# docker-compose.yml
services:
  lakupandai:
    image: lakupandai:latest
    working_dir: /app
    volumes:
      - /opt/lakupandai/.env:/app/.env:ro
      - /etc/sultrakey/lakupandai.key:/etc/sultrakey/lakupandai.key:ro
```

On the server, the key file must be owned by the same uid as the user inside the image. For example:
`sudo chown 1001 /etc/sultrakey/lakupandai.key`, keeping mode `400`.
CentOS 6 is supported through Docker only.

## 5. Common error messages

| Message | Meaning | What to do |
| --- | --- | --- |
| `✗ X is empty.` | A required key is still empty. | `sudo sultrakey setup` |
| `✗ X is stored plain ...` | A password was typed in by hand without encryption. | `sudo sultrakey setup` (encrypts it) |
| `✗ No permission to read key file ...` | Run by the wrong user. | Run as the application user: `sudo -u <user> sultrakey check` |
| `✗ Key file ... is too open ...` | Other users can read the key file. | `sudo chmod 400 /etc/sultrakey/<app>.key` |
| `✗ Key file ... does not belong to .env ...` | The key and `.env` come from different applications or servers. | Use the right key, or restore it from a backup. |
| `✗ Key file ... not found.` | The key is lost or in the wrong place. | Restore it from a backup. Without a backup: delete `.env`, run `init` and `setup` again. |
| `✗ X cannot be decrypted ...` | One key's value is damaged. | `sudo sultrakey set X` |
| `✗ Command 'node' not found.` | The application command is wrong. | Give its full path, for example `/usr/bin/node`. |
| `✗ .env line N: annotation '@masked' was renamed to @masking ...` | The file was written for sultrakey 0.4 or older. | Replace `# @masked` with `# @masking` in `.env` and `.env.example`. |

Exit codes: `0` success, `64` mistyped command, `78` configuration problem (the application is not
restarted), `1` anything else.

## 6. Take over an old plain `.env`

```sh
cd /opt/lakupandai
sudo sultrakey init lakupandai --owner lakupandai
```

Every filled password is encrypted right away, except keys marked `@plain` in the template.
`sultrakey` prints the names of the encrypted keys. The passwords themselves are not printed.

## 7. Update and remove

```sh
sultrakey update --check     # check only
sudo sultrakey update        # update /usr/bin/sultrakey
```

- Updates never run on their own.
- Running applications are not affected. They use the new version from their next restart.
- Servers without internet: copy the new binary, then `sudo ./sultrakey-... install`.

**Updating from 0.4 or older to 0.5:** before updating, replace `# @masked` with `# @masking` in
every `.env` and `.env.example` on the server. Otherwise `check` and `run` refuse the file and the
application does not start at its next restart. To find them:

```sh
sudo grep -rln '@masked' /opt --include='.env' --include='.env.example'
```

Removing the program:

```sh
sudo sultrakey uninstall
```

`uninstall` does not delete key files in `/etc/sultrakey/` and does not touch `.env`.

## 8. Key backups

- Keep a copy of `/etc/sultrakey/*.key` somewhere safe and offline (not on the same server, not in
  git).
- To restore: copy it back to `/etc/sultrakey/`, then `sudo chown <app-user> <file>` and
  `sudo chmod 400 <file>`.
- Anyone who can **write** `.env` can replace passwords. So guard write access to the application
  folder.
