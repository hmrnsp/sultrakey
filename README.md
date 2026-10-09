# sultrakey

File `.env` dengan value terenkripsi, dan peluncur yang menjalankan aplikasi bahasa apa pun
(Node.js, Spring Boot, Go, Rust, ...) dengan value yang sudah dibuka di environment.

- Satu binary statis, tanpa dependensi. Jalan di Rocky Linux 8/9, Ubuntu, dan CentOS 7.
  Windows dan macOS untuk laptop developer.
- Enkripsi memakai [age](https://age-encryption.org) X25519, tanpa kriptografi buatan sendiri.
  Value bisa dibuka darurat dengan CLI `age` resmi.
- Aplikasi tidak perlu diubah: aplikasi membaca environment variable seperti biasa.

Panduan untuk tim infra (server, pm2, systemd, Docker): [docs/runbook-infra.md](docs/runbook-infra.md).
Spesifikasi dan keputusan desain: [catatan.md](catatan.md).

## Pasang

Windows (PowerShell, tanpa Administrator):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/sultrakey/releases/latest/download/install.ps1 | iex"
```

Linux / macOS:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh        # laptop
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh   # server → /usr/bin
```

Manual: unduh binary dari [halaman rilis](https://github.com/hmrnsp/sultrakey/releases/latest), lalu
jalankan `./sultrakey-<target> install` (`sudo` di server). Dari source: `cargo install --path .`.

| Sistem | Lokasi pasang |
| --- | --- |
| Linux/macOS dengan sudo | `/usr/bin/sultrakey` (+ folder kunci `/etc/sultrakey`) |
| Linux/macOS tanpa sudo | `~/.local/bin/sultrakey` |
| Windows | `%LOCALAPPDATA%\Programs\sultrakey\sultrakey.exe` (ditambahkan ke PATH user) |

## Alur singkat

1. Repo aplikasi berisi `.env.template` (semua key, value kosong atau default yang bukan rahasia).
   `.env` asli tidak pernah masuk git (tambahkan ke `.gitignore`).
2. `sultrakey init <app>` membuat keypair dan `.env` dari template.
3. `sultrakey fill` menanyakan key yang masih kosong, lalu menyimpannya terenkripsi.
4. `sultrakey check` memastikan semua key terisi dan bisa dibuka.
5. `sultrakey run -- <perintah>` membuka value, mengisinya ke environment, lalu menjalankan aplikasi.

Di laptop Windows:

```powershell
cd C:\proyek\api
sultrakey init api
sultrakey fill
sultrakey run -- npm run dev
```

Di Linux/macOS, membuat kunci butuh sudo, termasuk di laptop. Kunci selalu disimpan di `/etc/sultrakey/`:

```sh
sudo sultrakey init api --owner $USER
sultrakey fill
sultrakey run -- npm run dev
```

## Template

```env
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=
# Password dari admin Redis. Kosongkan bila tanpa password.
# @optional
REDIS_PASSWORD=
# Sertifikat SSL dari tim jaringan (isi dari file)
SSL_CERT=
```

- Komentar tepat di atas key menjadi teks bantuan saat `fill`.
- `# @plain`: value tidak dienkripsi (bukan rahasia). `# @optional`: boleh kosong.
  Keduanya boleh ditulis dalam satu baris (`# @plain @optional`). Anotasi lain ditolak, supaya salah
  ketik tidak lolos.
- Value di template menjadi default yang ditawarkan saat `fill` (Enter = pakai default).
- Value yang panjangnya lebih dari satu baris diisi dari file: saat `fill` ketik `@/lokasi/file`,
  atau pakai `sultrakey set KEY --file <lokasi>`. Ketik `@@` untuk value yang memang diawali `@`.
- Urutan dan komentar template ikut disalin ke `.env`, bersama anotasinya. Karena itu `check` dan `run`
  tidak butuh template.

Hasil `.env`:

```env
SULTRAKEY_APP=api
SULTRAKEY_PUBLIC_KEY=age1...
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=enc:YWdlLWVuY3J5cHRpb24...
...
```

## Perintah

| Perintah | Fungsi |
| --- | --- |
| `init <app> [--owner user[:group]]` | Buat kunci bila belum ada (tidak pernah menimpa). Buat `.env` dari template, atau ambil alih `.env` polos lama dengan mengenkripsi value-nya. |
| `fill` | Samakan `.env` dengan template, enkripsi value polos di key rahasia, lalu tanyakan semua key kosong. Tanpa terminal: baca baris `KEY=value` dari stdin. |
| `set <KEY> [--stdin \| --file F]` | Ganti satu value. Value tidak pernah diambil dari argumen. |
| `list` | Nama key dan statusnya. Value tidak pernah ditampilkan. |
| `check` | Semua key wajib terisi, semua `enc:` bisa dibuka, kunci cocok, izin file kunci aman. |
| `run [--env F] -- <cmd> [args]` | `check`, lalu jalankan aplikasi dengan value di environment. |
| `install` / `update [--check] [-y]` / `uninstall [-y]` | Pasang, perbarui, atau hapus binary. |

Opsi global: `--env` (bawaan `./.env`), `--template` (bawaan `./.env.template`), `--key-file`.

Urutan mencari kunci:

1. `--key-file`
2. `SULTRAKEY_KEY_FILE`
3. `$CREDENTIALS_DIRECTORY/sultrakey.key` (systemd `LoadCredential=`, butuh systemd ≥ 247: Rocky 9, Ubuntu 22+)
4. `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`)

Exit code: `0` sukses, `64` salah pakai, `78` konfigurasi salah, `1` lainnya. Exit `78` membuat
pm2 (`stop_exit_codes: [78]`) dan systemd (`RestartPreventExitStatus=78`) berhenti mencoba restart.

## Detail `run`

- Linux/macOS: `exec`. Aplikasi mengambil alih PID sultrakey, jadi pm2/systemd memantau aplikasi aslinya,
  dan sinyal serta exit code langsung sampai ke aplikasi.
- Windows: aplikasi dijalankan sebagai proses anak, dan exit code-nya diteruskan.
  - Ctrl+C sampai ke aplikasi.
  - Bila terminal ditutup paksa, aplikasi ikut mati (Job Object), jadi port tidak tertahan.
  - Perintah `.cmd`/`.bat` (`npm`, `npx`, `pnpm`, `yarn`, `mvnw`) ditemukan lewat `PATHEXT`.
- Value dari `.env` mengalahkan environment yang sudah ada. Bila bentrok, sultrakey mencetak peringatan
  berisi nama key saja.
- Key `@optional` yang kosong dikirim sebagai string kosong.
- Variabel `SULTRAKEY_*` tidak diteruskan ke aplikasi.

## Membaca value di aplikasi

| Aplikasi | Cara membaca |
| --- | --- |
| Express / Node.js | `process.env.REDIS_HOST` |
| SvelteKit (adapter-node) | `import { env } from '$env/dynamic/private'` → `env.REDIS_HOST` |
| Spring Boot | `spring.data.redis.host=${REDIS_HOST}` di `application.properties` |
| Go | `os.Getenv("REDIS_HOST")` |
| Rust | `std::env::var("REDIS_HOST")` |

Contoh perintah: `sultrakey run -- node dist/main.js`, `sultrakey run -- node build` (SvelteKit),
`sultrakey run -- java -jar app.jar`, `sultrakey run -- mvnw.cmd spring-boot:run`, `sultrakey run -- go run .`.

**Peringatan:**

- **Jangan memuat `.env` sendiri dengan mode menimpa**, misalnya `dotenv` dengan `override: true`.
  Aplikasi akan membaca teks `enc:...` sebagai value. `dotenv` biasa (tanpa override) masih aman,
  karena tidak menimpa value dari sultrakey.
- **Framework yang membaca env saat build** (SvelteKit `$env/static/*`, Vite `import.meta.env`,
  Next.js `NEXT_PUBLIC_*`) menanam value ke hasil build. Saat aplikasi jalan, sultrakey sudah terlambat
  untuk mengisinya, dan value itu bisa terkirim ke browser. Secret hanya boleh dibaca di sisi server saat
  aplikasi jalan (SvelteKit: `$env/dynamic/private`).
- **pm2 mode cluster** tidak bisa menjalankan sultrakey. Pakai mode fork (lihat runbook).

## Darurat: buka value tanpa sultrakey

```sh
echo '<teks base64 setelah enc:>' | base64 -d | age -d -i /etc/sultrakey/<app>.key
```

File kunci memakai format identity age standar (`AGE-SECRET-KEY-1...`). Kunci hilang berarti semua value
harus diisi ulang, jadi selalu simpan backup kunci di tempat yang offline.

## Pengembangan

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                                   # test kompatibilitas age jalan bila CLI `age` terpasang
```

- Integration test memakai folder sementara (`SULTRAKEY_KEY_DIR`, `SULTRAKEY_INSTALL_DIR`) dan server HTTP
  lokal (`SULTRAKEY_UPDATE_URL`). Test tidak pernah menyentuh `/etc/sultrakey` atau PATH asli.
- CI (`.github/workflows/ci.yml`) menjalankan fmt, clippy, test di Linux x86/ARM, Windows, dan macOS,
  cek MSRV 1.89, shellcheck, serta smoke test binary musl di container `centos:7`.
- Rilis: naikkan `version` di `Cargo.toml`, commit, lalu `git tag v0.1.1 && git push origin v0.1.1`.
  Workflow `release.yml` membangun binary untuk 5 target, `SHA256SUMS`, `manifest.json`, `install.sh`,
  dan `install.ps1`, lalu menerbitkannya di GitHub Releases.
- Pindah ke GitLab kantor: build dengan `SULTRAKEY_RELEASE_BASE=<alamat rilis>`, terbitkan file yang sama,
  dan sesuaikan bentuk URL di `src/update/http.rs` bila berbeda. Proyek GitLab harus berstatus **Public**.
  Status "Internal" tetap butuh login, sehingga `update` dan skrip pasang akan gagal.
- Repo harus tetap publik. Di repo private, unduhan rilis butuh token.

Struktur kode:

```
src/envfile/   parser, penulis, dan sinkronisasi template ↔ .env
src/crypto.rs  age X25519 + base64
src/keyfile.rs lokasi, baca, dan buat file kunci
src/commands/  satu file per perintah
src/launch/    exec (Unix) / proses anak + Job Object (Windows)
src/install/   lokasi pasang dan penggantian binary (diadaptasi dari lopi)
src/update/    manifest, checksum, unduhan HTTPS
```

Di luar cakupan v1: rotasi kunci, perintah untuk mencetak value polos, dan integrasi dengan secret manager.
