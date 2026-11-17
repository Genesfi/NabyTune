//! The words NabyTune shows, in the listener's language (Settings › Appearance › Language).
//!
//! The UI is written in English; `t` gives the Indonesian for each fixed string when the
//! language is "id", from one table below. A string the table doesn't have stays English.
//! Strings built from a template (counts, names, times) go through `tf`, whose template is
//! looked up the same way and then filled in; `plural` handles "1 song" / "5 songs", which
//! Indonesian says alike ("1 lagu", "5 lagu").

use std::collections::HashMap;
use std::fmt::Display;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

/// Whether the UI is in Indonesian (otherwise English).
static INDONESIAN: AtomicBool = AtomicBool::new(false);

/// Takes `code` ("en" or "id") as the UI's language; anything else is English.
pub fn set_language(code: &str) {
    INDONESIAN.store(code == "id", Ordering::Relaxed);
}

/// The UI's language: "en" or "id".
pub fn language() -> &'static str {
    if indonesian() { "id" } else { "en" }
}

/// Whether the UI is in Indonesian.
fn indonesian() -> bool {
    INDONESIAN.load(Ordering::Relaxed)
}

/// `english` in the UI's language.
pub fn t(english: &'static str) -> &'static str {
    lookup(english, indonesian())
}

/// `template` in the UI's language, each `{}` filled with the next of `args`.
pub fn tf(template: &'static str, args: &[&dyn Display]) -> String {
    fill(lookup(template, indonesian()), args)
}

/// A count of things, as "1 song" or "5 songs": `one` for exactly one in English, `many`
/// otherwise; Indonesian doesn't pluralise, so it always takes `many`'s translation.
pub fn plural(n: usize, one: &'static str, many: &'static str) -> String {
    plural_in(n, one, many, indonesian())
}

/// "1 song", "5 songs".
pub fn songs(n: usize) -> String {
    plural(n, "{} song", "{} songs")
}

fn lookup(english: &'static str, indonesian: bool) -> &'static str {
    if !indonesian {
        return english;
    }
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| TABLE.iter().copied().collect()).get(english).copied().unwrap_or(english)
}

fn fill(template: &str, args: &[&dyn Display]) -> String {
    let mut parts = template.split("{}");
    let mut out = parts.next().unwrap_or_default().to_string();
    let mut args = args.iter();
    for part in parts {
        if let Some(arg) = args.next() {
            out.push_str(&arg.to_string());
        }
        out.push_str(part);
    }
    out
}

fn plural_in(n: usize, one: &'static str, many: &'static str, indonesian: bool) -> String {
    let template = if n == 1 && !indonesian { one } else { many };
    fill(lookup(template, indonesian), &[&n])
}

/// English → Indonesian. Natural, short app Indonesian ("kamu", sentence case), as in
/// Indonesian music apps; brand names, file formats and key names stay as they are, and so
/// does any word Indonesian says the same way (Karaoke, Crossfade, Equalizer, Lossless…).
const TABLE: &[(&str, &str)] = &[
    // Counts and times
    ("{} song", "{} lagu"),
    ("{} songs", "{} lagu"),
    ("{} play", "{} kali diputar"),
    ("{} plays", "{} kali diputar"),
    ("{} min", "{} mnt"),
    ("{} songs • {} min", "{} lagu • {} mnt"),
    // Tabs and the title bar
    ("Home", "Beranda"),
    ("Library", "Koleksi"),
    ("Stats", "Statistik"),
    ("Search", "Cari"),
    ("Settings", "Setelan"),
    ("Back", "Kembali"),
    ("Search songs, artists, albums", "Cari lagu, artis, album"),
    // Home and Search
    ("Listen again", "Putar lagi"),
    ("Played lately", "Baru kamu putar"),
    ("Recommended for you", "Rekomendasi untukmu"),
    ("From what you've played lately", "Dari yang kamu putar belakangan ini"),
    ("Loading Home from YouTube Music…", "Memuat Beranda dari YouTube Music…"),
    ("Couldn't load YouTube Music Home: {}", "Gagal memuat Beranda YouTube Music: {}"),
    ("Searching YouTube Music…", "Mencari di YouTube Music…"),
    ("Results for “{}”", "Hasil untuk “{}”"),
    // Playing
    ("Play", "Putar"),
    ("Pause", "Jeda"),
    ("Next", "Berikutnya"),
    ("Previous", "Sebelumnya"),
    ("Shuffle", "Acak"),
    ("Repeat", "Ulangi"),
    ("Loading…", "Memuat…"),
    ("Opening “{}”…", "Membuka “{}”…"),
    ("Pick a song to start", "Pilih lagu untuk mulai"),
    ("{} · on this PC", "{} · di PC ini"),
    ("Queue is empty", "Antrean kosong"),
    ("Up Next", "Berikutnya"),
    // A song's menu
    ("Play next", "Putar berikutnya"),
    ("Playing next: {}", "Diputar berikutnya: {}"),
    ("Add to queue", "Tambahkan ke antrean"),
    ("Added to queue: {}", "Ditambahkan ke antrean: {}"),
    ("Remove from queue", "Hapus dari antrean"),
    ("Start radio", "Mulai radio"),
    ("Like", "Suka"),
    ("Remove from liked", "Hapus dari yang disukai"),
    ("Not for me", "Tidak suka"),
    ("Go to artist", "Buka artis"),
    ("Go to album", "Buka album"),
    ("Add to playlist…", "Tambahkan ke playlist…"),
    ("Copy link", "Salin tautan"),
    ("Copy title", "Salin judul"),
    ("Copy artist", "Salin nama artis"),
    ("Copied: {}", "Disalin: {}"),
    ("Copied YouTube Music link", "Tautan YouTube Music disalin"),
    ("Copied artist link", "Tautan artis disalin"),
    ("Subscribe", "Subscribe"),
    ("Subscribed", "Disubscribe"),
    ("Share", "Bagikan"),
    ("Subscribed to {}", "Berhasil subscribe ke {}"),
    ("Unsubscribed from {}", "Berhenti subscribe dari {}"),
    ("Sign in to YouTube Music to subscribe", "Masuk ke YouTube Music untuk subscribe"),
    ("Couldn't update subscription: {}", "Gagal memperbarui langganan: {}"),
    ("YouTube Music didn't take the rating: {}", "YouTube Music tidak menerima penilaianmu: {}"),
    ("ADD TO PLAYLIST", "TAMBAHKAN KE PLAYLIST"),
    ("New playlist", "Playlist baru"),
    ("ON YOUTUBE MUSIC", "DI YOUTUBE MUSIC"),
    ("Couldn't add to “{}”: {}", "Gagal menambahkan ke “{}”: {}"),
    // Radio
    ("Radio from {}", "Radio dari {}"),
    ("Radio from {} · finding songs…", "Radio dari {} · mencari lagu…"),
    ("Stop radio", "Hentikan radio"),
    ("The artist's radio didn't load", "Radio artis gagal dimuat"),
    // Lyrics
    ("Lyrics", "Lirik"),
    ("Finding lyrics…", "Mencari lirik…"),
    ("No lyrics found for this song", "Lirik lagu ini tidak ditemukan"),
    ("Play a song to see its lyrics", "Putar lagu untuk melihat liriknya"),
    ("Lyrics from {} · not time-synced", "Lirik dari {} · tidak tersinkron"),
    ("Layout & background", "Tata letak & latar"),
    ("Lyric style", "Gaya lirik"),
    ("LYRIC STYLE", "GAYA LIRIK"),
    ("Word by word", "Per kata"),
    ("Synced", "Tersinkron"),
    ("Not synced", "Tidak tersinkron"),
    ("Blur other lines", "Buramkan baris lain"),
    ("LYRICS SOURCE", "SUMBER LIRIK"),
    ("Timing", "Waktu"),
    ("{} failed. Click to retry.", "{} gagal. Klik untuk coba lagi."),
    // Lyric looks
    ("Focus", "Fokus"),
    ("Center", "Tengah"),
    ("One line", "Satu baris"),
    ("Far lines fade away", "Baris yang jauh memudar"),
    ("Words light up one by one", "Kata menyala satu per satu"),
    ("Centered, current line grows", "Di tengah, baris aktif membesar"),
    ("One big line, rising in turn", "Satu baris besar, naik bergantian"),
    ("Small and quiet", "Kecil dan tenang"),
    ("Current line glows lime", "Baris aktif menyala hijau lime"),
    // The full-screen player's layout menu
    ("LAYOUT", "TATA LETAK"),
    ("BACKGROUND", "LATAR"),
    ("MOVES WITH THE MUSIC", "BERGERAK DENGAN MUSIK"),
    ("Side by side", "Berdampingan"),
    ("Full cover", "Sampul penuh"),
    ("Lyrics only", "Hanya lirik"),
    ("Card", "Kartu"),
    ("Up next", "Berikutnya"),
    ("Vinyl", "Piringan hitam"),
    ("VINYL", "PIRINGAN HITAM"),
    ("Vinyl disc", "Mode piringan vinyl"),
    ("Show album art as a vinyl record across full-screen layouts", "Tampilkan cover album sebagai piringan vinyl di semua layout layar penuh"),
    ("Rotate record", "Putar piringan"),
    ("Rotate vinyl", "Putar vinyl"),
    ("Vinyl rotation", "Putaran piringan vinyl"),
    ("Spin the vinyl record and album art while playing", "Putar piringan dan gambar album saat memutar lagu"),
    ("SPEED", "KECEPATAN"),
    ("ROTATION SPEED", "KECEPATAN PUTAR"),
    ("Vinyl spin speed", "Kecepatan putaran vinyl"),
    ("How fast the vinyl disc and album art rotate", "Seberapa cepat piringan vinyl dan cover album berputar"),
    ("Ambient", "Ambien"),
    ("Blurred cover", "Sampul buram"),
    ("Cover", "Sampul"),
    ("Gradient", "Gradien"),
    ("Dark", "Gelap"),
    ("Full cover always uses the cover", "Sampul penuh selalu memakai sampul"),
    ("Off", "Mati"),
    ("Pulse", "Denyut"),
    ("Spectrum", "Spektrum"),
    ("Pulse and spectrum", "Denyut dan spektrum"),
    // Keyboard shortcuts
    ("Keyboard shortcuts", "Pintasan keyboard"),
    ("Play or pause", "Putar atau jeda"),
    ("Back or ahead 5 seconds", "Mundur atau maju 5 detik"),
    ("Volume up or down", "Volume naik atau turun"),
    ("Next or previous song", "Lagu berikutnya atau sebelumnya"),
    ("Mute", "Bisukan"),
    ("Like the song", "Sukai lagu"),
    ("Full screen", "Layar penuh"),
    ("Leave full screen", "Keluar dari layar penuh"),
    ("Lyrics earlier or later", "Lirik lebih awal atau lambat"),
    ("Mini player", "Pemutar mini"),
    ("Lyrics on the desktop", "Lirik di desktop"),
    ("Mouse back", "Tombol kembali mouse"),
    // Mini player, desktop lyrics and the tray
    ("NabyTune mini player", "Pemutar mini NabyTune"),
    ("NabyTune lyrics", "Lirik NabyTune"),
    ("Nothing playing", "Tidak ada yang diputar"),
    ("Pick a song in NabyTune", "Pilih lagu di NabyTune"),
    ("Show NabyTune", "Tampilkan NabyTune"),
    ("Quit NabyTune", "Keluar dari NabyTune"),
    // Library, artists, albums and playlists
    ("Liked songs", "Lagu yang disukai"),
    ("Your hearts, newest first", "Lagu yang kamu sukai, terbaru di atas"),
    ("Press ♡ on a song", "Tekan ♡ pada lagu"),
    ("Playlists", "Playlist"),
    ("Make one here, or press + on any song", "Buat di sini, atau tekan + pada lagu mana pun"),
    ("On YouTube Music", "Di YouTube Music"),
    ("Recently played", "Baru diputar"),
    ("Songs you play show up here", "Lagu yang kamu putar muncul di sini"),
    ("Rename", "Ganti nama"),
    ("Delete", "Hapus"),
    ("Click again to delete", "Klik lagi untuk menghapus"),
    ("No songs yet. Press + on any song to add it here.", "Belum ada lagu. Tekan + pada lagu mana pun untuk menambahkannya ke sini."),
    ("Playlist name", "Nama playlist"),
    ("ARTIST", "ARTIS"),
    ("Top songs", "Lagu teratas"),
    ("YouTube Music has no artist page for {}", "YouTube Music tidak punya halaman artis untuk {}"),
    ("This song's album isn't known", "Album lagu ini tidak diketahui"),
    ("YouTube Music has no page for “{}”", "YouTube Music tidak punya halaman untuk “{}”"),
    // Music on this PC
    ("On this PC", "Di PC ini"),
    ("Add to NabyTune", "Tambahkan ke NabyTune"),
    ("Add a folder", "Tambahkan folder"),
    ("Scan again", "Pindai lagi"),
    ("Reading your music…", "Membaca musikmu…"),
    (
        "Add a folder with music (MP3, FLAC, M4A, OGG, WAV) and it plays here, beside YouTube Music",
        "Tambahkan folder berisi musik (MP3, FLAC, M4A, OGG, WAV) dan musiknya bisa diputar di sini, di samping YouTube Music",
    ),
    ("No music found in these folders", "Tidak ada musik di folder ini"),
    ("Show fewer", "Tampilkan lebih sedikit"),
    ("Show all {}", "Tampilkan semua {}"),
    // Stats
    ("Reading your listening…", "Membaca riwayat dengarmu…"),
    ("Nothing yet: play a few songs and they'll show up here", "Belum ada apa-apa: putar beberapa lagu dan semuanya akan muncul di sini"),
    ("No listening in this stretch", "Tidak ada yang didengarkan di periode ini"),
    ("Week", "Minggu"),
    ("Month", "Bulan"),
    ("Year", "Tahun"),
    ("All time", "Sepanjang waktu"),
    ("This week", "Minggu ini"),
    ("Last week", "Minggu lalu"),
    ("Top artists", "Artis teratas"),
    ("Minutes listened", "Menit didengarkan"),
    ("Songs played", "Lagu diputar"),
    ("{} different", "{} berbeda"),
    ("Artists", "Artis"),
    ("{} new to you", "{} baru buatmu"),
    ("Day streak", "Hari beruntun"),
    ("Longest {} · {} days with music", "Terpanjang {} · {} hari dengan musik"),
    ("▲ {}% on the one before", "▲ {}% dari periode sebelumnya"),
    ("▼ {}% on the one before", "▼ {}% dari periode sebelumnya"),
    ("Most on {} ({} min)", "Paling banyak pada {} ({} mnt)"),
    ("When you listen", "Kapan kamu mendengarkan"),
    ("Mostly around {}:00, and most on {}s", "Paling sering sekitar pukul {}.00, terutama hari {}"),
    ("By hour", "Per jam"),
    ("By day", "Per hari"),
    ("New to you", "Baru buatmu"),
    ("{} artists you hadn't played before", "{} artis yang belum pernah kamu putar"),
    ("Your stretch on one card. Save it as a picture to share.", "Periodemu dalam satu kartu. Simpan sebagai gambar untuk dibagikan."),
    ("minutes of music", "menit musik"),
    ("YOUR SONG", "LAGUMU"),
    ("TOP SONGS", "LAGU TERATAS"),
    ("TOP ARTISTS", "ARTIS TERATAS"),
    ("SONGS", "LAGU"),
    ("ARTISTS", "ARTIS"),
    ("BEST HOUR", "JAM TERBAIK"),
    ("DAY STREAK", "HARI BERUNTUN"),
    ("Save as picture", "Simpan sebagai gambar"),
    ("Scroll until the whole card shows, then save", "Gulir sampai seluruh kartu terlihat, lalu simpan"),
    ("Saved to {}", "Disimpan ke {}"),
    ("Couldn't save it: {}", "Gagal menyimpan: {}"),
    ("January", "Januari"),
    ("February", "Februari"),
    ("March", "Maret"),
    ("May", "Mei"),
    ("June", "Juni"),
    ("July", "Juli"),
    ("August", "Agustus"),
    ("October", "Oktober"),
    ("December", "Desember"),
    ("Monday", "Senin"),
    ("Tuesday", "Selasa"),
    ("Wednesday", "Rabu"),
    ("Thursday", "Kamis"),
    ("Friday", "Jumat"),
    ("Saturday", "Sabtu"),
    ("Sunday", "Minggu"),
    // Quick setup
    ("Welcome to NabyTune", "Selamat datang di NabyTune"),
    ("Make it yours", "Sesuaikan sesukamu"),
    ("Your YouTube Music", "YouTube Music-mu"),
    ("Lyrics, your way", "Lirik, sesuai gayamu"),
    ("One last thing", "Satu hal lagi"),
    ("Skip setup", "Lewati penyiapan"),
    ("Get started", "Mulai"),
    ("Start listening", "Mulai mendengarkan"),
    (
        "YouTube Music on Windows, made light and quiet: synced lyrics with romaji, a full-screen player, your library, and lossless where you have it. A minute here sets it up the way you like.",
        "YouTube Music di Windows, ringan dan tenang: lirik tersinkron dengan romaji, pemutar layar penuh, koleksimu, dan lossless kalau kamu punya. Semenit di sini cukup untuk menyiapkannya sesuai seleramu.",
    ),
    ("Cover follows each song's artwork; any colour at all is in Settings", "Sampul mengikuti artwork tiap lagu; warna apa pun ada di Setelan"),
    ("Window", "Jendela"),
    (
        "What shows behind the app: your wallpaper's colours (Mica), frosted glass, or plain",
        "Yang tampil di belakang aplikasi: warna wallpaper-mu (Mica), kaca buram, atau polos",
    ),
    ("Signed in as {}", "Masuk sebagai {}"),
    ("Sign in with Google", "Masuk dengan Google"),
    (
        "Optional. Signed in, Home is made for you, and your playlists and liked songs come along. Google's own page opens: your password goes to Google, never through NabyTune.",
        "Opsional. Setelah masuk, Beranda dibuat khusus untukmu, dan playlist serta lagu yang kamu sukai ikut terbawa. Yang terbuka adalah halaman Google sendiri: kata sandimu dikirim ke Google, tidak pernah lewat NabyTune.",
    ),
    ("Japanese, Chinese, Korean and more, in Latin letters under each line", "Jepang, Mandarin, Korea, dan lainnya, dalam huruf Latin di bawah tiap baris"),
    (
        "NabyTune is free, with no ads, and made by one developer. If it becomes part of your day, you can support it — it's what keeps it going. (It's in Settings too, any time.)",
        "NabyTune gratis, tanpa iklan, dan dibuat oleh satu developer. Kalau NabyTune jadi bagian harimu, kamu bisa mendukungnya — itulah yang membuatnya terus berjalan. (Ada juga di Setelan, kapan saja.)",
    ),
    // Settings › support and account
    ("Support NabyTune", "Dukung NabyTune"),
    (
        "Free, no ads, made by one developer. If it's part of your day, a coffee helps keep it going.",
        "Gratis, tanpa iklan, dibuat oleh satu developer. Kalau NabyTune jadi bagian harimu, secangkir kopi membantunya terus berjalan.",
    ),
    ("Account", "Akun"),
    ("Signed in to YouTube Music", "Masuk ke YouTube Music"),
    ("{} · signed in to YouTube Music", "{} · masuk ke YouTube Music"),
    ("Sign in", "Masuk"),
    ("Sign out", "Keluar"),
    ("Waiting", "Menunggu"),
    ("Finish signing in in the window that opened…", "Selesaikan proses masuk di jendela yang terbuka…"),
    ("Signed in; loading your account…", "Sudah masuk; memuat akunmu…"),
    ("Sign in for your library, your playlists and a Home made for you", "Masuk untuk koleksimu, playlist-mu, dan Beranda yang dibuat untukmu"),
    // Settings › Appearance
    ("Appearance", "Tampilan"),
    ("Accent colour", "Warna aksen"),
    (
        "Buttons, highlights and the song that's playing · {}. Pick one, or any colour and brightness on the strips",
        "Tombol, sorotan, dan lagu yang sedang diputar · {}. Pilih salah satu, atau warna dan kecerahan apa pun di strip",
    ),
    ("Custom", "Kustom"),
    ("Neutral", "Netral"),
    ("Sky", "Langit"),
    ("Coral", "Koral"),
    ("Rose", "Mawar"),
    ("Window material", "Material jendela"),
    ("Solid", "Polos"),
    ("Your wallpaper's colours, softly, behind the page", "Warna wallpaper-mu, lembut, di belakang halaman"),
    ("Mica with more of the wallpaper's colour", "Mica dengan lebih banyak warna wallpaper"),
    ("Frosted glass over whatever is behind the window", "Kaca buram di atas apa pun yang ada di belakang jendela"),
    ("A plain dark ground: nothing shows through", "Latar gelap polos: tidak ada yang tembus"),
    ("Ambient background", "Latar ambien"),
    ("The playing song's cover, as a soft glow behind everything", "Sampul lagu yang diputar, sebagai cahaya lembut di belakang semuanya"),
    ("Language", "Bahasa"),
    ("Menus, buttons and messages in NabyTune", "Menu, tombol, dan pesan di NabyTune"),
    // Settings › Playback
    ("Playback", "Pemutaran"),
    ("Normalise volume", "Normalkan volume"),
    ("Loud songs are turned down to the same level, as on YouTube Music", "Lagu yang keras dikecilkan ke level yang sama, seperti di YouTube Music"),
    ("Autoplay", "Putar otomatis"),
    ("Autoplay on · similar vibes", "Putar otomatis aktif · vibes serupa"),
    ("Autoplay off", "Putar otomatis nonaktif"),
    ("Finding similar vibes…", "Mencari lagu dengan vibes serupa…"),
    ("Play similar vibes automatically", "Putar lagu dengan vibes serupa otomatis"),
    ("Turn on Autoplay to keep matching songs lined up next", "Aktifkan Putar otomatis agar antrean selalu terisi lagu yang cocok"),
    ("Keep playing songs with similar vibes when the queue runs low", "Terus memutar lagu dengan vibes serupa saat antrean hampir habis"),
    ("When the queue's last song starts, songs like it line up after it", "Saat lagu terakhir di antrean dimulai, lagu yang mirip ikut berbaris setelahnya"),
    (
        "Songs that run into the next overlap by this long, one fading out as the other fades in",
        "Lagu yang bersambung saling bertumpuk selama ini, yang satu memudar saat yang lain masuk",
    ),
    ("Shape the sound: more bass, clearer voices, or your own curve", "Atur suaranya: bass lebih tebal, vokal lebih jernih, atau kurvamu sendiri"),
    ("Presets", "Preset"),
    ("Flat", "Datar"),
    ("Vocal", "Vokal"),
    ("Loud", "Keras"),
    ("Soft", "Lembut"),
    ("Pick up where you left off", "Lanjutkan dari terakhir kali"),
    ("When NabyTune opens, the last queue is back, paused where it was", "Saat NabyTune dibuka, antrean terakhir kembali, dijeda di posisi terakhirnya"),
    ("Sleep timer", "Timer tidur"),
    ("15 min", "15 mnt"),
    ("30 min", "30 mnt"),
    ("45 min", "45 mnt"),
    ("1 hr", "1 jam"),
    ("1.5 hr", "1,5 jam"),
    ("End of song", "Akhir lagu"),
    ("Pauses when this song ends", "Jeda saat lagu ini selesai"),
    ("Pauses in {} min", "Jeda dalam {} mnt"),
    ("Pause playback after a while, or when the song ends", "Jeda pemutaran setelah beberapa saat, atau saat lagu selesai"),
    // Settings › Lossless
    ("Play lossless", "Putar lossless"),
    ("Now playing {} from {}", "Sedang memutar {} dari {}"),
    ("Now playing {} from YouTube", "Sedang memutar {} dari YouTube"),
    ("Songs an addon has play lossless; the rest stay on YouTube", "Lagu yang ada di addon diputar lossless; sisanya tetap dari YouTube"),
    ("Quality", "Kualitas"),
    ("Hi-Res is larger: up to 24-bit/192 kHz, where the addon has it", "Hi-Res lebih besar: hingga 24-bit/192 kHz, jika addon menyediakannya"),
    ("Not answering: {}", "Tidak merespons: {}"),
    ("Connecting…", "Menghubungkan…"),
    ("Reading its manifest…", "Membaca manifesnya…"),
    ("Paste an addon's address and press Enter. None is built in.", "Tempel alamat addon lalu tekan Enter. Tidak ada addon bawaan."),
    ("Add an addon", "Tambahkan addon"),
    ("Add", "Tambah"),
    ("Addon address (https://…)", "Alamat addon (https://…)"),
    // Settings › Lyrics
    (
        "Lyrics in other scripts, in Latin letters under each line: Japanese, Chinese, Korean, Russian, Greek, Hindi, Thai, Arabic",
        "Lirik berhuruf lain, dalam huruf Latin di bawah tiap baris: Jepang, Mandarin, Korea, Rusia, Yunani, Hindi, Thai, Arab",
    ),
    ("Lines away from the one being sung go softly out of focus", "Baris yang jauh dari yang sedang dinyanyikan perlahan jadi buram"),
    ("Preferred source", "Sumber utama"),
    ("Best available", "Terbaik yang ada"),
    ("Shown first when a song has it; the lyric menu can still switch", "Ditampilkan duluan jika lagunya punya; menu lirik tetap bisa menggantinya"),
    ("Sources picked per song", "Sumber yang dipilih per lagu"),
    ("No song has a source picked by hand", "Belum ada lagu yang sumbernya dipilih manual"),
    ("{} song keeps the source you picked for it", "{} lagu memakai sumber yang kamu pilih"),
    ("{} songs keep the source you picked for them", "{} lagu memakai sumber yang kamu pilih"),
    ("Reset", "Atur ulang"),
    ("Lyric timing", "Waktu lirik"),
    ("When the lyrics run ahead or behind this song. [ and ] do it while it plays", "Saat lirik lagu ini terlalu cepat atau lambat. [ dan ] juga bisa saat lagu diputar"),
    ("Play a song to nudge its lyrics earlier or later ([ and ] while it plays)", "Putar lagu untuk menggeser liriknya lebih awal atau lambat ([ dan ] saat diputar)"),
    ("Earlier", "Lebih awal"),
    ("Later", "Lebih lambat"),
    // The full-screen rail, the mini player, the desktop lyrics' backdrops, the cover accent
    ("Romaji", "Romaji"),
    ("Hide romaji", "Sembunyikan romaji"),
    ("Hide lyrics on the desktop", "Sembunyikan lirik di desktop"),
    ("Shadow", "Bayangan"),
    ("See-through", "Tembus pandang"),
    ("Behind the desktop lyrics", "Latar lirik di desktop"),
    ("Just the words, straight on whatever is underneath", "Hanya teksnya, langsung di atas apa pun yang ada di bawahnya"),
    ("A soft dark glow with no edges, so the words read on any wallpaper", "Bayangan gelap lembut tanpa tepi, supaya teks tetap terbaca di wallpaper apa pun"),
    ("A light see-through plate behind the words", "Panel tipis tembus pandang di belakang teks"),
    ("A dark plate, for busy backgrounds", "Panel gelap, untuk latar yang ramai"),
    ("Border round the desktop lyrics", "Garis tepi lirik di desktop"),
    ("A thin line round the plate", "Garis tipis di sekeliling panel"),
    ("Colour from the album cover", "Warna dari sampul album"),
    ("The accent takes the colour of the cover that's playing, and changes with each song", "Warna aksen mengikuti sampul lagu yang sedang diputar, dan berganti di setiap lagu"),
    ("Shown until a song's cover gives its colour. Picking one here turns the cover colour off", "Dipakai sampai sampul lagu memberi warnanya. Memilih warna di sini mematikan warna dari sampul"),
    // Settings › About: where NabyTune comes from, and its licence
    ("Credits", "Kredit"),
    ("Inspired by and built on BitChord, © Kushagra Singh and its contributors. Changed into NabyTune by BillyNabil, since 2026.", "Terinspirasi dari dan dibangun di atas BitChord, © Kushagra Singh dan para kontributornya. Diubah menjadi NabyTune oleh BillyNabil, sejak 2026."),
    ("Licence", "Lisensi"),
    ("GNU General Public License v3: you may share and change NabyTune under its terms. It comes with no warranty of any kind. The source code is available on request.", "GNU General Public License v3: kamu boleh membagikan dan mengubah NabyTune sesuai ketentuannya. NabyTune diberikan tanpa garansi apa pun. Kode sumbernya tersedia atas permintaan."),
    ("Read the licence", "Baca lisensinya"),
    ("AAC 128 kbps · another upload", "AAC 128 kbps · unggahan lain"),
    ("Unlock", "Buka kunci"),
    ("Lock", "Kunci"),
    ("Drag the words to move them · Lock lets clicks pass through", "Geser liriknya untuk memindahkan · Kunci agar klik tembus"),
    ("Desktop lyrics alignment", "Posisi lirik di desktop"),
    ("Left or right keeps the words to one side, so they can sit in a corner of the screen", "Kiri atau kanan menaruh lirik di satu sisi, jadi bisa ditaruh di pojok layar"),
    ("Desktop lyrics opacity", "Transparansi lirik di desktop"),
    ("They stay on top of every window; fainter words get in the way less", "Lirik selalu di atas semua window; makin transparan, makin tidak mengganggu"),
    ("Left", "Kiri"),
    ("Right", "Kanan"),
    // Settings › Windows and Discord
    ("Windows and Discord", "Windows dan Discord"),
    ("Show on Discord", "Tampilkan di Discord"),
    ("Your Discord status says what you're listening to, with the cover and a link", "Status Discord-mu menampilkan yang sedang kamu dengarkan, lengkap dengan sampul dan tautan"),
    ("Keep running in the tray", "Tetap berjalan di tray"),
    ("The close button hides NabyTune by the clock instead of quitting; the music goes on", "Tombol tutup menyembunyikan NabyTune di dekat jam, bukan menutupnya; musik tetap berjalan"),
    ("The tray icon isn't available right now", "Ikon tray sedang tidak tersedia"),
    ("A small window that stays on top, with the song, the sung line and the buttons (Ctrl+M)", "Jendela kecil yang selalu di atas, berisi lagu, baris yang dinyanyikan, dan tombolnya (Ctrl+M)"),
    ("Open", "Buka"),
    ("The sung line floats over everything, games and other apps too (Ctrl+D)", "Baris yang dinyanyikan melayang di atas semuanya, termasuk game dan aplikasi lain (Ctrl+D)"),
    ("Lock the desktop lyrics", "Kunci lirik desktop"),
    (
        "Locked, clicks go through them to what's underneath; unlock to move or size them",
        "Saat terkunci, klik tembus ke yang ada di bawahnya; buka kuncinya untuk memindah atau mengubah ukuran",
    ),
    // Settings › About and updates
    ("About", "Tentang"),
    ("Version {} · native build (GPUI, Rust audio engine)", "Versi {} · build native (GPUI, mesin audio Rust)"),
    ("YouTube Music, your way", "YouTube Music, sesuai gayamu"),
    ("Updates", "Pembaruan"),
    ("Settings file", "File setelan"),
    ("Open folder", "Buka folder"),
    ("Check for updates", "Periksa pembaruan"),
    ("Checking…", "Memeriksa…"),
    ("Updates come from NabyTune's releases on GitHub", "Pembaruan diambil dari rilis NabyTune di GitHub"),
    ("Updates aren't set up for this copy yet", "Pembaruan belum diatur untuk salinan ini"),
    ("You have the newest version", "Kamu sudah memakai versi terbaru"),
    ("Couldn't check: {}", "Gagal memeriksa: {}"),
    ("Version {} is out", "Versi {} sudah tersedia"),
    ("Update to {}", "Perbarui ke {}"),
    ("Open the release", "Buka rilisnya"),
    ("Downloading… {} of {} MB", "Mengunduh… {} dari {} MB"),
    // Stats and Wrapped (redesign)
    ("What you've been listening to, in plain numbers", "Yang kamu dengarkan, dalam angka yang gampang dibaca"),
    ("Back to now", "Kembali ke sekarang"),
    ("{} minute", "{} menit"),
    ("{} minutes", "{} menit"),
    ("{} hour", "{} jam"),
    ("{} hours", "{} jam"),
    ("{} artist", "{} artis"),
    ("{} artists", "{} artis"),
    ("You've listened for {} in all", "Total kamu sudah mendengarkan musik selama {}"),
    ("You listened for {} this week", "Minggu ini kamu mendengarkan musik selama {}"),
    ("You listened for {} last week", "Minggu lalu kamu mendengarkan musik selama {}"),
    ("You listened for {} this month", "Bulan ini kamu mendengarkan musik selama {}"),
    ("You listened for {} this year", "Tahun ini kamu mendengarkan musik selama {}"),
    ("You listened for {} in {}", "Kamu mendengarkan musik selama {} di {}"),
    ("That's {} by {}.", "Itu {} dari {}."),
    ("About the same as the week before.", "Kurang lebih sama dengan minggu sebelumnya."),
    ("About the same as the month before.", "Kurang lebih sama dengan bulan sebelumnya."),
    ("About the same as the year before.", "Kurang lebih sama dengan tahun sebelumnya."),
    ("{}% more than the week before.", "{}% lebih banyak dari minggu sebelumnya."),
    ("{}% more than the month before.", "{}% lebih banyak dari bulan sebelumnya."),
    ("{}% more than the year before.", "{}% lebih banyak dari tahun sebelumnya."),
    ("{}% less than the week before.", "{}% lebih sedikit dari minggu sebelumnya."),
    ("{}% less than the month before.", "{}% lebih sedikit dari bulan sebelumnya."),
    ("{}% less than the year before.", "{}% lebih sedikit dari tahun sebelumnya."),
    ("Your #1 song is “{}” by {}", "Lagu nomor 1-mu: “{}” dari {}"),
    ("Just getting started: the more you listen, the fuller this gets.", "Baru mulai nih: makin sering kamu dengar, makin lengkap isinya."),
    ("Play your top songs", "Putar lagu teratasmu"),
    ("No music here yet", "Belum ada musik di sini"),
    (
        "Nothing was played in this stretch. Pick another one above, or see everything at once.",
        "Tidak ada yang diputar di periode ini. Pilih periode lain di atas, atau lihat semuanya sekaligus.",
    ),
    ("See all time", "Lihat sepanjang waktu"),
    ("Days in a row", "Hari berturut-turut"),
    ("Longest: {} day", "Terpanjang: {} hari"),
    ("Longest: {} days", "Terpanjang: {} hari"),
    ("Show top 10", "Tampilkan 10 teratas"),
    ("Played the most. Click one to play it.", "Yang paling sering diputar. Klik untuk memutarnya."),
    ("By minutes heard. Click one to find their songs.", "Menurut menit yang didengarkan. Klik untuk mencari lagu-lagunya."),
    ("Hide", "Sembunyikan"),
    ("Undo", "Urungkan"),
    ("“{}” is hidden from your stats", "“{}” disembunyikan dari statistikmu"),
    ("{} is hidden from your stats, with their songs", "{} disembunyikan dari statistikmu, beserta lagu-lagunya"),
    ("You're a morning listener", "Kamu pendengar pagi"),
    ("You're an afternoon listener", "Kamu pendengar siang"),
    ("You're an evening listener", "Kamu pendengar sore"),
    ("You're a night owl", "Kamu si burung hantu"),
    ("{}: most of your music plays around {}.", "{}: musikmu paling sering diputar sekitar pukul {}."),
    ("{} is your biggest day of the week.", "{} jadi hari paling ramai musikmu dalam seminggu."),
    ("Your Wrapped", "Wrapped-mu"),
    ("Your stretch on one card. Pick a style, then save it as a picture to share.", "Periodemu dalam satu kartu. Pilih gayanya, lalu simpan sebagai gambar untuk dibagikan."),
    ("Card style", "Gaya kartu"),
    (
        "Something on the card you'd rather not share? Hover it in Top songs or Top artists and press Hide.",
        "Ada yang nggak mau kamu bagikan di kartu? Arahkan kursor ke lagu atau artisnya di Lagu teratas atau Artis teratas, lalu tekan Sembunyikan.",
    ),
    ("Signature", "Khas"),
    ("Poster", "Poster"),
    ("Minimal", "Minimalis"),
    ("Receipt", "Struk"),
    ("Neon", "Neon"),
    ("Aurora", "Aurora"),
    ("Cassette", "Kaset"),
    ("Polaroid", "Polaroid"),
    ("Big numbers", "Angka besar"),
    ("Magazine", "Majalah"),
    ("Saving…", "Menyimpan…"),
    ("Make the window taller so the whole card fits, then save", "Perbesar tinggi jendela sampai seluruh kartu muat, lalu simpan"),
    ("minutes", "menit"),
    ("My No. 1", "No. 1-ku"),
    ("Different songs", "Lagu berbeda"),
    ("TOTAL", "TOTAL"),
    ("{} MIN", "{} MNT"),
    ("THANK YOU FOR LISTENING", "TERIMA KASIH SUDAH MENDENGARKAN"),
    ("THE MUSIC ISSUE", "EDISI MUSIK"),
    ("COVER STORY", "LIPUTAN UTAMA"),
    ("ALSO INSIDE", "JUGA DI EDISI INI"),
    ("SIDE A · TOP SONGS", "SISI A · LAGU TERATAS"),
    ("SIDE B · TOP ARTISTS", "SISI B · ARTIS TERATAS"),
    ("Manage your stats", "Kelola statistikmu"),
    ("Hidden songs and artists", "Lagu dan artis yang disembunyikan"),
    (
        "Left out of everything here, minutes and Wrapped too. Hiding an artist hides songs they're on. Hover a song or artist above and press Hide.",
        "Tidak dihitung di mana pun di sini, termasuk menit dan Wrapped. Menyembunyikan artis juga menyembunyikan lagu yang ada dia di dalamnya. Arahkan kursor ke lagu atau artis di atas, lalu tekan Sembunyikan.",
    ),
    ("None", "Tidak ada"),
    ("Close", "Tutup"),
    ("Song", "Lagu"),
    ("Artist", "Artis"),
    ("Show again", "Tampilkan lagi"),
    ("Couldn't save that: {}", "Gagal menyimpannya: {}"),
    ("Clear listening history", "Hapus riwayat dengar"),
    ("Start your stats and Wrapped over. Your playlists, likes and songs stay as they are.", "Mulai ulang statistik dan Wrapped-mu. Playlist, lagu yang disukai, dan lagu-lagumu tetap aman."),
    ("Clear…", "Hapus…"),
    ("Cancel", "Batal"),
    ("Clear your listening history for good?", "Hapus riwayat dengarmu selamanya?"),
    ("This can't be undone. Hidden songs and artists stay hidden.", "Ini tidak bisa dibatalkan. Lagu dan artis yang disembunyikan tetap tersembunyi."),
    ("Clear this week", "Hapus minggu ini"),
    ("Clear last week", "Hapus minggu lalu"),
    ("Clear this month", "Hapus bulan ini"),
    ("Clear this year", "Hapus tahun ini"),
    ("Clear {}", "Hapus {}"),
    ("Clear everything", "Hapus semuanya"),
    ("{} is cleared", "{} sudah dihapus"),
    ("Your listening history is cleared", "Riwayat dengarmu sudah dihapus"),
    ("Couldn't clear it: {}", "Gagal menghapus: {}"),
    (
        "How it's counted: a song counts as played once you've heard 30 seconds of it (or half, for a short one). Minutes count everything you heard. It all stays on this PC.",
        "Cara hitungnya: lagu dianggap diputar setelah kamu dengar 30 detik (atau separuhnya, untuk lagu pendek). Menit menghitung semua yang kamu dengar. Semuanya tetap tersimpan di PC ini.",
    ),
    ("Your stats start here", "Statistikmu mulai di sini"),
    (
        "Play some music and NabyTune keeps count: your top songs and artists, when you listen, and a Wrapped card to share.",
        "Putar musik dan NabyTune akan mencatatnya: lagu dan artis teratasmu, kapan kamu mendengarkan, dan kartu Wrapped untuk dibagikan.",
    ),
    ("It all stays on this PC.", "Semuanya tetap tersimpan di PC ini."),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_sound() {
        let mut seen = std::collections::HashSet::new();
        for (english, indonesian) in TABLE {
            assert!(seen.insert(english), "{english:?} is in the table twice");
            assert!(!indonesian.trim().is_empty(), "{english:?} has no Indonesian");
            assert_eq!(english.matches("{}").count(), indonesian.matches("{}").count(), "{english:?}: the {{}} don't match");
        }
    }

    #[test]
    fn counts_and_templates() {
        // The global language is left alone: other modules' tests read it.
        assert_eq!(plural_in(1, "{} song", "{} songs", false), "1 song");
        assert_eq!(plural_in(5, "{} song", "{} songs", false), "5 songs");
        assert_eq!(plural_in(1, "{} song", "{} songs", true), "1 lagu");
        assert_eq!(plural_in(5, "{} song", "{} songs", true), "5 lagu");
        assert_eq!(lookup("No such string", true), "No such string");
        assert_eq!(fill(lookup("Radio from {}", true), &[&"Tulus"]), "Radio dari Tulus");
        assert_eq!(fill("{} of {}", &[&1, &2]), "1 of 2");
    }
}
