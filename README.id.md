# sultrakey

[English](README.md) · **Bahasa Indonesia**

File `.env` dengan nilai terenkripsi, dilengkapi shortcut yang menjalankan aplikasi berbahasa apa
pun (Node.js, Spring Boot, Go, Rust, dan sebagainya) dengan nilai yang sudah dibuka di variabel
lingkungannya.

- Satu berkas biner statis tanpa dependensi. Berjalan di Rocky Linux 8/9, Ubuntu, dan CentOS 7.
  Windows dan macOS tersedia untuk laptop pengembang.
- Enkripsi memakai [age](https://age-encryption.org) X25519, tanpa kriptografi buatan sendiri. Dalam
  keadaan darurat, nilai tetap dapat dibuka dengan CLI `age` resmi.
- Aplikasi tidak perlu diubah: aplikasi membaca variabel lingkungan seperti biasa.
- Di terminal, tanda `✓`, `!`, `✗`, dan `Fix:` ditampilkan berwarna. Saat keluaran dialirkan ke
  skrip atau log, teksnya sama persis tanpa warna. Warna dapat dimatikan dengan `NO_COLOR=1`.

Panduan lengkap untuk tim infrastruktur (pemasangan di server, setup, pm2, systemd, Docker) ada di
[docs/runbook-infra.md](docs/runbook-infra.md).

## Pemasangan

Windows (PowerShell, tanpa hak Administrator):

```powershell
powershell -ExecutionPolicy Bypass -c "irm https://github.com/hmrnsp/sultrakey/releases/latest/download/install.ps1 | iex"
```

Linux / macOS:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sh        # laptop
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh   # server → /usr/bin
```

Pemasangan manual: unduh berkas biner dari [halaman rilis](https://github.com/hmrnsp/sultrakey/releases/latest),
lalu jalankan `./sultrakey-<target> install` (gunakan `sudo` di server). Dari kode sumber:
`cargo install --path .`.

| Sistem                  | Lokasi pemasangan                                                                |
| ----------------------- | -------------------------------------------------------------------------------- |
| Linux/macOS dengan sudo | `/usr/bin/sultrakey` (beserta folder kunci `/etc/sultrakey`)                     |
| Linux/macOS tanpa sudo  | `~/.local/bin/sultrakey`                                                         |
| Windows                 | `%LOCALAPPDATA%\Programs\sultrakey\sultrakey.exe` (ditambahkan ke PATH pengguna) |

## Alur singkat

1. Repositori aplikasi berisi `.env.example` (semua variabel, dengan nilai kosong atau nilai bawaan
   yang bukan rahasia). Berkas `.env` asli tidak boleh masuk git. Tambahkan `.env` dan `.env.lock` ke
   `.gitignore`. Jangan memakai pola `.env*`, karena `.env.example` ikut terabaikan.
2. `sultrakey init <app>` membuat pasangan kunci dan `.env` dari templat.
3. `sultrakey setup` menanyakan variabel yang masih kosong (variabel yang sudah terisi ikut tampil dan
   dapat diganti), lalu menyimpannya secara terenkripsi.
4. `sultrakey check` memastikan semua variabel terisi dan dapat dibuka.
5. `sultrakey run -- <perintah>` membuka nilai, mengisinya ke variabel lingkungan, lalu menjalankan
   aplikasi.

Di laptop Windows:

```powershell
cd C:\proyek\example-app
sultrakey init example-app
sultrakey setup
sultrakey run -- npm run dev
```

Di Linux/macOS, pembuatan kunci memerlukan sudo, termasuk di laptop. Kunci selalu disimpan di
`/etc/sultrakey/`:

```sh
sudo sultrakey init example-app --owner $USER
sultrakey setup
sultrakey run -- npm run dev
```

## Layar `setup`

![Layar setup sultrakey](docs/Screenshot.png)

Di terminal, `setup` menampilkan semua variabel dalam satu layar. Daftar variabel berada di kiri,
sedangkan variabel yang dipilih berada di kanan. Variabel yang sudah terisi tampil redup dengan tanda
`✓`. Layar dimulai dari variabel kosong pertama.

- Di bawah nama variabel terdapat tiga label:
  - `SECRET` / `VISIBLE`: cara pengetikan (tampil sebagai bintang dan diketik dua kali, atau tampil
    apa adanya dan diketik sekali).
  - `REQUIRED` / `OPTIONAL`: wajib diisi atau boleh kosong.
  - `ENCRYPTED` / `PLAIN`: cara penyimpanan di `.env`.
- Keterangan (tampil redup) diambil dari komentar di atas variabel pada templat, diikuti nilai bawaan
  (`Default: ...`) bila ada. Pada variabel yang sudah terisi tertulis `Example: ...`: itu nilai dari templat,
  bukan nilai yang tersimpan di `.env`. Variabel tanpa komentar menampilkan `No description for this key yet.`
- Saat Anda mengetik `@lokasi-berkas`, layar langsung memberi tahu apakah berkas tersebut ada. Isi
  berkas tidak pernah ditampilkan.
- Enter menyimpan isian dan berpindah ke variabel kosong berikutnya. Setelah yang terakhir, muncul
  daftar `Review before saving` untuk diperiksa. Nilai rahasia pada daftar itu selalu tampil `********`.
- Variabel yang sudah terisi dimulai dengan baris kosong; nilainya tidak pernah ditampilkan. Enter pada baris
  kosong itu mempertahankan nilai lama; mengetik nilai baru menggantinya. Di daftar review tampil
  `(unchanged)`.
- `.env` baru ditulis setelah `[ Save ]` dipilih. Esc atau Ctrl+C membatalkan tanpa mengubah apa pun.
- Variabel yang dilewati (berpindah dengan ↑↓ tanpa menekan Enter) tidak berubah.
- Bila semua variabel sudah terisi, `setup` hanya memberi tahu hal itu dan tidak membuka layar.
- F1 menampilkan arti label, anotasi (`@plain`, `@optional`, `@masking`), dan semua tombol. Isinya
  dapat digulir dengan ↑↓; tombol lain menutupnya.

## Templat

```env
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=
# Kata sandi dari admin Redis. Kosongkan bila tidak memakai kata sandi.
# @optional
REDIS_PASSWORD=
# Sertifikat SSL dari tim jaringan (diisi dari berkas)
SSL_CERT=
```

- Komentar tepat di atas variabel menjadi keterangan saat `setup`.
- Nilai pada templat menjadi nilai bawaan saat `setup`:
  - Variabel biasa: nilai bawaan sudah tertulis di baris isian. Enter = pakai, Backspace = ubah. Jika
    seluruh isian dihapus lalu Enter ditekan, variabel `@optional` menjadi kosong, sedangkan variabel
    wajib ditolak sampai terisi. Tanpa terminal, variabel biasa yang kosong dan tidak dikirim langsung
    diisi nilai bawaan.
  - Variabel rahasia: nilai pada templat **diabaikan** dan wajib diketik, termasuk saat tanpa
    terminal. Dengan demikian, kata sandi contoh seperti `DB_PASSWORD=secret` tidak pernah tersimpan.
  - **Kosongkan nilai contoh pada variabel biasa** (misalnya `SMTP_HOST=smtp.example.com`). Jika tidak,
    nilai contoh itu tersimpan sebagai nilai sungguhan dan `check` tetap lolos.
- Nilai yang lebih dari satu baris diisi dari berkas: saat `setup`, ketik `@/lokasi/berkas`, atau
  gunakan `sultrakey set KEY --file <lokasi>`. Ketik `@@` untuk nilai yang memang diawali `@`.
- Urutan dan komentar templat ikut disalin ke `.env` beserta anotasinya. Karena itu, `check` dan `run`
  tidak memerlukan templat.

### Anotasi

Anotasi ditulis sebagai komentar di atas variabel, pada blok komentar yang menempel dengan variabel
tersebut (tanpa baris kosong di antaranya). Variabel tanpa anotasi dianggap **rahasia dan wajib**:
dienkripsi dan harus terisi sebelum `run`.

| Anotasi       | Fungsi                                                                                                                                                                                                                                                 | Jika tanpa anotasi                                                                                                                                                 |
| ------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `# @plain`    | Nilai disimpan **apa adanya**, tidak dienkripsi (label `PLAIN`). Untuk nilai yang bukan rahasia, misalnya port atau level log.                                                                                                                         | Nilai dienkripsi (`enc:...`, label `ENCRYPTED`). Nilai polos yang diketik manual pada variabel ini membuat `check` dan `run` gagal sampai `setup` mengenkripsinya. |
| `# @optional` | Variabel **boleh kosong** (label `OPTIONAL`). `check` dan `run` tetap berjalan, dan aplikasi menerima variabel itu dengan isi kosong. Saat `setup`, hapus seluruh isian lalu tekan Enter untuk membiarkannya kosong.                                   | Variabel wajib diisi (label `REQUIRED`). `check` dan `run` gagal (kode keluar 78) selama variabel masih kosong.                                                    |
| `# @masking`  | Ketikan tampil sebagai `*` dan diketik **dua kali** saat `setup` dan `set` (label `SECRET`). Nilai bawaan dari templat diabaikan. Untuk rahasia yang namanya tidak terlihat seperti rahasia, misalnya `DATABASE_URL=postgres://user:password@host/db`. | Cara pengetikan mengikuti nama variabel (lihat di bawah).                                                                                                          |

Aturan penulisan:

- Beberapa anotasi boleh ditulis dalam satu baris (`# @plain @optional`) atau pada baris terpisah.
- Baris komentar yang diawali `@` hanya boleh berisi anotasi. Anotasi yang tidak dikenal (misalnya
  salah ketik `# @optinal`) ditolak beserta nomor barisnya, sehingga rahasia tidak tersimpan polos
  tanpa disadari. Nama lama `@masked` juga ditolak; gunakan `@masking`.
- Komentar yang tidak diawali `@` adalah keterangan biasa, meskipun berisi `@` di tengahnya
  (`# email @ kantor`).

Cara pengetikan tanpa `@masking`: variabel tetap tampil `*` jika salah satu bagian namanya (dipisahkan
`_`, huruf besar atau kecil sama saja) adalah `PASSWORD`, `PASSWD`, `PASS`, `PWD`, `SECRET`, `TOKEN`,
`AUTH`, atau `SALT`. Contoh: `REDIS_PASSWORD` tampil `*`, sedangkan `API_KEY` dan `PASSPORT_URL` tampil
apa adanya (tambahkan `@masking` jika `API_KEY` harus tampil `*`). Cara pengetikan tidak memengaruhi
enkripsi: yang menentukan enkripsi hanyalah `@plain`.

Contoh kombinasi:

| Templat                                  | Disimpan    | Saat diketik                           | Boleh kosong |
| ---------------------------------------- | ----------- | -------------------------------------- | ------------ |
| `REDIS_HOST=`                            | terenkripsi | tampil apa adanya, sekali              | tidak        |
| `REDIS_PASSWORD=`                        | terenkripsi | `*`, dua kali                          | tidak        |
| `# @optional`<br>`REDIS_PASSWORD=`       | terenkripsi | `*`, dua kali                          | ya           |
| `# @plain`<br>`PORT=8899`                | apa adanya  | tampil apa adanya, `8899` sudah terisi | tidak        |
| `# @plain @optional`<br>`LOG_LEVEL=info` | apa adanya  | tampil apa adanya, `info` sudah terisi | ya           |
| `# @masking`<br>`DATABASE_URL=`          | terenkripsi | `*`, dua kali                          | tidak        |

### Penanda lain

| Penanda                        | Lokasi                                                | Fungsi                                                                                                                                                                                                          |
| ------------------------------ | ----------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `SULTRAKEY_APP=<app>`          | Baris awal `.env` (dibuat oleh `init`)                | Nama aplikasi; menentukan berkas kunci `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`). Tidak boleh ada di templat.                                                                       |
| `SULTRAKEY_PUBLIC_KEY=age1...` | Baris awal `.env` (dibuat oleh `init`)                | Kunci publik untuk mengenkripsi. Harus berpasangan dengan berkas kunci; jika tidak, `check` gagal.                                                                                                              |
| `SULTRAKEY_*`                  | Nama variabel                                         | Awalan yang dicadangkan untuk sultrakey. Selain dua baris di atas, variabel berawalan ini ditolak di `.env` dan templat. Variabel lingkungan berawalan ini juga tidak pernah diteruskan ke aplikasi oleh `run`. |
| `enc:...`                      | Nilai di `.env`                                       | Nilai terenkripsi. Jangan disunting manual; gantilah dengan `sultrakey set KEY`.                                                                                                                                |
| `@/lokasi/berkas`              | Jawaban saat `setup` (atau `KEY=@berkas` lewat stdin) | Nilai diambil dari isi berkas, untuk nilai multibaris seperti sertifikat. Hapus berkas itu setelahnya. Tanpa `@`, lokasi disimpan apa adanya sebagai teks (cocok untuk `PUBLIC_KEY_PATH=keys/public_key.pem`).  |
| `@@...`                        | Jawaban saat `setup`                                  | Nilai yang memang diawali `@`. `@@abc` disimpan sebagai `@abc`.                                                                                                                                                 |

Hasil `.env`:

```env
SULTRAKEY_APP=example-app
SULTRAKEY_PUBLIC_KEY=age1...
# Port aplikasi
# @plain
PORT=8899
# Host Redis, tanpa http:// dan port. Contoh: 10.10.1.20
REDIS_HOST=enc:YWdlLWVuY3J5cHRpb24...
...
```

## Perintah

| Perintah                                               | Fungsi                                                                                                                                                                                                                                                                                                                                |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `init <app> [--owner user[:group]]`                    | Membuat kunci jika belum ada (tidak pernah menimpa). Membuat `.env` dari templat, atau mengambil alih `.env` polos lama dengan mengenkripsi nilainya.                                                                                                                                                                                 |
| `setup`                                                | Menyamakan `.env` dengan templat, mengenkripsi nilai polos pada variabel rahasia, lalu menampilkan semua variabel dalam satu layar: yang kosong untuk diisi, yang terisi untuk dipertahankan atau diganti (variabel rahasia tampil `*`). Tanpa terminal: membaca baris `KEY=value` dari stdin.                                    |
| `set <KEY> [--stdin \| --file F]`                      | Mengganti satu nilai. Nilai tidak pernah diambil dari argumen.                                                                                                                                                                                                                                                                        |
| `list`                                                 | Menampilkan nama variabel dan statusnya; nilai tidak pernah ditampilkan. Di terminal: tabel berwarna dengan kolom `STORED` (`ENCRYPTED`/`PLAIN`) dan `TYPED` (`SECRET`/`VISIBLE`), disertai ringkasan variabel yang masih memerlukan `setup`. Saat dialirkan ke skrip (`\| grep`, `> file`): kolom `KEY  STATUS` polos seperti biasa. |
| `check`                                                | Memastikan semua variabel wajib terisi, semua `enc:` dapat dibuka, kunci cocok, dan izin berkas kunci aman. Di terminal: tabel hasil per variabel dan berkas kunci. Masalah selalu dicetak sebagai baris `✗ ...` dan `Fix: ...` di stderr.                                                                                            |
| `run [--env F] -- <cmd> [args]`                        | Menjalankan `check`, lalu menjalankan aplikasi dengan nilai di variabel lingkungannya.                                                                                                                                                                                                                                                |
| `install` / `update [--check] [-y]` / `uninstall [-y]` | Memasang, memperbarui, atau menghapus berkas biner.                                                                                                                                                                                                                                                                                   |

Opsi global: `--env` (bawaan `./.env`), `--template` (bawaan `./.env.example`), dan `--key-file`.

Urutan pencarian berkas kunci:

1. `--key-file`
2. `SULTRAKEY_KEY_FILE`
3. `$CREDENTIALS_DIRECTORY/sultrakey.key` (systemd `LoadCredential=`, memerlukan systemd ≥ 247: Rocky 9, Ubuntu 22.04+)
4. `/etc/sultrakey/<app>.key` (Windows: `%APPDATA%\sultrakey\<app>.key`)

Kode keluar: `0` berhasil, `64` salah penggunaan, `78` konfigurasi salah, `1` kesalahan lainnya. Kode
`78` membuat pm2 (`stop_exit_codes: [78]`) dan systemd (`RestartPreventExitStatus=78`) berhenti
mencoba memulai ulang aplikasi.

## Cara kerja `run`

- Linux/macOS: memakai `exec`. Aplikasi mengambil alih PID sultrakey sehingga pm2/systemd memantau
  aplikasi yang sebenarnya, dan sinyal serta kode keluar langsung sampai ke aplikasi.
- Windows: aplikasi dijalankan sebagai proses anak, dan kode keluarnya diteruskan.
  - Ctrl+C sampai ke aplikasi.
  - Jika terminal ditutup paksa, aplikasi ikut berhenti (Job Object) sehingga port tidak tertahan.
  - Perintah `.cmd`/`.bat` (`npm`, `npx`, `pnpm`, `yarn`, `mvnw`) ditemukan melalui `PATHEXT`.
- Nilai dari `.env` mengalahkan variabel lingkungan yang sudah ada. Jika bentrok, sultrakey mencetak
  peringatan yang hanya memuat nama variabel.
- Variabel `@optional` yang kosong dikirim sebagai string kosong.
- Variabel `SULTRAKEY_*` tidak diteruskan ke aplikasi.

## Membaca nilai di aplikasi

| Aplikasi                 | Cara membaca                                                       |
| ------------------------ | ------------------------------------------------------------------ |
| Express / Node.js        | `process.env.REDIS_HOST`                                           |
| SvelteKit (adapter-node) | `import { env } from '$env/dynamic/private'` → `env.REDIS_HOST`    |
| Spring Boot              | `spring.data.redis.host=${REDIS_HOST}` di `application.properties` |
| Go                       | `os.Getenv("REDIS_HOST")`                                          |
| Rust                     | `std::env::var("REDIS_HOST")`                                      |

Contoh perintah: `sultrakey run -- node dist/main.js`, `sultrakey run -- node build` (SvelteKit),
`sultrakey run -- java -jar app.jar`, `sultrakey run -- mvnw.cmd spring-boot:run`, dan
`sultrakey run -- go run .`.

**Peringatan:**

- **Jangan memuat `.env` sendiri dengan mode menimpa**, misalnya `dotenv` dengan `override: true`.
  Aplikasi akan membaca teks `enc:...` sebagai nilainya. `dotenv` biasa (tanpa override) tetap aman
  karena tidak menimpa nilai dari sultrakey.
- **Kerangka kerja yang membaca variabel lingkungan saat build** (SvelteKit `$env/static/*`, Vite
  `import.meta.env`, Next.js `NEXT_PUBLIC_*`) menanamkan nilai ke hasil build. Saat aplikasi berjalan,
  sultrakey sudah terlambat mengisinya, dan nilai itu dapat terkirim ke peramban. Nilai rahasia hanya
  boleh dibaca di sisi server saat aplikasi berjalan (SvelteKit: `$env/dynamic/private`).
- **pm2 mode cluster** tidak dapat menjalankan sultrakey. Gunakan mode fork (lihat runbook).

## Darurat: membuka nilai tanpa sultrakey

```sh
echo '<teks base64 setelah enc:>' | base64 -d | age -d -i /etc/sultrakey/<app>.key
```

Berkas kunci memakai format identitas age standar (`AGE-SECRET-KEY-1...`). Kunci yang hilang berarti
semua nilai harus diisi ulang, sehingga simpanlah cadangan kunci di tempat yang luring (offline).

## Pengembangan

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test                                   # uji kompatibilitas age berjalan jika CLI `age` terpasang
```

- Uji integrasi memakai folder sementara (`SULTRAKEY_KEY_DIR`, `SULTRAKEY_INSTALL_DIR`) dan server HTTP
  lokal (`SULTRAKEY_UPDATE_URL`). Pengujian tidak pernah menyentuh `/etc/sultrakey` maupun PATH asli.
- CI (`.github/workflows/ci.yml`) menjalankan fmt, clippy, dan pengujian di Linux x86/ARM, Windows, dan
  macOS; memeriksa MSRV 1.89 dan shellcheck; serta menguji berkas biner musl di kontainer `centos:7`.
- Rilis: naikkan `version` di `Cargo.toml`, commit, lalu jalankan `git tag v0.1.1 && git push origin v0.1.1`.
  Workflow `release.yml` membangun berkas biner untuk 5 target, `SHA256SUMS`, `manifest.json`,
  `install.sh`, dan `install.ps1`, lalu menerbitkannya di GitHub Releases.
- Pindah ke GitLab kantor: build dengan `SULTRAKEY_RELEASE_BASE=<alamat rilis>`, terbitkan berkas yang
  sama, dan sesuaikan bentuk URL di `src/update/http.rs` jika berbeda. Proyek GitLab harus berstatus
  **Public**. Status "Internal" tetap memerlukan login sehingga `update` dan skrip pemasangan gagal.
- Repositori harus tetap publik. Pada repositori privat, pengunduhan rilis memerlukan token.

Struktur kode:

```
src/envfile/   pengurai, penulis, dan sinkronisasi templat ↔ .env
src/crypto.rs  age X25519 + base64
src/keyfile.rs lokasi, pembacaan, dan pembuatan berkas kunci
src/commands/  satu berkas per perintah
src/input/     prompt, layar setup (ratatui), dan jawaban @berkas
src/launch/    exec (Unix) / proses anak + Job Object (Windows)
src/install/   lokasi pemasangan dan penggantian berkas biner
src/update/    manifes, checksum, unduhan HTTPS
```

Di luar cakupan v1: rotasi kunci, perintah untuk mencetak nilai polos, dan integrasi dengan secret
manager.
