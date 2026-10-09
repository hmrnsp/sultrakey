# Tugas: bangun `sultrakey` — CLI Rust untuk file .env terenkripsi

## Konteks

- Kami punya banyak aplikasi (Node.js, Spring Boot, Go, Rust) yang dideploy tim infra (bukan programmer)
  ke server Rocky Linux 8/9, Ubuntu, dan CentOS 7 (CentOS 6 hanya lewat Docker), memakai pm2, Docker, atau systemd.
- Masalah: secret di `.env` tersimpan polos dan pernah bocor lewat git. Sistem lama terlalu rumit.
- Tujuan: satu binary mandiri yang membuat `.env` berisi value terenkripsi, lalu menjalankan aplikasi
  bahasa apa pun dengan value yang sudah dibuka. Alat ini TIDAK boleh bergantung pada aplikasi mana pun.

## Konsep alur

1. Repo aplikasi hanya berisi `.env.template` (semua key, value kosong atau default non-rahasia).
   `.env` asli dibuat di server dan masuk `.gitignore` (kalau `.env` di-track, `git pull` gagal setelah diisi).
2. `sudo sultrakey init <app> --owner <user>` membuat keypair age X25519:
   private key → `/etc/sultrakey/<app>.key` (0400, milik user aplikasi), public key → header `.env`.
3. `sultrakey fill` menanyakan key yang masih kosong, mengenkripsi value, menyimpannya ke `.env`.
4. `sultrakey check` memastikan semua key wajib terisi dan bisa dibuka.
5. `sultrakey run -- <perintah>` membuka value, mengisinya ke environment, lalu exec ke aplikasi.

Contoh `.env.template`:

```env
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=
# Password dari admin Redis. Kosongkan bila tanpa password.
# @optional
REDIS_PASSWORD=
# Sertifikat SSL dari tim jaringan (isi banyak baris, diisi dari file)
SSL_CERT=
```

Contoh `.env` hasil (anotasi ikut disalin, lihat keputusan D1):

```env
SULTRAKEY_APP=lakupandai
SULTRAKEY_PUBLIC_KEY=age1...
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=enc:<base64 ciphertext age biner>
# Password dari admin Redis. Kosongkan bila tanpa password.
# @optional
REDIS_PASSWORD=
# Sertifikat SSL dari tim jaringan (isi banyak baris, diisi dari file)
SSL_CERT=enc:<base64 ciphertext age biner>
```

## Keputusan hasil diskusi

| No  | Keputusan                                                                                                                                                                                                                                         |
| --- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| D1  | `.env` berdiri sendiri: anotasi `# @plain` / `# @optional` ikut disalin ke `.env`. `check` dan `run` membaca anotasi dari `.env`, tidak butuh template. Template hanya dipakai `init` dan `fill`.                                                  |
| D2  | Migrasi `.env` polos lama: `init` pada `.env` tanpa header mengenkripsi semua value yang terisi secara langsung, kecuali key yang `@plain` di template. Key yang tidak ada di template ikut dienkripsi. Tampilkan nama key yang dienkripsi. |
| D3  | Key `@optional` yang kosong tetap dikirim ke aplikasi sebagai string kosong (`REDIS_PASSWORD=""`).                                                                                                                                                 |
| D4  | `init` yang dijalankan sebagai root menolak jalan tanpa `--owner` (exit 64) dan menampilkan perintah yang benar.                                                                                                                                  |
| D5  | Runbook developer memuat peringatan: aplikasi jangan memuat `.env` sendiri dengan mode menimpa (mis. `dotenv` `override: true`), karena akan membaca teks `enc:...` sebagai value.                                                                 |
| D6  | Value banyak baris didukung tanpa anotasi khusus. Isi dari file: saat `fill` ketik `@/lokasi/file` (diawali `@`), saat `set` pakai `--file <lokasi>`. Lihat "Value dari file".                                                                    |
| D7  | Windows didukung untuk laptop developer saja, bukan server produksi. Semua perintah jalan di Windows. Lihat "Windows (laptop developer)".                                                                                                          |
| D8  | Ada `install`, `update`, `uninstall`, dan skrip pasang satu baris, meniru proyek `C:\DEVONLY\RUST\lopi`. Rilis sementara di GitHub publik `hmrnsp/sultrakey` (GitLab kantor belum tersedia; nanti pindah). Update hanya manual. Lihat "Pasang, update, hapus". |
| D9  | pm2: standar mode **fork, 1 proses** (`interpreter: 'none'`), karena mode cluster pm2 hanya menerima file JavaScript. Aplikasi yang sekarang cluster: cek CPU dulu. Lihat "pm2 dan mode cluster".                                          |
| D10 | Di Linux/macOS, kunci selalu di `/etc/sultrakey/<app>.key`. Membuat kunci butuh sudo, termasuk di laptop developer: `sudo sultrakey init <app> --owner $USER`. Windows tetap `%APPDATA%\sultrakey\`. |
| D11 | Input tersembunyi diketik dua kali; bila tidak sama, prompt diulang. Input `@lokasi-file` dan input `@plain` (terlihat) cukup sekali. |
| D12 | `fill` selalu menanyakan semua key yang kosong, termasuk `@optional`. Enter pada key `@optional` = tetap kosong. |
| D13 | Value polos di key rahasia (bukan `@plain`, tidak diawali `enc:`) membuat `check`/`run` gagal (exit 78, `Solusi: sultrakey fill`). `fill` mengenkripsinya otomatis dan menyebut nama key-nya. |

Asumsi implementasi (disetujui bersama rencana):
- Anotasi tidak dikenal (mis. `# @optinal`) ditolak dengan exit 78 dan nomor baris. Beberapa anotasi boleh satu baris: `# @plain @optional`.
- Perintah aplikasi tidak ditemukan saat `run` → exit 78, supaya pm2/systemd tidak restart terus-menerus.
- `.env` punya header tetapi file kunci tidak ada → exit 78; kunci baru TIDAK dibuat otomatis.
- `fill` tanpa template → exit 78. `init` tanpa template dan tanpa `.env` → exit 78.
- Key di `.env` yang tidak ada di template disimpan di akhir file, dengan peringatan.
- `run` hanya menghapus `SULTRAKEY_*` dari env anak; `CREDENTIALS_DIRECTORY` dibiarkan.
- `--owner` menerima nama atau angka (`1001:1001`); binary musl statis hanya membaca `/etc/passwd` (bukan LDAP/SSSD).

## Aturan template

- Baris komentar biasa di atas key = teks bantuan saat `fill`.
- Anotasi di baris komentar `# @...` tepat di atas key: `@plain` (tidak dienkripsi), `@optional` (boleh kosong).
- Value di template = default yang ditawarkan saat `fill` (Enter = pakai default).
- Urutan key dan komentar template dipertahankan di `.env`. Terima CRLF (checkout Windows).

## Value dari file (D6)

- Saat `fill`, jawaban yang diawali `@` dibaca sebagai lokasi file. Contoh: `@/tmp/cert.pem`.
  Untuk value yang memang diawali `@`, ketik `@@` (jarang terjadi; sebutkan di teks bantuan prompt).
- `set <KEY> --file <lokasi>` mengisi satu key dari file.
- Isi file dibaca apa adanya, hanya satu baris baru di akhir file yang dibuang. File harus teks UTF-8 tanpa byte NUL
  (environment tidak bisa membawa data biner); bila bukan, tolak dengan pesan jelas.
- Setelah berhasil, cetak pengingat: `Hapus file /tmp/cert.pem sekarang.`
- Value `enc:` banyak baris aman (base64 satu baris). Value `@plain` banyak baris ditulis dalam kutip ganda dengan `\n`.

## Windows (laptop developer) (D7)

- Semua perintah (`init`, `fill`, `set`, `list`, `check`, `run`) jalan di Windows tanpa hak Administrator.
- Lokasi kunci bawaan: `%APPDATA%\sultrakey\<app>.key` (per user, tidak perlu Administrator).
- `--owner` tidak dipakai di Windows; bila diisi, cetak peringatan dan abaikan.
- Pemeriksaan izin file kunci dilewati. `check` menampilkan satu baris info "izin file kunci tidak diperiksa di Windows".
- Mode 0600 dan pemilik `.env` tidak diatur di Windows. Tulis atomik tetap berlaku; bila rename gagal karena file
  sedang dibuka program lain, coba ulang sebentar lalu beri pesan jelas.
- `run`: spawn, tunggu, teruskan exit code. Saat developer menekan Ctrl+C, sultrakey tidak mati duluan;
  ia menunggu aplikasi berhenti lalu meneruskan exit code-nya. Bila sultrakey dimatikan paksa, aplikasi ikut mati
  (Job Object), supaya port tidak tertahan proses yatim.
- `run -- npm run dev` harus jalan: cari perintah memakai `PATHEXT` (`.exe`, `.cmd`, `.bat`), karena `npm`, `npx`,
  `pnpm`, `yarn`, dan `mvnw` di Windows berupa file `.cmd`.
- Lokasi file di `fill`/`set` menerima gaya Windows: `@C:\temp\cert.pem`.
- `.env` selalu ditulis dengan akhir baris LF; CRLF tetap diterima saat membaca.

## Pasang, update, hapus (D8)

Acuan kode: `C:\DEVONLY\RUST\lopi` (`src/install/`, `src/update/`, `src/commands/install.rs`, `update.rs`, `uninstall.rs`).

Lokasi pasang:

| OS                         | Lokasi                                   | Hak akses                                        |
| -------------------------- | ---------------------------------------- | ------------------------------------------------ |
| Linux, dijalankan root     | `/usr/bin/sultrakey` (0755, root:root)   | `sudo`. Juga buat `/etc/sultrakey` (0755) bila belum ada. |
| Linux non-root (developer) | `~/.local/bin/sultrakey`                 | Tanpa sudo. Bila tidak ada di PATH, cetak baris yang perlu ditambahkan. |
| macOS (developer)          | `~/.local/bin/sultrakey`                 | Sama seperti Linux non-root.                     |
| Windows (developer)        | `%LOCALAPPDATA%\Programs\sultrakey\sultrakey.exe` | Tanpa Administrator. Folder ditambahkan ke PATH user (registry). |

`install`:
- Menyalin dirinya sendiri; tidak butuh internet. Menjalankan lagi = menimpa versi lama.
- Windows: exe yang sedang dipakai di-rename ke `.old`, dihapus pada install/update/uninstall berikutnya (seperti lopi).

`update`:
- Hanya mengganti binary yang dipasang oleh `install` atau skrip pasang. Selain itu: beri tahu cara update yang benar.
- Di `/usr/bin` butuh root. Bila bukan root: `✗ Butuh hak root.` / `Solusi: sudo sultrakey update`.
- Langkah: baca manifest rilis terbaru → bandingkan versi → unduh binary untuk OS/CPU ini → cocokkan SHA256 →
  jalankan file baru dengan `--version` → baru tukar dengan rename atomik. Gagal di langkah mana pun = versi lama utuh.
- Tidak pernah berjalan otomatis. Tanya konfirmasi kecuali `-y`.
- Aplikasi yang sedang jalan tidak terganggu; versi baru dipakai saat aplikasi di-restart berikutnya. Tulis ini di runbook.

`uninstall`:
- Menghapus binary dan entri PATH (Windows). Tidak menyentuh `/etc/sultrakey/`, `%APPDATA%\sultrakey\`, atau `.env`.
- Cetak pengingat lokasi file kunci yang masih tersimpan.

Sumber rilis:
- Sementara: GitHub Releases publik `https://github.com/hmrnsp/sultrakey/releases` (sama seperti lopi).
  Versi terbaru: `<base>/latest/download/<file>`; versi tertentu: `<base>/download/v<versi>/<file>`.
  Repo harus tetap publik: di repo private, unduhan rilis butuh token login, sehingga `update` dan skrip pasang
  gagal di server. Bila kelak kode ingin private: pisahkan repo rilis publik (`hmrnsp/sultrakey-releases`).
  Server produksi bisa mengakses github.com.
- Nanti pindah ke GitLab kantor. Supaya mudah: alamat rilis ditanam saat build (satu konstanta / variabel build),
  dan kode unduh dipisah di satu modul. Pindah = ganti alamat + file CI, tanpa ubah logika update.
  (Catatan GitLab nanti: proyek harus berstatus **Public**, karena "Internal" tetap butuh login.)
- Alamat bisa ditimpa `SULTRAKEY_UPDATE_URL` (untuk test dan masa transisi pindah ke GitLab).
- File rilis: binary mentah per target (tanpa arsip), `SHA256SUMS`, `manifest.json` (versi + nama file + SHA256),
  `install.sh`, `install.ps1`.
- HTTPS saja. Pakai daftar sertifikat milik OS (sertifikat SSL kantor ikut dipercaya); `SSL_CERT_FILE` bisa menimpa.
  Proxy dari `HTTPS_PROXY`/`NO_PROXY`.
- Server yang tidak bisa mengakses github.com tetap bisa dipasang/di-update tanpa internet:
  salin file binary ke server, lalu `sudo ./sultrakey install`.

Skrip pasang satu baris:
- `install.sh` (Linux/macOS): `curl -fsSL <url>/install.sh | sh` (developer) atau `... | sudo sh` (server).
- `install.ps1` (Windows): `powershell -ExecutionPolicy Bypass -c "irm <url>/install.ps1 | iex"`.
- Skrip hanya: deteksi OS/CPU → unduh binary → cocokkan SHA256 → jalankan `sultrakey install`.
  Semua logika pasang ada di binary, bukan di skrip.

## pm2 dan mode cluster (D9)

- Mode cluster pm2 tidak bisa menjalankan `sultrakey` (cluster hanya untuk file JavaScript).
- Standar untuk semua aplikasi yang memakai sultrakey (Cara 1):

  ```js
  { name: 'api', script: '/usr/bin/sultrakey', args: 'run -- node ./dist/main.js',
    interpreter: 'none', exec_mode: 'fork', instances: 1, stop_exit_codes: [78], /* setting lain tetap */ }
  ```

- Aplikasi yang sekarang memakai cluster: cek `pm2 list` / `pm2 monit` saat jam sibuk.
  - CPU tiap salinan rendah (di bawah ~50%) → pindah ke Cara 1.
  - CPU satu salinan sering mendekati 100% → Cara 3: systemd menjalankan
    `/usr/bin/sultrakey run -- /usr/bin/pm2-runtime start ecosystem.config.js` (mode cluster tetap).
    Wajib diuji dulu di satu server sebelum jadi standar.
- Cara 2 (beberapa proses fork dengan port berbeda + nginx) hanya disebut di runbook sebagai alternatif.
- DILARANG: `sultrakey run -- pm2 start ...`. pm2 menyimpan env yang sudah dibuka, dan `pm2 save`
  menulisnya polos ke `~/.pm2/dump.pm2`.
- Runbook juga menjelaskan efek samping cluster: memori berlipat, cron jalan berkali-kali,
  data di memori tidak dibagi antarsalinan, WebSocket butuh pengaturan tambahan.

## Kriptografi (jangan buat kriptografi sendiri)

- Pakai crate `age` (X25519 recipient). Satu value = satu ciphertext age biner, di-base64 standar, prefix `enc:`.
- File kunci = format identity age standar (`AGE-SECRET-KEY-1...`), sehingga value bisa dibuka darurat
  dengan CLI resmi: `echo <b64> | base64 -d | age -d -i /etc/sultrakey/<app>.key`. Wajib ada test kompatibilitas ini.
- Enkripsi hanya butuh public key dari header `.env`; private key hanya dibaca saat `check`/`run`.
- Simpan plaintext dan kunci dengan `secrecy`/`zeroize`.

## Perintah

| Perintah                            | Perilaku                                                                                                                                                                                          |
| ----------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init <app> [--owner user[:group]]` | Buat kunci bila belum ada. Bila sudah ada: pakai ulang, cocokkan dengan public key di `.env`, jangan pernah menimpa. Buat `.env` dari template bila belum ada. Nama app `[a-z0-9-]+`. Lihat D2, D4. |
| `fill`                              | Sinkron template → `.env`: tambah key baru, tanya yang kosong (input tersembunyi kecuali `@plain`), value lama tidak diubah. Key di `.env` yang tidak ada di template: peringatan, tidak dihapus. |
| `set <KEY>`                         | Ganti satu value. Value dari prompt tersembunyi, stdin (`--stdin`, seluruh isi stdin), atau file (`--file`). JANGAN dari argumen (terlihat di `ps`/history).                                      |
| `list`                              | Nama key + status (terisi/kosong/terenkripsi/plain). Tidak pernah menampilkan value.                                                                                                              |
| `check`                             | Semua key wajib terisi, semua `enc:` bisa dibuka, public key cocok, izin file kunci tidak terbuka untuk group/other.                                                                              |
| `run [--env FILE] -- <cmd> [args]`  | Jalankan `check`, buka value, set env, lalu exec.                                                                                                                                                 |
| `install`                           | Salin binary yang sedang jalan ke lokasi pasang. Dijalankan lagi = timpa (cara update tanpa internet). Lihat "Pasang, update, hapus".                                                             |
| `update [--check] [-y]`             | Unduh rilis terbaru dari sumber rilis, periksa, lalu ganti binary. `--check` hanya memberi tahu (exit 1 bila ada versi baru).                                                                   |
| `uninstall`                         | Hapus binary (dan entri PATH di Windows). File kunci TIDAK pernah dihapus.                                                                                                                        |

Opsi global: `--env` (default `./.env`), `--template` (default `./.env.template`), `--key-file`.
Urutan mencari kunci: `--key-file` → `SULTRAKEY_KEY_FILE` → `$CREDENTIALS_DIRECTORY/sultrakey.key` (systemd LoadCredential) → `/etc/sultrakey/<app>.key`
(di Windows langkah terakhir diganti `%APPDATA%\sultrakey\<app>.key`).
Tidak ada perintah untuk mencetak value polos (di v1).

- `fill` tanpa TTY (untuk test/otomasi): baca baris `KEY=value` dari stdin; `KEY=@/lokasi/file` juga berlaku.
  Baris yang bukan format itu ditolak.
- Catatan: `LoadCredential` hanya ada di systemd ≥ 247 (Rocky 9, Ubuntu 22+). CentOS 7 dan Rocky 8 memakai `/etc/sultrakey/`.

## Detail `run`

- Unix: `CommandExt::exec` supaya PID = aplikasi asli (pm2/systemd memantau aplikasi, sinyal & exit code langsung).
  Windows (hanya untuk developer): spawn, tunggu, teruskan exit code (detail di bagian Windows).
- Value dari `.env` mengalahkan env yang sudah ada. Bila bentrok, cetak peringatan berisi nama key saja.
- Key `@optional` yang kosong dikirim sebagai string kosong (D3).
- Jangan teruskan `SULTRAKEY_*` dan path kunci ke proses anak.

## Exit code & pesan

- 0 sukses, 64 salah pemakaian (termasuk argumen tidak dikenal; ganti exit 2 bawaan clap), 78 konfigurasi salah
  (key kosong, gagal dekripsi, izin, kunci tidak cocok), 1 lainnya.
  Konfigurasi salah harus 78 karena pm2 `stop_exit_codes: [78]` dan systemd `RestartPreventExitStatus=78`.
- Semua teks untuk infra dalam bahasa Indonesia sederhana: baris masalah + baris `Solusi: <perintah persis>`.
  Contoh: `✗ REDIS_HOST belum diisi.` / `Solusi: sultrakey fill`
- Bila gagal membaca kunci karena izin, `Solusi:` menyebut `sudo -u <user pemilik kunci> sultrakey ...`.
- Jangan pernah mencetak value atau potongannya ke stdout, stderr, atau log.

## Keandalan file

- Tulis `.env` atomik (file temp di folder yang sama, fsync, rename), mode 0600, pemilik dipertahankan.
  Butuh izin tulis di folder aplikasi, bukan hanya di file `.env`.
- Kunci lock agar dua `fill` tidak berjalan bersamaan. Lock memakai file terpisah `.env.lock`
  (lock pada `.env` sendiri hilang saat rename).
- Output `.env` harus tetap valid untuk parser dotenv umum (quote bila ada spasi, `#`, kutip, atau baris baru).

## Target build

- `x86_64-unknown-linux-musl` (statis; wajib jalan di CentOS 7) dan `aarch64-unknown-linux-musl`.
  Windows & macOS untuk developer. Cross-compile aarch64 di CI memakai `cargo-zigbuild` atau `cross`.
- Windows: `x86_64-pc-windows-msvc` dengan CRT statis (`+crt-static`), supaya `sultrakey.exe` jalan tanpa
  instal Visual C++ Redistributable. Satu file `.exe`, cukup ditaruh di folder yang ada di PATH.
- Pasang di server ke `/usr/bin/sultrakey` lewat `sudo ./sultrakey install` (sudo di RHEL tidak mencari di `/usr/local/bin`).
- Crate yang disarankan: clap (derive), age, base64, rpassword atau dialoguer, secrecy/zeroize, thiserror, fd-lock,
  tempfile; untuk install/update: ureq (rustls + sertifikat OS), sha2, serde_json, self-replace, dirs,
  windows-sys (registry PATH); test: assert_cmd, predicates. Pin versi di Cargo.lock.

## Deliverable

1. Proyek Cargo dengan unit test (parser roundtrip, enkripsi/dekripsi, anotasi, CRLF, value banyak baris) dan integration test
   (`init → fill via stdin → check → run -- env`, migrasi `.env` polos, value dari file), plus test kompatibilitas dengan CLI `age` bila tersedia.
   Semua test harus lulus di Linux dan Windows (`cargo test` di laptop developer).
   Test install/update memakai folder sementara dan server HTTP lokal (seperti lopi), tidak menyentuh sistem asli.
2. GitHub Actions (`.github/workflows/ci.yml` dan `release.yml`, acuan: lopi): test di Linux & Windows,
   build musl (x86_64, aarch64), Windows `.exe`, macOS (x86_64, aarch64; runner Mac gratis di GitHub),
   lalu saat tag `v*`: upload binary + `SHA256SUMS` + `manifest.json` + `install.sh` + `install.ps1` ke GitHub Release.
   Tidak memakai cargo-dist: skrip bawaannya memasang ke `~/.cargo/bin`, sedangkan server butuh `/usr/bin`.
   `.gitlab-ci.yml` dibuat nanti saat GitLab kantor tersedia.
3. `README.md` (developer) dan `docs/runbook-infra.md` (infra, ringkas) berisi contoh:
   - pm2: `script: '/usr/bin/sultrakey', args: 'run -- node dist/main.js', interpreter: 'none', stop_exit_codes: [78]`
     (hanya mode fork, bukan cluster; pastikan versi pm2 mendukung `stop_exit_codes`; lihat D9 untuk cluster,
     cek CPU, dan larangan `sultrakey run -- pm2 start`)
   - systemd: `User=<app>`, `ExecStart=/usr/bin/sultrakey run --env /opt/<app>/.env -- /usr/bin/node dist/main.js`,
     `RestartPreventExitStatus=78`
   - Docker: COPY binary, `ENTRYPOINT ["sultrakey","run","--"]`, mount `.env` dan file kunci `:ro`
     (pemilik kunci = uid user di image)
   - Spring Boot (`java -jar`), Go (`os.Getenv`), Rust (`std::env`)
   - Developer Windows: pasang `sultrakey.exe`, `sultrakey init <app>`, `sultrakey fill`,
     `sultrakey run -- npm run dev` / `sultrakey run -- mvnw.cmd spring-boot:run` / `sultrakey run -- go run .`
   - Backup: simpan satu salinan file kunci offline. Kunci hilang = semua value harus diisi ulang.
   - Peringatan D5 (aplikasi jangan memuat `.env` sendiri dengan mode menimpa).
   - Peringatan framework yang membaca env saat build (SvelteKit `$env/static/*`, Vite `import.meta.env`,
     Next.js `NEXT_PUBLIC_*`): secret harus dibaca saat jalan (SvelteKit `$env/dynamic/private`), bukan saat build,
     dan tidak pernah dikirim ke browser. Contoh: Express, SvelteKit (adapter-node), Spring Boot, Go, Rust.
   - Keamanan: siapa pun yang bisa menulis `.env` bisa mengganti value. Jaga izin tulis folder aplikasi.
   - Pasang/update/hapus: skrip satu baris, `sudo ./sultrakey install` tanpa internet, `sudo sultrakey update`,
     catatan bahwa update baru berlaku setelah aplikasi di-restart, dan `uninstall` tidak menghapus kunci.

## Cara kerja

- Mulai dengan rencana singkat + struktur modul, tunggu persetujuan saya, baru menulis kode.
- Urutan MVP: parser/format `.env` → crypto → init/fill/check → run → list/set → install/uninstall → update → CI & docs.
- Tanyakan bila ada keputusan desain yang belum tercakup di sini; jangan menebak.
- Di luar cakupan: integrasi ke aplikasi tertentu, validasi zod (dikerjakan di masing-masing aplikasi), secret manager,
  rotasi kunci (v1).
