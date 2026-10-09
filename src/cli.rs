use std::ffi::OsString;
use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "sultrakey",
    version,
    about = "File .env terenkripsi, lalu menjalankan aplikasi dengan value yang sudah dibuka",
    after_help = "Alur di server:\n  \
        sudo sultrakey init <app> --owner <user-aplikasi>\n  \
        sultrakey fill\n  \
        sultrakey check\n  \
        sultrakey run -- <perintah aplikasi>"
)]
pub struct Cli {
    #[command(flatten)]
    pub global: Global,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Clone, Args)]
pub struct Global {
    /// Lokasi file .env
    #[arg(long, global = true, value_name = "FILE", default_value = ".env")]
    pub env: PathBuf,

    /// Lokasi file template
    #[arg(
        long,
        global = true,
        value_name = "FILE",
        default_value = ".env.template"
    )]
    pub template: PathBuf,

    /// Lokasi file kunci (bawaan: /etc/sultrakey/<app>.key; Windows: %APPDATA%\sultrakey\<app>.key)
    #[arg(long, global = true, value_name = "FILE")]
    pub key_file: Option<PathBuf>,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    /// Buat kunci (bila belum ada) dan .env dari template, atau enkripsi .env polos lama
    Init {
        /// Nama aplikasi: huruf kecil, angka, tanda minus
        app: String,
        /// Pemilik file kunci dan .env: user[:group] aplikasi (wajib saat memakai sudo)
        #[arg(long, value_name = "USER[:GROUP]")]
        owner: Option<String>,
    },

    /// Samakan .env dengan template, lalu tanyakan key yang masih kosong
    Fill,

    /// Ganti value satu key (dari prompt tersembunyi, --stdin, atau --file)
    Set {
        /// Nama key, persis seperti di .env
        key: String,
        /// Baca value dari stdin (seluruh isinya)
        #[arg(long, conflicts_with = "file")]
        stdin: bool,
        /// Baca value dari file (untuk isi banyak baris, misalnya sertifikat)
        #[arg(long, value_name = "FILE")]
        file: Option<PathBuf>,
    },

    /// Tampilkan nama key dan statusnya (value tidak pernah ditampilkan)
    List,

    /// Periksa semua key terisi, bisa dibuka, dan kunci cocok
    Check,

    /// Periksa, buka value, lalu jalankan aplikasi dengan value itu di environment
    Run {
        /// Perintah aplikasi, ditulis setelah --, contoh: sultrakey run -- node dist/main.js
        #[arg(last = true, required = true, value_name = "PERINTAH")]
        command: Vec<OsString>,
    },

    /// Pasang sultrakey ini ke /usr/bin (sudo), ~/.local/bin, atau folder program Windows
    Install,

    /// Perbarui sultrakey ke rilis terbaru
    Update {
        /// Hanya periksa apakah ada versi baru (exit 1 bila ada)
        #[arg(long)]
        check: bool,
        /// Jangan tanya konfirmasi
        #[arg(short, long)]
        yes: bool,
    },

    /// Hapus sultrakey yang terpasang (file kunci dan .env tidak disentuh)
    Uninstall {
        /// Jangan tanya konfirmasi
        #[arg(short, long)]
        yes: bool,
    },
}
