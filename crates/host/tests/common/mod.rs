//! What the tests of `floti update` and `floti add` share: an HTTP server on
//! a loopback port serving a release, and a minisign key made for the test
//! to sign it with.

#![allow(dead_code)]

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Cursor, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

#[derive(Clone)]
pub enum Served {
    Body(Vec<u8>),
    /// Announces the whole body, sends the first bytes, and closes.
    CutAfter(Vec<u8>, usize),
}

/// An HTTP/1.1 server on a loopback port, serving fixed bodies by path, one
/// connection at a time, until it is dropped.
pub struct Server {
    address: SocketAddr,
    served: Arc<Mutex<HashMap<String, Served>>>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl Server {
    pub fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind a loopback port");
        let address = listener.local_addr().expect("its address");
        let served: Arc<Mutex<HashMap<String, Served>>> = Arc::default();
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let served = Arc::clone(&served);
            let stop = Arc::clone(&stop);
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    if let Ok(stream) = stream {
                        answer(stream, &served);
                    }
                }
            })
        };
        Self {
            address,
            served,
            stop,
            thread: Some(thread),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.address)
    }

    pub fn serve(&self, path: &str, served: Served) {
        self.served.lock().unwrap().insert(path.to_owned(), served);
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // Wakes the accept so the loop sees the stop.
        let _ = TcpStream::connect(self.address);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn answer(stream: TcpStream, served: &Mutex<HashMap<String, Served>>) {
    let mut reader = BufReader::new(stream);
    let mut request = String::new();
    if reader.read_line(&mut request).is_err() {
        return;
    }
    loop {
        let mut header = String::new();
        match reader.read_line(&mut header) {
            Ok(0) | Err(_) => return,
            Ok(_) if header == "\r\n" => break,
            Ok(_) => {}
        }
    }
    let path = request.split(' ').nth(1).unwrap_or_default().to_owned();
    let found = served.lock().unwrap().get(&path).cloned();
    let mut stream = reader.into_inner();
    let (status, body, sent) = match found {
        None => ("404 Not Found", Vec::new(), 0),
        Some(Served::Body(body)) => {
            let length = body.len();
            ("200 OK", body, length)
        }
        Some(Served::CutAfter(body, kept)) => ("200 OK", body, kept),
    };
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(&body[..sent]);
    let _ = stream.flush();
    let _ = stream.shutdown(std::net::Shutdown::Both);
}

/// A minisign key made for the test.
pub struct Signer {
    pair: minisign::KeyPair,
}

impl Signer {
    pub fn new() -> Self {
        Self {
            pair: minisign::KeyPair::generate_unencrypted_keypair().expect("make a key pair"),
        }
    }

    pub fn public_text(&self) -> String {
        self.pair.pk.to_box().expect("box the key").into_string()
    }

    pub fn sign(&self, bytes: &[u8], comment: &str) -> Vec<u8> {
        minisign::sign(
            Some(&self.pair.pk),
            &self.pair.sk,
            Cursor::new(bytes),
            Some(comment),
            None,
        )
        .expect("sign")
        .into_string()
        .into_bytes()
    }
}
