# Runbook sultrakey untuk tim infra

`sultrakey` menyimpan password aplikasi di file `.env` dalam bentuk terenkripsi. Saat aplikasi
dinyalakan, `sultrakey` membuka password itu dan memberikannya ke aplikasi lewat environment variable.
Kode aplikasi tidak perlu diubah.

Aturan dasar:

- Setiap pesan error berisi dua baris: masalahnya (`✗ ...`) dan perintah perbaikannya (`Fix: ...`).
  Semua pesan program berbahasa Inggris; runbook ini mengutipnya apa adanya.
- `sultrakey` tidak pernah menampilkan isi password.
- File kunci (`/etc/sultrakey/<app>.key`) wajib di-backup. **Kunci hilang = semua password harus diisi
  ulang.**

Contoh di seluruh runbook ini memakai:

| Hal | Contoh | Ganti dengan |
| --- | --- | --- |
| Nama aplikasi | `example-app` | Nama aplikasi Anda: huruf kecil, angka, dan tanda minus saja |
| User Linux yang menjalankan aplikasi | `example-app` | User aplikasi Anda |
| Folder aplikasi | `/opt/example-app` | Folder aplikasi Anda |
| Perintah start aplikasi | `/usr/bin/node dist/main.js` | Perintah start aplikasi Anda, misalnya `/usr/bin/java -jar app.jar` |

## Daftar isi

1. [Pasang sultrakey di server](#1-pasang-sultrakey-di-server)
2. [Siapkan aplikasi baru](#2-siapkan-aplikasi-baru)
3. [Menjalankan aplikasi dengan pm2](#3-menjalankan-aplikasi-dengan-pm2)
4. [Menjalankan aplikasi dengan systemd](#4-menjalankan-aplikasi-dengan-systemd)
5. [Menjalankan aplikasi dengan Docker](#5-menjalankan-aplikasi-dengan-docker)
6. [Pekerjaan rutin](#6-pekerjaan-rutin)
7. [Pesan error yang sering muncul](#7-pesan-error-yang-sering-muncul)
8. [Pindahkan `.env` lama yang masih polos](#8-pindahkan-env-lama-yang-masih-polos)
9. [Update dan hapus sultrakey](#9-update-dan-hapus-sultrakey)
10. [Backup dan pemulihan kunci](#10-backup-dan-pemulihan-kunci)

## 1. Pasang sultrakey di server

Satu binary yang sama jalan di Rocky Linux 8/9, Ubuntu, dan CentOS 7. Binary ini statis (musl), jadi
tidak butuh library tambahan.

### 1a. Server yang bisa mengakses github.com

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh
```

Skrip ini mengunduh binary yang cocok dengan CPU server, mencocokkan SHA256-nya, lalu memasangnya.
Server yang memakai proxy: jalankan `export HTTPS_PROXY=http://proxy:port` dulu, lalu jalankan
perintah di atas dengan `sudo -E sh` sebagai ganti `sudo sh`.

Memasang versi tertentu:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo SULTRAKEY_VERSION=0.5.0 sh
```

### 1b. Server tanpa internet

1. Di komputer lain, unduh dari halaman rilis
   (<https://github.com/hmrnsp/sultrakey/releases/latest>):
   - `sultrakey-x86_64-unknown-linux-musl` (server ARM: `sultrakey-aarch64-unknown-linux-musl`)
   - `SHA256SUMS`
2. Salin kedua file ke server, misalnya ke `/tmp` dengan `scp`.
3. Di server, cocokkan checksum. Harus muncul `OK`:

   ```sh
   cd /tmp
   grep 'sultrakey-x86_64-unknown-linux-musl$' SHA256SUMS | sha256sum -c -
   ```

4. Pasang:

   ```sh
   chmod +x sultrakey-x86_64-unknown-linux-musl
   sudo ./sultrakey-x86_64-unknown-linux-musl install
   rm sultrakey-x86_64-unknown-linux-musl SHA256SUMS
   ```

### 1c. Pastikan terpasang

```sh
sultrakey --version
ls -ld /etc/sultrakey
```

Hasil yang benar:

- `sultrakey --version` mencetak versinya, misalnya `sultrakey 0.5.0`.
- Program ada di `/usr/bin/sultrakey`, jadi bisa dipakai pm2, systemd, dan semua user.
- Folder kunci `/etc/sultrakey` ada, milik `root`, izin `drwxr-xr-x` (755).

## 2. Siapkan aplikasi baru

Lakukan langkah ini satu kali per aplikasi per server.

### 2a. Buat user aplikasi

Aplikasi sebaiknya berjalan sebagai user Linux sendiri, bukan root. Hanya user itu yang boleh membaca
file kunci aplikasinya.

```sh
# Rocky / CentOS
sudo useradd --system --create-home --shell /sbin/nologin example-app
# Ubuntu
sudo useradd --system --create-home --shell /usr/sbin/nologin example-app
```

- `--create-home` dibutuhkan bila aplikasi dijalankan dengan pm2 (pm2 menyimpan datanya di
  `~/.pm2`).
- Untuk Docker, beri uid tetap yang sama dengan user di dalam image, misalnya `--uid 1001`. Lihat
  [bagian 5](#5-menjalankan-aplikasi-dengan-docker).
- User sudah ada? Lewati langkah ini.

### 2b. Siapkan folder aplikasi

```sh
sudo mkdir -p /opt/example-app
# deploy kode aplikasi ke /opt/example-app (git clone, salin hasil build, dsb.)
ls -la /opt/example-app/.env.example
```

- Folder harus berisi `.env.example` (dibawa developer lewat git). Tanpa file ini, `init` gagal dengan
  `✗ Template .env.example not found.`
- User aplikasi harus bisa **membaca** folder dan kode aplikasi. User aplikasi **tidak perlu** bisa
  menulis folder itu.
- `.env` dan `.env.lock` tidak boleh masuk git. Pastikan keduanya ada di `.gitignore` repo aplikasi.

### 2c. Periksa `.env.example` sebelum dipakai

Buka filenya dan periksa empat hal:

1. **Value contoh palsu di key biasa.** Value yang terisi di `.env.example` dipakai sebagai nilai
   bawaan key biasa. Bila isinya contoh palsu (misalnya `SMTP_HOST=smtp.example.com`), minta developer
   mengosongkannya dulu. Value di key rahasia selalu diabaikan, jadi aman.
2. **Anotasi.** Tanpa anotasi, key dianggap rahasia dan wajib. Anotasi yang dikenal hanya `# @plain`
   (disimpan polos), `# @optional` (boleh kosong), dan `# @masking` (diketik sebagai bintang).
3. **`@masked` lama.** Sejak versi 0.5, `@masked` ditolak. Ganti menjadi `@masking`.
4. **Keterangan.** Komentar di atas key tampil saat `setup`. Key tanpa komentar menampilkan
   `No description for this key yet.` Minta developer menambahkan keterangan bila perlu.

### 2d. Buat kunci dan `.env`

```sh
cd /opt/example-app
sudo sultrakey init example-app --owner example-app
```

Hasil yang benar:

```
✓ New key created: /etc/sultrakey/example-app.key (mode 400, owner example-app)
! Keep a copy of this key file somewhere safe (offline). A lost key means every value must be entered again.
✓ .env created from .env.example (5 keys, all still empty).
Next: sudo sultrakey setup, then sudo -u example-app sultrakey check
```

- `--owner` wajib saat memakai sudo. Isinya user yang menjalankan aplikasi. Format `user` atau
  `user:group`.
- File kunci dibuat dengan izin `400`, milik user aplikasi. `.env` dibuat dengan izin `600`, milik user
  aplikasi.
- `init` tidak pernah menimpa kunci yang sudah ada. Menjalankan ulang `init` aman.

**Backup file kunci sekarang juga.** Lihat [bagian 10](#10-backup-dan-pemulihan-kunci).

### 2e. Isi semua value

```sh
cd /opt/example-app
sudo sultrakey setup
```

Jalankan dari terminal sungguhan: SSH, Windows Terminal, atau PowerShell. Jendela bawaan Git Bash
(mintty) tidak bisa menampilkan layar ini.

`setup` menampilkan semua key yang masih kosong dalam satu layar. Daftar key ada di kiri, key yang
dipilih ada di kanan.

- Di bawah nama key ada tiga label:

  | Label | Arti |
  | --- | --- |
  | `SECRET` | Diketik sebagai bintang, dua kali. Untuk key yang namanya mengandung PASSWORD, PASS, PWD, SECRET, TOKEN, AUTH, atau SALT, atau yang diberi `@masking`. |
  | `VISIBLE` | Diketik terlihat, sekali. |
  | `REQUIRED` | Wajib diisi sebelum aplikasi bisa jalan. |
  | `OPTIONAL` | Boleh dikosongkan: hapus isinya, lalu Enter. |
  | `ENCRYPTED` | Disimpan terenkripsi di `.env`. |
  | `PLAIN` | Disimpan apa adanya (key `@plain`). |

- Keterangan dari `.env.example` tampil redup di bawah label, diikuti `Default: ...` bila ada nilai
  bawaan.
- Key biasa yang punya nilai bawaan: nilainya sudah tertulis di baris input. Enter = pakai, Backspace =
  ubah.
- Key rahasia tidak pernah punya nilai bawaan: selalu diketik.
- Isi yang panjangnya lebih dari satu baris (sertifikat, file kunci) diisi dari file: salin filenya ke
  server, lalu ketik `@/tmp/cert.pem`. Layar langsung memberi tahu apakah file itu ada
  (`✓ File found ...` atau `✗ File not found.`). Isi file tidak pernah ditampilkan. Setelah selesai,
  **hapus file itu**.
- Enter menyimpan isian dan pindah ke key berikutnya. ↑↓ pindah key tanpa menyimpan ketikan.
- Setelah key terakhir, muncul `Review before saving` berisi semua jawaban (key rahasia tampil
  `********`). Pilih `[ Save ]` untuk menyimpan, atau `[ Back to edit ]` untuk mengubah jawaban.
- Key yang dilewati tetap kosong, dan peringatan kuning `! Not filled: ...` menyebut namanya.
- Esc atau Ctrl+C membatalkan. Tidak ada yang tersimpan sampai `[ Save ]` dipilih.
- F1 menampilkan arti label, anotasi, dan semua tombol.

Kenapa `setup` dijalankan dengan `sudo`: saat menyimpan, `setup` menulis file sementara dan
`.env.lock` di folder aplikasi. User aplikasi biasanya tidak boleh menulis folder itu.

**Tanpa layar interaktif** (otomasi): kirim baris `KEY=value` lewat stdin. Jangan ketik password
langsung di perintah, karena akan tersimpan di riwayat shell. Pakai file sementara:

```sh
sudo sultrakey setup < /tmp/values.env
shred -u /tmp/values.env
```

Baris `KEY=@/lokasi/file` juga berlaku di sini.

### 2f. Periksa hasilnya

```sh
cd /opt/example-app
sudo -u example-app sultrakey check
sudo -u example-app sultrakey list
```

- `check` dijalankan **sebagai user aplikasi**, supaya sekaligus memastikan user itu bisa membaca
  kuncinya. Di terminal, `check` menampilkan hasil per key dan hasil file kunci:

  ```
   KEY           RESULT
   PORT          ✓ plain
   REDIS_HOST    ✓ decrypted
   DB_PASSWORD   ✓ decrypted

   KEY FILE      ✓ readable, matches .env
                 /etc/sultrakey/example-app.key

  ✓ All good (3 keys, key file: /etc/sultrakey/example-app.key).
  ```

  Key yang bermasalah tampil kuning dengan `✗` (misalnya `✗ empty`), lalu di bawah tabel muncul
  pesan `✗ ...` dan `Fix: ...` untuk setiap masalah. Saat dijalankan dari skrip, pm2, atau systemd,
  tabel tidak dicetak; yang keluar hanya pesan `✗`/`Fix:` atau baris `✓ All good`.
- `list` menampilkan nama key dan statusnya (`encrypted`, `plain`, `empty — required`,
  `empty (optional)`, `plain — must be encrypted`). Isi password tidak pernah ditampilkan.
  Di terminal, hasilnya berupa tabel berwarna:

  ```
   KEY           STATUS               STORED      TYPED
   PORT          ✓ plain              PLAIN       VISIBLE
   REDIS_HOST    ✓ encrypted          ENCRYPTED   VISIBLE
   DB_PASSWORD   ✗ empty — required   ENCRYPTED   SECRET

   3 keys · 1 needs setup: sultrakey setup
  ```

  Hijau = terisi, kuning (`✗`) = perlu `setup`, redup (`○`) = kosong tapi boleh. Saat hasilnya
  dialirkan ke skrip (`| grep`, `> file`), yang keluar adalah kolom `KEY  STATUS` polos, jadi skrip
  lama tetap jalan.
- Masih ada key kosong? `check` mencetak `✗ <KEY> is empty.` Jalankan `sudo sultrakey setup` lagi.

Aplikasi siap dijalankan. Pilih salah satu cara: [pm2](#3-menjalankan-aplikasi-dengan-pm2),
[systemd](#4-menjalankan-aplikasi-dengan-systemd), atau [Docker](#5-menjalankan-aplikasi-dengan-docker).

Polanya sama untuk semua cara: tambahkan `sultrakey run --` di depan perintah start aplikasi.
`sultrakey run` memeriksa `.env` dulu (sama seperti `check`), membuka semua value, lalu **mengganti
dirinya** dengan proses aplikasi (`exec`). Akibatnya:

- PID aplikasi sama dengan PID yang dipantau pm2/systemd/Docker.
- Sinyal stop (SIGTERM) dan exit code langsung sampai ke aplikasi.
- Bila ada masalah konfigurasi, aplikasi tidak start dan exit code-nya `78`.

## 3. Menjalankan aplikasi dengan pm2

### 3a. Syarat

- Node.js dan pm2 sudah terpasang (`sudo npm install -g pm2`).
- User aplikasi punya folder home (lihat [2a](#2a-buat-user-aplikasi)).
- Catat lokasi lengkap `node` dan `pm2`:

  ```sh
  which node pm2
  ```

  Bila node dipasang lewat nvm, lokasinya ada di folder home user tertentu. Pakai lokasi lengkap itu
  di file konfigurasi.

### 3b. Buat file konfigurasi pm2

Simpan sebagai `/opt/example-app/ecosystem.config.js`:

```js
module.exports = {
  apps: [{
    name: 'example-app',
    cwd: '/opt/example-app',
    script: '/usr/bin/sultrakey',
    args: ['run', '--', '/usr/bin/node', 'dist/main.js'],
    interpreter: 'none',
    exec_mode: 'fork',
    instances: 1,
    autorestart: true,
    stop_exit_codes: [78],
    max_restarts: 10,
    restart_delay: 3000,
    time: true,
    // setting lain (log, max_memory_restart, dll.) tetap seperti biasa
  }]
};
```

Arti setiap baris penting:

| Baris | Kenapa |
| --- | --- |
| `script: '/usr/bin/sultrakey'` | pm2 menjalankan sultrakey, bukan node langsung. |
| `args: ['run', '--', ...]` | Perintah start aplikasi ditulis setelah `--`. |
| `interpreter: 'none'` | sultrakey adalah program biasa, bukan file JavaScript. Tanpa baris ini pm2 mencoba menjalankannya dengan node dan gagal. |
| `exec_mode: 'fork'`, `instances: 1` | Mode cluster tidak bisa dipakai (lihat [3f](#3f-pm2-mode-cluster)). |
| `cwd` | Folder tempat `.env` berada. |
| `stop_exit_codes: [78]` | Bila password belum diisi atau kunci salah, pm2 berhenti dan tidak mencoba start berulang-ulang. |

**Jangan** menaruh password di bagian `env:` file ini. Semua password diambil dari `.env` oleh sultrakey.

### 3c. Jalankan sebagai user aplikasi

pm2 harus berjalan sebagai user aplikasi, karena hanya user itu yang boleh membaca kuncinya.

```sh
sudo -u example-app -H pm2 start /opt/example-app/ecosystem.config.js
sudo -u example-app -H pm2 list
sudo -u example-app -H pm2 logs example-app --lines 50
```

- `-H` membuat pm2 memakai folder home user aplikasi (`/home/example-app/.pm2`).
- Muncul `pm2: command not found`? Pakai lokasi lengkap dari `which pm2`, misalnya
  `sudo -u example-app -H /usr/local/bin/pm2 start ...`.
- Status `online` = aplikasi jalan. Status `stopped` atau `errored` = lihat log. Pesan `✗` dan `Fix:`
  dari sultrakey ada di log error pm2.

### 3d. Nyala otomatis saat server reboot

```sh
sudo pm2 startup systemd -u example-app --hp /home/example-app
sudo -u example-app -H pm2 save
```

- Perintah pertama membuat service systemd `pm2-example-app` yang menyalakan pm2 milik user aplikasi.
- Perintah kedua menyimpan daftar aplikasi yang sedang jalan, supaya dinyalakan lagi setelah reboot.
- `pm2 save` aman dengan konfigurasi ini: password dibuka di dalam proses aplikasi setelah start, jadi
  tidak ikut tersimpan di `~/.pm2/dump.pm2`.

Cek setelah reboot: `sudo -u example-app -H pm2 list`.

### 3e. Uji bahwa `stop_exit_codes` bekerja

Opsi `stop_exit_codes` hanya dikenali pm2 versi baru. Di versi lama, opsi ini diabaikan dan pm2 terus
mencoba restart. Uji sekali di setiap server dengan aplikasi uji yang sengaja salah konfigurasi
(aplikasi asli tidak disentuh):

```sh
cat > /tmp/sultrakey-test.config.js <<'END'
module.exports = { apps: [{
  name: 'sultrakey-test',
  script: '/usr/bin/sultrakey',
  args: ['run', '--env', '/tmp/tidak-ada/.env', '--', '/bin/true'],
  interpreter: 'none',
  exec_mode: 'fork',
  stop_exit_codes: [78],
}] };
END
sudo -u example-app -H pm2 start /tmp/sultrakey-test.config.js
sleep 5
sudo -u example-app -H pm2 list      # sultrakey-test harus stopped/errored, kolom restart (↺) tetap 0
sudo -u example-app -H pm2 delete sultrakey-test
rm /tmp/sultrakey-test.config.js
```

Bila jumlah restart terus naik, perbarui pm2 (`sudo npm install -g pm2@latest`, lalu
`sudo -u example-app -H pm2 update`).

### 3f. pm2 mode cluster

Mode cluster pm2 tidak bisa menjalankan `sultrakey`, karena hanya menerima file JavaScript.

1. Cek dulu apakah aplikasinya benar-benar butuh cluster. Jalankan `pm2 monit` saat jam sibuk.
   - CPU tiap salinan rendah (di bawah ~50%): pindah ke mode fork 1 proses seperti di atas.
   - CPU satu salinan sering mendekati 100%: pakai systemd + `pm2-runtime` (uji dulu di satu server).
     Di unit systemd ([bagian 4](#4-menjalankan-aplikasi-dengan-systemd)), tulis:

     ```ini
     ExecStart=/usr/bin/sultrakey run -- /usr/bin/pm2-runtime start ecosystem.config.js
     ```

     Di cara ini, `ecosystem.config.js` menjalankan file JavaScript aplikasi langsung (bukan
     sultrakey), karena password sudah dibuka oleh sultrakey di depan.
2. Alternatif lain: beberapa proses fork dengan port berbeda, lalu nginx membagi bebannya.

Efek samping mode cluster yang sering tidak disadari:

- Memori berlipat sebanyak jumlah salinan.
- Jadwal otomatis (cron) jalan berkali-kali.
- Data di memori (misalnya sesi login) tidak dibagi antarsalinan.
- WebSocket butuh pengaturan tambahan.

### 3g. Jangan lakukan ini

- **Jangan** menjalankan `sultrakey run -- pm2 start ...`. Dengan cara itu pm2 menyimpan password yang
  sudah dibuka, dan `pm2 save` menulisnya **polos** ke `~/.pm2/dump.pm2`.
- **Jangan** menjalankan pm2 sebagai root untuk aplikasi ini. Root bisa membaca semua kunci di server.

## 4. Menjalankan aplikasi dengan systemd

### 4a. Buat file unit

Simpan sebagai `/etc/systemd/system/example-app.service`:

```ini
[Unit]
Description=example-app
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=example-app
Group=example-app
WorkingDirectory=/opt/example-app
ExecStart=/usr/bin/sultrakey run --env /opt/example-app/.env -- /usr/bin/node dist/main.js
Restart=on-failure
RestartSec=5
RestartPreventExitStatus=78
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
```

Arti setiap baris penting:

| Baris | Kenapa |
| --- | --- |
| `User=` / `Group=` | Aplikasi berjalan sebagai user aplikasi, yang memiliki file kunci. |
| `WorkingDirectory=` | Folder aplikasi. Path relatif di perintah start (`dist/main.js`) dihitung dari sini. |
| `ExecStart=` | `sultrakey run`, lalu `--`, lalu perintah start aplikasi. Tulis lokasi lengkap program (`/usr/bin/node`, `/usr/bin/java`). systemd tidak memakai PATH dari shell login, jadi program dari nvm atau sdkman tidak ditemukan tanpa lokasi lengkap. |
| `Restart=on-failure`, `RestartSec=5` | Aplikasi yang mati karena error dinyalakan lagi setelah 5 detik. |
| `RestartPreventExitStatus=78` | Bila password belum diisi atau kunci salah, systemd **tidak** mencoba restart. |

Contoh `ExecStart` untuk aplikasi lain:

```ini
ExecStart=/usr/bin/sultrakey run --env /opt/example-app/.env -- /usr/bin/java -jar /opt/example-app/app.jar
ExecStart=/usr/bin/sultrakey run --env /opt/example-app/.env -- /opt/example-app/example-app
```

**Jangan** menaruh password di `Environment=` atau `EnvironmentFile=`. Semua password diambil dari
`.env` oleh sultrakey.

### 4b. Nyalakan

```sh
sudo systemctl daemon-reload
sudo systemctl enable --now example-app
sudo systemctl status example-app
```

- `enable --now` menyalakan aplikasi sekarang dan setiap kali server reboot.
- Status `active (running)` = aplikasi jalan.

### 4c. Lihat log

```sh
journalctl -u example-app -n 50          # 50 baris terakhir
journalctl -u example-app -f             # ikuti log secara langsung
```

Bila aplikasi tidak start, pesan `✗` dan `Fix:` dari sultrakey ada di sini. Status unit menjadi
`failed` dengan `status=78`, dan systemd tidak mencoba restart. Perbaiki sesuai pesan `Fix:`, lalu
`sudo systemctl restart example-app`.

### 4d. Pilihan: kunci lewat `LoadCredential` (systemd ≥ 247)

Dengan cara ini, file kunci cukup bisa dibaca root. systemd yang membacanya, lalu memberikannya ke
aplikasi lewat folder khusus yang hanya bisa dibaca service itu. Cara ini hanya untuk systemd ≥ 247
(Rocky 9, Ubuntu 22.04+), **tidak** untuk Rocky 8 dan CentOS 7. Cek versi dengan `systemctl --version`.

1. Tambahkan baris ini di bagian `[Service]`:

   ```ini
   LoadCredential=sultrakey.key:/etc/sultrakey/example-app.key
   ```

2. Jadikan root pemilik file kunci:

   ```sh
   sudo chown root:root /etc/sultrakey/example-app.key
   sudo chmod 400 /etc/sultrakey/example-app.key
   ```

3. Muat ulang dan uji:

   ```sh
   sudo systemctl daemon-reload
   sudo systemctl restart example-app
   journalctl -u example-app -n 20
   ```

Akibatnya, `check` sekarang harus dijalankan dengan `sudo sultrakey check` (bukan
`sudo -u example-app ...`), karena user aplikasi tidak bisa lagi membaca kuncinya langsung.

## 5. Menjalankan aplikasi dengan Docker

Di Docker, kunci dan `.env` tetap disimpan di server (host), lalu di-mount ke container saat jalan.
Keduanya **tidak pernah** dimasukkan ke image.

### 5a. Tentukan uid user aplikasi

Container harus berjalan dengan uid yang sama dengan pemilik file kunci dan `.env` di host. Cara paling
mudah: buat user aplikasi di host dengan uid tetap, lalu pakai uid yang sama di container.

```sh
sudo useradd --system --create-home --uid 1001 --shell /sbin/nologin example-app
id example-app       # harus uid=1001
```

Lalu jalankan [bagian 2](#2-siapkan-aplikasi-baru) seperti biasa (`init --owner example-app`,
`setup`). Hasilnya, file kunci dan `.env` milik uid 1001.

User aplikasi sudah ada dengan uid lain? Pakai uid itu di semua contoh di bawah, sebagai ganti `1001`.

### 5b. Masukkan sultrakey ke image

1. Unduh `sultrakey-x86_64-unknown-linux-musl` dan cocokkan checksum-nya
   (lihat [1b](#1b-server-tanpa-internet)). Taruh di folder build image.
2. Contoh `Dockerfile`:

   ```dockerfile
   FROM node:22-alpine
   WORKDIR /app

   COPY sultrakey-x86_64-unknown-linux-musl /usr/local/bin/sultrakey
   RUN chmod 755 /usr/local/bin/sultrakey

   COPY . .
   RUN npm ci --omit=dev

   USER 1001
   ENTRYPOINT ["/usr/local/bin/sultrakey", "run", "--"]
   CMD ["node", "dist/main.js"]
   ```

3. Tambahkan baris ini ke `.dockerignore`, supaya `.env` tidak pernah ikut masuk image:

   ```
   .env
   .env.lock
   ```

Catatan:

- `ENTRYPOINT` harus ditulis dalam bentuk array seperti di atas. Bentuk teks biasa
  (`ENTRYPOINT sultrakey run -- ...`) menjalankan shell di depan, sehingga sinyal stop tidak sampai ke
  aplikasi.
- Binary sultrakey statis, jadi jalan di image apa pun: Alpine, Debian, Ubuntu, distroless.
- `CMD` adalah perintah start aplikasi. Bisa diganti saat `docker run` tanpa mengubah `ENTRYPOINT`.

### 5c. Jalankan dengan docker compose

`docker-compose.yml`:

```yaml
services:
  example-app:
    image: example-app:latest
    init: true
    user: "1001:1001"
    working_dir: /app
    volumes:
      - /opt/example-app/.env:/app/.env:ro
      - /etc/sultrakey/example-app.key:/etc/sultrakey/example-app.key:ro
    restart: "on-failure:5"
```

Arti setiap baris penting:

| Baris | Kenapa |
| --- | --- |
| `init: true` | Docker menaruh proses init kecil sebagai PID 1, sehingga sinyal stop dan proses anak ditangani dengan benar. |
| `user: "1001:1001"` | Sama dengan pemilik file kunci dan `.env` di host. |
| `working_dir: /app` | `sultrakey run` mencari `.env` di folder ini. |
| volume `.env` | `.env` dari host, read-only (`:ro`). `run` hanya membaca, tidak menulis. |
| volume kunci | File kunci dari host ke lokasi bawaan `/etc/sultrakey/<app>.key` di container. Izinnya tetap `400`. |
| `restart: on-failure:5` | Docker tidak bisa mengecualikan exit code 78. Batas 5 kali mencegah restart tanpa henti saat konfigurasi salah. |

Nyalakan dan lihat log:

```sh
docker compose up -d
docker compose ps
docker compose logs -f example-app
```

### 5d. Jalankan dengan `docker run`

```sh
docker run -d --name example-app \
  --init \
  --user 1001:1001 \
  -w /app \
  -v /opt/example-app/.env:/app/.env:ro \
  -v /etc/sultrakey/example-app.key:/etc/sultrakey/example-app.key:ro \
  --restart on-failure:5 \
  example-app:latest

docker logs -f example-app
```

### 5e. Periksa dari dalam container

```sh
docker compose run --rm --entrypoint /usr/local/bin/sultrakey example-app check
```

Hasil yang benar: `✓ All good (... keys, key file: /etc/sultrakey/example-app.key).`

### 5f. Masalah yang sering muncul di Docker

| Gejala | Penyebab | Perbaikan |
| --- | --- | --- |
| `✗ No permission to read key file ...` | uid container berbeda dengan pemilik file kunci di host. | Samakan uid (lihat [5a](#5a-tentukan-uid-user-aplikasi)), atau `sudo chown 1001 /etc/sultrakey/example-app.key`. |
| `✗ No permission to read ...` padahal uid sudah sama, di Rocky/RHEL | SELinux memblokir akses container ke file host. | Tambahkan `,z` di akhir opsi volume, misalnya `/opt/example-app/.env:/app/.env:ro,z`. |
| `✗ Key file ... is too open ...` | Izin file kunci di host lebih longgar dari `400`. | `sudo chmod 400 /etc/sultrakey/example-app.key` |
| `✗ File .env not found.` | Volume `.env` tidak terpasang, atau `working_dir` salah. | Periksa baris `volumes` dan `working_dir`. |
| Container restart terus lalu berhenti | Konfigurasi salah (exit 78). | `docker compose logs example-app`, perbaiki sesuai pesan `Fix:`. |

Mengubah password untuk aplikasi Docker: jalankan `sudo sultrakey set ...` di **host** (folder
`/opt/example-app`), lalu `docker compose restart example-app`.

CentOS 6 hanya didukung lewat Docker.

## 6. Pekerjaan rutin

### 6a. Ganti satu password

```sh
cd /opt/example-app
sudo sultrakey set DB_PASSWORD                            # key rahasia: diketik dua kali
sudo sultrakey set SSL_CERT --file /tmp/cert.pem          # isi dari file
shred -u /tmp/cert.pem                                    # hapus file sumbernya
sudo -u example-app sultrakey check
```

Lalu restart aplikasinya supaya password baru dipakai:

| Cara jalan | Perintah restart |
| --- | --- |
| pm2 | `sudo -u example-app -H pm2 restart example-app` |
| systemd | `sudo systemctl restart example-app` |
| Docker | `docker compose restart example-app` |

### 6b. Deploy versi baru aplikasi

1. Deploy kode baru seperti biasa (`git pull`, salin hasil build, dsb.). `.env` tidak tersentuh, karena
   tidak ada di git.
2. Bila developer menambah key baru di `.env.example`, isi key baru itu:

   ```sh
   cd /opt/example-app
   sudo sultrakey setup
   ```

   `setup` hanya menanyakan key yang masih kosong. Value lama tidak diubah.
   `check` juga mengingatkan bila template punya key baru:
   `! The template has keys that are not in .env yet: ...`
3. Periksa, lalu restart aplikasi:

   ```sh
   sudo -u example-app sultrakey check
   ```

### 6c. Lihat daftar key

```sh
cd /opt/example-app
sudo -u example-app sultrakey list
```

## 7. Pesan error yang sering muncul

| Pesan | Artinya | Yang dilakukan |
| --- | --- | --- |
| `✗ X is empty.` | Key wajib masih kosong. | `sudo sultrakey setup` |
| `✗ X is stored plain ...` | Ada password yang diketik manual tanpa enkripsi. | `sudo sultrakey setup` (otomatis dienkripsi) |
| `✗ No permission to read key file ...` | Dijalankan oleh user yang salah. | Jalankan sebagai user aplikasi: `sudo -u <user> sultrakey check` |
| `✗ Key file ... is too open ...` | File kunci bisa dibaca user lain. | `sudo chmod 400 /etc/sultrakey/<app>.key` |
| `✗ Key file ... does not belong to .env ...` | Kunci dan `.env` berasal dari aplikasi atau server berbeda. | Pakai kunci yang benar, atau pulihkan dari backup. |
| `✗ Key file ... not found.` | Kunci hilang atau salah lokasi. | Pulihkan dari backup. Bila tidak ada backup: hapus `.env`, jalankan `init` dan `setup` ulang. |
| `✗ X cannot be decrypted ...` | Isi satu key rusak. | `sudo sultrakey set X` |
| `✗ Command 'node' not found.` | Perintah aplikasi salah, atau tidak ada di PATH. | Tulis lokasi lengkapnya, contoh `/usr/bin/node`. |
| `✗ File .env not found.` | Dijalankan dari folder yang salah. | `cd` ke folder aplikasi, atau pakai `--env /opt/example-app/.env`. |
| `✗ .env line N: annotation '@masked' was renamed to @masking ...` | File dibuat untuk sultrakey 0.4 atau lebih lama. | Ganti `# @masked` menjadi `# @masking` di `.env` dan `.env.example`. |
| `✗ Cannot ask questions because this is not a terminal ...` | `set`, `update`, atau `uninstall` dijalankan tanpa terminal (misalnya dari skrip). | Jalankan dari SSH biasa. Untuk skrip: `set KEY --stdin` atau `set KEY --file <lokasi>`, `update -y`, `uninstall -y`. |
| `✗ Another sultrakey process is changing .env ...` | Ada `setup` atau `set` lain yang sedang berjalan. | Tunggu sebentar, lalu ulangi. |

Arti exit code: `0` sukses, `64` perintah salah ketik, `78` konfigurasi salah (aplikasi tidak
di-restart oleh pm2/systemd), `1` masalah lain.

## 8. Pindahkan `.env` lama yang masih polos

Untuk aplikasi yang sudah punya `.env` berisi password polos:

```sh
cd /opt/example-app
sudo cp -p .env /root/example-app.env.polos.bak        # cadangan sementara
sudo sultrakey init example-app --owner example-app
sudo -u example-app sultrakey check
sudo shred -u /root/example-app.env.polos.bak          # hapus cadangan polos setelah check lolos
```

- Semua password yang sudah terisi langsung dienkripsi, kecuali key yang ditandai `@plain` di template.
- `sultrakey` mencetak nama key yang dienkripsi (`✓ Encrypted: ...`). Isi passwordnya tidak dicetak.
- Key yang ada di `.env` tapi tidak ada di template tetap disimpan di akhir file, dengan peringatan.
- Setelah itu, ubah cara start aplikasi ke `sultrakey run -- ...` (bagian 3, 4, atau 5).

## 9. Update dan hapus sultrakey

### 9a. Update

```sh
sultrakey update --check     # hanya memeriksa; exit 1 bila ada versi baru
sudo sultrakey update        # memperbarui /usr/bin/sultrakey
```

- Update tidak pernah berjalan otomatis.
- Aplikasi yang sedang berjalan tidak terganggu. Versi baru dipakai saat aplikasi di-restart berikutnya.
- Server tanpa internet: ulangi [1b](#1b-server-tanpa-internet) dengan binary baru.
- Image Docker: ganti binary di folder build, lalu build ulang image.

**Update dari 0.4 atau lebih lama ke 0.5:** sebelum update, ganti `# @masked` menjadi `# @masking` di
setiap `.env` dan `.env.example` di server. Kalau tidak, `check` dan `run` menolak file itu dan aplikasi
tidak start saat restart berikutnya. Cari filenya dengan:

```sh
sudo grep -rln '@masked' /opt --include='.env' --include='.env.example'
```

### 9b. Hapus

```sh
sudo sultrakey uninstall
```

`uninstall` tidak menghapus file kunci di `/etc/sultrakey/` dan tidak menyentuh `.env`.

## 10. Backup dan pemulihan kunci

### Backup

- Simpan salinan `/etc/sultrakey/*.key` di tempat aman yang offline: bukan di server yang sama, bukan di
  git.
- Backup segera setelah `init` membuat kunci baru.
- `.env` boleh ikut di-backup bersama kuncinya. Tanpa kunci, isi `.env` tidak bisa dibuka.

### Pemulihan

```sh
sudo cp example-app.key /etc/sultrakey/example-app.key
sudo chown example-app /etc/sultrakey/example-app.key
sudo chmod 400 /etc/sultrakey/example-app.key
cd /opt/example-app && sudo -u example-app sultrakey check
```

Bila memakai `LoadCredential` ([4d](#4d-pilihan-kunci-lewat-loadcredential-systemd--247)), pemiliknya
`root`, bukan user aplikasi.

### Keamanan folder aplikasi

Siapa pun yang bisa **menulis** `.env` bisa mengganti password. Karena itu, jaga izin tulis folder
aplikasi. `check` memberi peringatan bila `.env` bisa diubah user lain.
