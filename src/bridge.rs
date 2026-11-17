//! FocusGrow & DynamicNotification WebSocket Bridge (ws://127.0.0.1:8765):
//! Implements a lightweight, zero-dependency RFC 6455 WebSocket server using std::net,
//! sha1_smol and base64. Allows FocusGrow and DynamicNotification to connect directly to NabyTune.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::engine::general_purpose::STANDARD;
use base64::Engine as _;
use serde_json::{json, Value};

#[derive(Clone, Debug)]
pub enum BridgeEvent {
    PlayPause,
    Next,
    Previous,
    ToggleWindow,
    Seek(Duration),
}

pub struct Bridge {
    events: Receiver<BridgeEvent>,
    latest: Arc<Mutex<Option<String>>>,
    clients: Arc<Mutex<Vec<TcpStream>>>,
}

impl Bridge {
    pub fn new() -> Option<Self> {
        let (sender, events) = channel();
        let latest = Arc::new(Mutex::new(None));
        let clients: Arc<Mutex<Vec<TcpStream>>> = Arc::new(Mutex::new(Vec::new()));

        let listener = TcpListener::bind("127.0.0.1:8765").ok()?;
        let _ = listener.set_nonblocking(true);

        let s_clone = sender;
        let l_clone = Arc::clone(&latest);
        let c_clone = Arc::clone(&clients);

        thread::Builder::new()
            .name("ytmpx-bridge".into())
            .spawn(move || {
                run_server(listener, s_clone, l_clone, c_clone);
            })
            .ok()?;

        Some(Self {
            events,
            latest,
            clients,
        })
    }

    pub fn events(&self) -> Vec<BridgeEvent> {
        self.events.try_iter().collect()
    }

    pub fn broadcast_track(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        cover_url: Option<&str>,
        duration: Duration,
        video_id: &str,
        playing: bool,
    ) {
        let payload = json!({
            "event": "track",
            "source": "nabytune",
            "exeName": "nabytune.exe",
            "isPlaying": playing,
            "metadata": {
                "title": title,
                "author": if artist.is_empty() { "YouTube Music" } else { artist },
                "artist": artist,
                "url": format!("https://music.youtube.com/watch?v={video_id}"),
                "image": cover_url.unwrap_or_default(),
                "coverUrl": cover_url.unwrap_or_default(),
                "totalDuration": duration.as_millis() as u64,
                "elapsed": 0.0,
                "playlist": album.unwrap_or_default()
            }
        })
        .to_string();

        *self.latest.lock().unwrap() = Some(payload.clone());
        broadcast(&self.clients, &payload);
    }

    pub fn broadcast_state(&self, playing: bool) {
        let payload = json!({
            "event": if playing { "resume" } else { "pause" },
            "isPlaying": playing,
            "source": "nabytune",
            "exeName": "nabytune.exe"
        })
        .to_string();

        broadcast(&self.clients, &payload);
    }
}

fn run_server(
    listener: TcpListener,
    event_tx: Sender<BridgeEvent>,
    latest: Arc<Mutex<Option<String>>>,
    clients: Arc<Mutex<Vec<TcpStream>>>,
) {
    loop {
        // Accept incoming WebSocket connections
        match listener.accept() {
            Ok((mut stream, _)) => {
                let _ = stream.set_nonblocking(false);
                let _ = stream.set_read_timeout(Some(Duration::from_millis(150)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(1)));

                if let Some(_) = do_handshake(&mut stream) {
                    if let Some(payload) = latest.lock().unwrap().clone() {
                        let _ = send_frame(&mut stream, &payload);
                    }
                    let _ = stream.set_nonblocking(true);
                    clients.lock().unwrap().push(stream);
                }
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(40));
            }
            Err(_) => {
                thread::sleep(Duration::from_millis(100));
            }
        }

        // Poll existing clients for messages (commands)
        poll_clients(&clients, &event_tx, &latest);
    }
}

fn do_handshake(stream: &mut TcpStream) -> Option<String> {
    let mut buf = [0u8; 2048];
    let n = stream.read(&mut buf).ok()?;
    let req = std::str::from_utf8(&buf[..n]).ok()?;

    let mut key = None;
    for line in req.lines() {
        if let Some((header, val)) = line.split_once(':') {
            if header.trim().eq_ignore_ascii_case("Sec-WebSocket-Key") {
                key = Some(val.trim().to_string());
                break;
            }
        }
    }
    let key = key?;
    let concat = format!("{key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11");
    let hash = sha1_smol::Sha1::from(concat).digest().bytes();
    let accept = STANDARD.encode(hash);

    let resp = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(resp.as_bytes()).ok()?;
    stream.flush().ok()?;
    Some(key)
}

fn send_frame(stream: &mut TcpStream, text: &str) -> std::io::Result<()> {
    let payload = text.as_bytes();
    let len = payload.len();
    let mut header = Vec::with_capacity(10);
    header.push(0x81); // FIN + Text opcode

    if len < 126 {
        header.push(len as u8);
    } else if len <= 0xFFFF {
        header.push(126);
        header.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        header.push(127);
        header.extend_from_slice(&(len as u64).to_be_bytes());
    }

    stream.write_all(&header)?;
    stream.write_all(payload)?;
    stream.flush()
}

fn poll_clients(
    clients: &Arc<Mutex<Vec<TcpStream>>>,
    event_tx: &Sender<BridgeEvent>,
    latest: &Arc<Mutex<Option<String>>>,
) {
    let mut guard = clients.lock().unwrap();
    guard.retain_mut(|stream| {
        loop {
            match read_client_frame(stream) {
                Ok(Some(msg)) => {
                    handle_incoming_message(&msg, event_tx, stream, latest);
                }
                Ok(None) => break,
                Err(_) => return false,
            }
        }
        true
    });
}

fn read_client_frame(stream: &mut TcpStream) -> std::io::Result<Option<String>> {
    let mut hdr = [0u8; 2];
    match stream.read_exact(&mut hdr) {
        Ok(()) => {}
        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
        Err(e) => return Err(e),
    }

    let opcode = hdr[0] & 0x0F;
    if opcode == 0x8 {
        return Err(std::io::Error::new(std::io::ErrorKind::ConnectionAborted, "closed"));
    }

    let is_masked = (hdr[1] & 0x80) != 0;
    let mut payload_len = (hdr[1] & 0x7F) as usize;

    if payload_len == 126 {
        let mut ext = [0u8; 2];
        stream.read_exact(&mut ext)?;
        payload_len = u16::from_be_bytes(ext) as usize;
    } else if payload_len == 127 {
        let mut ext = [0u8; 8];
        stream.read_exact(&mut ext)?;
        payload_len = u64::from_be_bytes(ext) as usize;
    }

    let mask = if is_masked {
        let mut m = [0u8; 4];
        stream.read_exact(&mut m)?;
        Some(m)
    } else {
        None
    };

    let mut payload = vec![0u8; payload_len];
    stream.read_exact(&mut payload)?;

    if let Some(m) = mask {
        for (i, b) in payload.iter_mut().enumerate() {
            *b ^= m[i % 4];
        }
    }

    if opcode == 0x9 {
        let pong = [0x8A, 0x00];
        let _ = stream.write_all(&pong);
        let _ = stream.flush();
        return Ok(None);
    }

    String::from_utf8(payload)
        .map(Some)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

fn handle_incoming_message(
    msg: &str,
    event_tx: &Sender<BridgeEvent>,
    stream: &mut TcpStream,
    latest: &Arc<Mutex<Option<String>>>,
) {
    let text = msg.trim();
    if text.is_empty() {
        return;
    }

    let cmd_str = if let Ok(val) = serde_json::from_str::<Value>(text) {
        if let Some(cmd) = val["command"].as_str() {
            if cmd == "seek" || cmd == "seekTo" {
                if let Some(secs) = val["value"].as_f64() {
                    let _ = event_tx.send(BridgeEvent::Seek(Duration::from_secs_f64(secs)));
                    return;
                }
            }
            cmd.to_string()
        } else {
            return;
        }
    } else {
        text.to_string()
    };

    match cmd_str.as_str() {
        "playPause" | "toggle" => {
            let _ = event_tx.send(BridgeEvent::PlayPause);
        }
        "next" => {
            let _ = event_tx.send(BridgeEvent::Next);
        }
        "prev" | "previous" => {
            let _ = event_tx.send(BridgeEvent::Previous);
        }
        "toggleMusicWindow" => {
            let _ = event_tx.send(BridgeEvent::ToggleWindow);
        }
        "getTrack" | "getMetadata" | "status" => {
            if let Some(payload) = latest.lock().unwrap().clone() {
                let _ = send_frame(stream, &payload);
            }
        }
        _ => {}
    }
}

fn broadcast(clients: &Arc<Mutex<Vec<TcpStream>>>, payload: &str) {
    let mut guard = clients.lock().unwrap();
    guard.retain_mut(|stream| send_frame(stream, payload).is_ok());
}
