# Runbook sultrakey untuk tim infra

`sultrakey` menyimpan password aplikasi di file `.env` dalam bentuk terenkripsi. Saat aplikasi
dinyalakan, `sultrakey` membuka password itu dan memberikannya ke aplikasi. Kode aplikasi tidak
perlu diubah.

Aturan dasar:

- Setiap pesan error berisi dua baris: masalahnya (`✗ ...`) dan perintah perbaikannya (`Solusi: ...`).
- `sultrakey` tidak pernah menampilkan isi password.
- File kunci (`/etc/sultrakey/<app>.key`) wajib di-backup. **Kunci hilang = semua password harus diisi ulang.**

## 1. Pasang sultrakey di server

Server yang bisa mengakses github.com:

```sh
curl -fsSL https://github.com/hmrnsp/sultrakey/releases/latest/download/install.sh | sudo sh
sultrakey --version
```

Server tanpa internet:

1. Unduh `sultrakey-x86_64-unknown-linux-musl` (ARM: `sultrakey-aarch64-unknown-linux-musl`) dari
   halaman rilis di komputer lain.
2. Salin file itu ke server, misalnya dengan `scp`.
3. Jalankan:

   ```sh
   chmod +x sultrakey-x86_64-unknown-linux-musl
   sudo ./sultrakey-x86_64-unknown-linux-musl install
   ```

Hasilnya, program terpasang di `/usr/bin/sultrakey` dan folder kunci `/etc/sultrakey` sudah dibuat.
Satu binary ini jalan di Rocky Linux 8/9, Ubuntu, dan CentOS 7.

## 2. Siapkan aplikasi baru

Contoh: aplikasi `lakupandai` di `/opt/lakupandai`, dijalankan oleh user Linux `lakupandai`.
Folder aplikasi harus berisi `.env.example` (dibawa developer lewat git).
Value yang terisi di `.env.example` dipakai sebagai nilai bawaan untuk key biasa. Bila isinya contoh
palsu (misalnya `SMTP_HOST=smtp.example.com`), minta developer mengosongkannya dulu. Value di key rahasia
selalu diabaikan.

```sh
cd /opt/lakupandai
sudo sultrakey init lakupandai --owner lakupandai    # buat kunci + .env
sudo sultrakey setup                                  # isi semua yang masih kosong
sudo -u lakupandai sultrakey check                   # pastikan semuanya beres
```

Saat `setup`:

- Key rahasia (nama mengandung PASSWORD, KEY, TOKEN, dll., atau bertanda `@masked`) tampil sebagai `*`
  dan diketik **dua kali**. Key lain terlihat saat diketik.
- Untuk key biasa, nilai bawaan sudah tertulis di baris input. Tekan Enter untuk memakainya, atau
  hapus dengan Backspace lalu ketik nilai baru. Hapus semua lalu Enter untuk mengosongkan key yang boleh
  kosong.
- Key rahasia tidak punya nilai bawaan: selalu diketik.
- Isi yang panjangnya lebih dari satu baris (sertifikat, file kunci) diisi dari file: ketik `@/tmp/cert.pem`.
  Setelah selesai, hapus file itu.
- Tekan Ctrl+C untuk berhenti. Tidak ada yang tersimpan sampai semua pertanyaan dijawab.

Langsung **backup** file `/etc/sultrakey/lakupandai.key` ke tempat aman yang offline.

## 3. Ganti satu password

```sh
sudo sultrakey set DB_PASSWORD                            # diketik dua kali
sudo sultrakey set SSL_CERT --file /tmp/cert.pem          # dari file, lalu hapus filenya
sudo -u lakupandai sultrakey check
```

Setelah itu restart aplikasinya supaya password baru dipakai.

Lihat daftar key dan statusnya (isi password tidak pernah ditampilkan):

```sh
sultrakey list
```

## 4. Menjalankan aplikasi

Polanya sama untuk semua aplikasi: tambahkan `sultrakey run --` di depan perintah start yang biasa dipakai.

### pm2 (mode fork)

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
    // setting lain (log, max_memory_restart, dll.) tetap seperti biasa
  }]
};
```

- `stop_exit_codes: [78]`: bila password belum diisi atau kunci salah, pm2 berhenti dan tidak mencoba
  start berulang-ulang. Fitur ini butuh pm2 versi baru; cek dengan `pm2 --version`.
- Jalankan pm2 sebagai user aplikasi (`lakupandai`), karena hanya user itu yang boleh membaca kuncinya.
- **Jangan** menjalankan `sultrakey run -- pm2 start ...`. Dengan cara itu pm2 menyimpan password yang
  sudah dibuka, dan `pm2 save` menulisnya **polos** ke `~/.pm2/dump.pm2`.

### pm2 mode cluster

Mode cluster pm2 tidak bisa menjalankan `sultrakey`, karena hanya menerima file JavaScript.

1. Cek dulu apakah aplikasinya benar-benar butuh cluster. Jalankan `pm2 list` atau `pm2 monit` saat
   jam sibuk.
   - CPU tiap salinan rendah (di bawah ~50%): pindah ke mode fork 1 proses seperti di atas.
   - CPU satu salinan sering mendekati 100%: pakai systemd + `pm2-runtime` (uji dulu di satu server):

     ```ini
     ExecStart=/usr/bin/sultrakey run -- /usr/bin/pm2-runtime start ecosystem.config.js
     ```

2. Alternatif lain: beberapa proses fork dengan port berbeda, lalu nginx membagi bebannya.

Efek samping mode cluster yang sering tidak disadari:

- Memori berlipat sebanyak jumlah salinan.
- Jadwal otomatis (cron) jalan berkali-kali.
- Data di memori (misalnya sesi login) tidak dibagi antarsalinan.
- WebSocket butuh pengaturan tambahan.

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
journalctl -u lakupandai -n 50        # bila gagal, pesan ✗ dan Solusi ada di sini
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

Di server, pemilik file kunci harus uid yang sama dengan user di dalam image. Contoh:
`sudo chown 1001 /etc/sultrakey/lakupandai.key`, lalu izinnya tetap `400`.
CentOS 6 hanya didukung lewat Docker.

## 5. Pesan error yang sering muncul

| Pesan | Artinya | Yang dilakukan |
| --- | --- | --- |
| `✗ X belum diisi.` | Key wajib masih kosong. | `sudo sultrakey setup` |
| `✗ X tersimpan polos ...` | Ada password yang diketik manual tanpa enkripsi. | `sudo sultrakey setup` (otomatis dienkripsi) |
| `✗ Tidak punya izin membaca file kunci ...` | Dijalankan oleh user yang salah. | Jalankan sebagai user aplikasi: `sudo -u <user> sultrakey check` |
| `✗ Izin file kunci ... terlalu terbuka` | File kunci bisa dibaca user lain. | `sudo chmod 400 /etc/sultrakey/<app>.key` |
| `✗ File kunci ... bukan pasangan .env ...` | Kunci dan `.env` berasal dari aplikasi/server berbeda. | Pakai kunci yang benar, atau pulihkan dari backup. |
| `✗ File kunci ... tidak ditemukan.` | Kunci hilang atau salah lokasi. | Pulihkan dari backup. Bila tidak ada backup: hapus `.env`, jalankan `init` dan `setup` ulang. |
| `✗ X tidak bisa dibuka ...` | Isi satu key rusak. | `sudo sultrakey set X` |
| `✗ Perintah 'node' tidak ditemukan.` | Perintah aplikasi salah. | Tulis lokasi lengkapnya, contoh `/usr/bin/node`. |

Arti exit code: `0` sukses, `64` perintah salah ketik, `78` konfigurasi salah (aplikasi tidak
di-restart), `1` masalah lain.

## 6. Pindahkan `.env` lama yang masih polos

```sh
cd /opt/lakupandai
sudo sultrakey init lakupandai --owner lakupandai
```

Semua password yang sudah terisi langsung dienkripsi, kecuali key yang ditandai `@plain` di template.
`sultrakey` mencetak nama key yang dienkripsi. Isi passwordnya tidak dicetak.

## 7. Update dan hapus

```sh
sultrakey update --check     # hanya memeriksa
sudo sultrakey update        # memperbarui /usr/bin/sultrakey
```

- Update tidak pernah berjalan otomatis.
- Aplikasi yang sedang berjalan tidak terganggu. Versi baru dipakai saat aplikasi di-restart berikutnya.
- Server tanpa internet: salin binary baru, lalu `sudo ./sultrakey-... install`.

Menghapus program:

```sh
sudo sultrakey uninstall
```

`uninstall` tidak menghapus file kunci di `/etc/sultrakey/` dan tidak menyentuh `.env`.

## 8. Backup kunci

- Simpan salinan `/etc/sultrakey/*.key` di tempat aman yang offline (bukan di server yang sama,
  bukan di git).
- Untuk memulihkan: salin kembali ke `/etc/sultrakey/`, lalu
  `sudo chown <user-aplikasi> <file>` dan `sudo chmod 400 <file>`.
- Siapa pun yang bisa **menulis** `.env` bisa mengganti password. Karena itu, jaga izin tulis folder aplikasi.
