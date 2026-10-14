//! Discord's local IPC protocol. No bot token, OAuth, or remote API is needed.
//! https://docs.discord.com/developers/topics/rpc
use crate::presence::{Snapshot, Transport};
use anyhow::{Context, Result, bail, ensure};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

const MAX_FRAME: usize = 64 * 1024;
const TIMEOUT: Duration = Duration::from_secs(2);
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

#[cfg(windows)]
type Socket = std::fs::File;
#[cfg(unix)]
type Socket = std::os::unix::net::UnixStream;

pub struct DiscordTransport {
    app_id: String,
    socket: Option<Socket>,
    sequence: u64,
}

impl DiscordTransport {
    pub fn new(app_id: &str) -> Self {
        Self {
            app_id: app_id.into(),
            socket: None,
            sequence: 0,
        }
    }

    fn send(&mut self, opcode: u32, payload: &Value) -> Result<()> {
        let socket = self.socket.as_mut().context("Discord is disconnected")?;
        write_frame(socket, opcode, payload)
    }

    fn receive(&mut self, deadline: Instant) -> Result<(u32, Value)> {
        let socket = self.socket.as_mut().context("Discord is disconnected")?;
        let mut header = [0; 8];
        timed_read(socket, &mut header, deadline)?;
        let opcode = u32::from_le_bytes(header[..4].try_into().unwrap());
        let length = checked_length(&header)?;
        let mut payload = vec![0; length];
        timed_read(socket, &mut payload, deadline)?;
        Ok((
            opcode,
            serde_json::from_slice(&payload).context("Invalid Discord response")?,
        ))
    }

    fn await_response(&mut self, nonce: Option<&str>) -> Result<()> {
        let deadline = Instant::now()
            + if nonce.is_none() {
                HANDSHAKE_TIMEOUT
            } else {
                TIMEOUT
            };
        loop {
            ensure!(Instant::now() < deadline, "Discord response timed out");
            let (opcode, payload) = self.receive(deadline)?;
            match opcode {
                2 => bail!("Discord closed the connection"),
                3 => {
                    self.send(4, &payload)?;
                    continue;
                }
                1 => {}
                _ => continue,
            }
            if payload["evt"] == "ERROR" {
                bail!(
                    "Discord rejected the request (code {}): {}",
                    payload["data"]["code"],
                    payload["data"]["message"]
                        .as_str()
                        .unwrap_or("unknown error")
                );
            }
            if nonce.is_none() && payload["evt"] == "READY" {
                return Ok(());
            }
            if nonce.is_some_and(|nonce| payload["nonce"].as_str() == Some(nonce)) {
                ensure!(
                    payload["cmd"] == "SET_ACTIVITY",
                    "Unexpected Discord command response"
                );
                return Ok(());
            }
        }
    }

    fn set_activity(&mut self, activity: Value) -> Result<()> {
        self.sequence += 1;
        let nonce = format!("scrp-{}-{}", std::process::id(), self.sequence);
        self.send(1, &json!({"cmd":"SET_ACTIVITY", "args":{"pid":std::process::id(), "activity":activity}, "nonce":nonce}))?;
        self.await_response(Some(&nonce))
    }
}

impl Transport for DiscordTransport {
    fn connect(&mut self) -> Result<()> {
        self.socket = Some(connect_socket()?);
        self.send(0, &json!({"v":1,"client_id":self.app_id}))?;
        self.await_response(None)
    }
    fn publish(&mut self, snapshot: &Snapshot) -> Result<()> {
        self.set_activity(activity_payload(snapshot))
    }
    fn clear(&mut self) -> Result<()> {
        self.set_activity(Value::Null)
    }
    fn close(&mut self) {
        self.socket = None;
    }
}

fn activity_payload(s: &Snapshot) -> Value {
    let mut assets = json!({"large_image":s.large_image,"large_text":s.large_text});
    if s.large_image != "logo" {
        assets["small_image"] = json!("logo");
        assets["small_text"] = json!("Star Citizen");
    }
    let mut payload = json!({"type":0,"details":s.details,"state":s.state,"assets":assets,"timestamps":{"start":s.started}});
    if let Some(url) = &s.profile_url {
        payload["buttons"] = json!([{"label":"RSI Profile","url":url}]);
    }
    payload
}

fn write_frame(writer: &mut impl Write, opcode: u32, payload: &Value) -> Result<()> {
    let body = serde_json::to_vec(payload)?;
    ensure!(body.len() <= MAX_FRAME, "Discord payload is too large");
    let mut frame = Vec::with_capacity(body.len() + 8);
    frame.extend(opcode.to_le_bytes());
    frame.extend((body.len() as u32).to_le_bytes());
    frame.extend(body);
    writer.write_all(&frame).context("Cannot write to Discord")
}

fn checked_length(header: &[u8; 8]) -> Result<usize> {
    let length = u32::from_le_bytes(header[4..].try_into().unwrap()) as usize;
    ensure!(length <= MAX_FRAME, "Discord frame exceeds the size limit");
    Ok(length)
}

#[cfg(windows)]
fn connect_socket() -> Result<Socket> {
    for index in 0..10 {
        if let Ok(file) = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!(r"\\?\pipe\discord-ipc-{index}"))
        {
            return Ok(file);
        }
    }
    bail!("Discord desktop is not running, or its IPC pipe is inaccessible")
}

#[cfg(windows)]
fn timed_read(socket: &mut Socket, mut buffer: &mut [u8], deadline: Instant) -> Result<()> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::System::Pipes::PeekNamedPipe;
    while !buffer.is_empty() {
        ensure!(Instant::now() < deadline, "Discord response timed out");
        let mut available = 0;
        // SAFETY: The file owns a valid pipe handle throughout this call. All optional
        // output pointers are null; `available` is a live writable u32.
        let ok = unsafe {
            PeekNamedPipe(
                socket.as_raw_handle(),
                std::ptr::null_mut(),
                0,
                std::ptr::null_mut(),
                &mut available,
                std::ptr::null_mut(),
            )
        };
        if ok == 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        if available == 0 {
            std::thread::sleep(Duration::from_millis(10));
            continue;
        }
        let length = buffer.len().min(available as usize);
        let read = socket.read(&mut buffer[..length])?;
        ensure!(read != 0, "Discord pipe closed");
        buffer = &mut buffer[read..];
    }
    Ok(())
}

#[cfg(unix)]
fn connect_socket() -> Result<Socket> {
    let mut roots: Vec<std::path::PathBuf> = ["XDG_RUNTIME_DIR", "TMPDIR", "TMP", "TEMP"]
        .iter()
        .filter_map(std::env::var_os)
        .map(Into::into)
        .collect();
    roots.push("/tmp".into());
    for root in roots {
        for index in 0..10 {
            if let Ok(socket) = Socket::connect(root.join(format!("discord-ipc-{index}"))) {
                socket.set_write_timeout(Some(TIMEOUT))?;
                return Ok(socket);
            }
        }
    }
    bail!("Discord desktop IPC socket not found")
}

#[cfg(unix)]
fn timed_read(socket: &mut Socket, buffer: &mut [u8], deadline: Instant) -> Result<()> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .context("Discord response timed out")?;
    socket.set_read_timeout(Some(remaining))?;
    socket
        .read_exact(buffer)
        .context("Cannot read from Discord")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    fn mock_pipe(
        server: impl FnOnce(Socket) + Send + 'static,
    ) -> (DiscordTransport, std::thread::JoinHandle<()>) {
        use std::os::windows::io::FromRawHandle;
        use windows_sys::Win32::{
            Foundation::INVALID_HANDLE_VALUE,
            Storage::FileSystem::PIPE_ACCESS_DUPLEX,
            System::Pipes::{ConnectNamedPipe, CreateNamedPipeW},
        };
        static INDEX: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let index = INDEX.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let name = format!(r"\\?\pipe\scrp-test-{}-{index}", std::process::id());
        let wide: Vec<u16> = name.encode_utf16().chain(Some(0)).collect();
        // SAFETY: Null-terminated name, no security/overlapped pointer, one byte-mode instance.
        let handle = unsafe {
            CreateNamedPipeW(
                wide.as_ptr(),
                PIPE_ACCESS_DUPLEX,
                0,
                1,
                65536,
                65536,
                0,
                std::ptr::null(),
            )
        };
        assert_ne!(handle, INVALID_HANDLE_VALUE);
        // SAFETY: This transfers the exclusively owned, valid handle into File for RAII cleanup.
        let file = unsafe { Socket::from_raw_handle(handle) };
        let thread = std::thread::spawn(move || {
            use std::os::windows::io::AsRawHandle;
            // SAFETY: File keeps the handle alive; the pipe was created in synchronous mode.
            let connected = unsafe { ConnectNamedPipe(file.as_raw_handle(), std::ptr::null_mut()) };
            // ERROR_PIPE_CONNECTED is expected if the client opened first.
            assert!(connected != 0 || std::io::Error::last_os_error().raw_os_error() == Some(535));
            server(file);
        });
        let socket = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(name)
            .unwrap();
        (
            DiscordTransport {
                app_id: "123456789012345678".into(),
                socket: Some(socket),
                sequence: 0,
            },
            thread,
        )
    }

    #[cfg(windows)]
    fn server_read(file: &mut Socket) -> (u32, Value) {
        let mut header = [0; 8];
        file.read_exact(&mut header).unwrap();
        let mut body = vec![0; checked_length(&header).unwrap()];
        file.read_exact(&mut body).unwrap();
        (
            u32::from_le_bytes(header[..4].try_into().unwrap()),
            serde_json::from_slice(&body).unwrap(),
        )
    }

    #[cfg(windows)]
    #[test]
    fn real_named_pipe_handles_ping_handshake_activity_ack_and_clear() {
        let (mut client, server) = mock_pipe(|mut file| {
            let (opcode, handshake) = server_read(&mut file);
            assert_eq!(opcode, 0);
            assert_eq!(handshake["v"], 1);
            write_frame(&mut file, 3, &json!({"ping":42})).unwrap();
            let (opcode, pong) = server_read(&mut file);
            assert_eq!(opcode, 4);
            assert_eq!(pong["ping"], 42);
            write_frame(&mut file, 1, &json!({"evt":"READY"})).unwrap();
            for cleared in [false, true] {
                let (opcode, request) = server_read(&mut file);
                assert_eq!(opcode, 1);
                assert_eq!(request["cmd"], "SET_ACTIVITY");
                assert_eq!(request["args"]["activity"].is_null(), cleared);
                // Ignore unrelated dispatch frames until the matching command nonce arrives.
                write_frame(&mut file, 1, &json!({"evt":"UNRELATED"})).unwrap();
                write_frame(
                    &mut file,
                    1,
                    &json!({"cmd":"SET_ACTIVITY","nonce":request["nonce"],"data":{}}),
                )
                .unwrap();
            }
        });
        client
            .send(0, &json!({"v":1,"client_id":client.app_id}))
            .unwrap();
        client.await_response(None).unwrap();
        let snapshot = Snapshot {
            details: "Playing".into(),
            state: "LIVE".into(),
            large_image: "logo".into(),
            large_text: "Star Citizen".into(),
            profile_url: None,
            started: 123,
        };
        client.publish(&snapshot).unwrap();
        client.clear().unwrap();
        client.close();
        server.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn real_named_pipe_rejection_is_reported() {
        let (mut client, server) = mock_pipe(|mut file| {
            let (_, request) = server_read(&mut file);
            write_frame(&mut file, 1, &json!({"evt":"ERROR","nonce":request["nonce"],"data":{"code":4000,"message":"Invalid application"}})).unwrap();
        });
        let error = client.set_activity(Value::Null).unwrap_err();
        assert!(error.to_string().contains("Invalid application"));
        client.close();
        server.join().unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn silent_named_pipe_respects_the_read_deadline() {
        let (mut client, server) = mock_pipe(|_file| {
            std::thread::sleep(Duration::from_millis(100));
        });
        let started = Instant::now();
        let error = client
            .receive(started + Duration::from_millis(30))
            .unwrap_err();
        assert!(error.to_string().contains("timed out"));
        assert!(started.elapsed() < Duration::from_secs(1));
        client.close();
        server.join().unwrap();
    }
    #[test]
    fn frame_codec_uses_little_endian_and_rejects_unbounded_allocations() {
        let payload = json!({"v":1,"client_id":"123456789012345678"});
        let mut bytes = Vec::new();
        write_frame(&mut bytes, 0, &payload).unwrap();
        assert_eq!(&bytes[..4], &[0, 0, 0, 0]);
        let header: [u8; 8] = bytes[..8].try_into().unwrap();
        assert_eq!(checked_length(&header).unwrap(), bytes.len() - 8);
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes[8..]).unwrap(),
            payload
        );
        let mut bad = [0; 8];
        bad[4..].copy_from_slice(&u32::MAX.to_le_bytes());
        assert!(checked_length(&bad).is_err());
    }
    #[test]
    fn activity_optional_fields_and_clear_payload_are_correct() {
        let snapshot = Snapshot {
            details: "Playing".into(),
            state: "LIVE".into(),
            large_image: "logo".into(),
            large_text: "Star Citizen".into(),
            profile_url: None,
            started: 123,
        };
        let activity = activity_payload(&snapshot);
        assert_eq!(activity["timestamps"]["start"], 123);
        assert!(activity.get("buttons").is_none());
        let mut bytes = Vec::new();
        write_frame(&mut bytes, 1, &json!({"args":{"activity":null}})).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes[8..]).unwrap()["args"]["activity"],
            Value::Null
        );
    }
}
