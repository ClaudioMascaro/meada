//! The loopback decider stub (spec #18): an HTTP server on `127.0.0.1` that speaks both decider
//! protocols, answers from a script each test provides and records every request it receives.
//! Tests point `MEADA_DECIDER_URL` at `Stub::url`, so decider mode runs with no network and no
//! real key, and a test can prove that a command sent nothing.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

/// A request as the stub received it.
#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    /// The request target, such as `/v1/chat/completions`.
    pub path: String,
    /// Header names in lower case, in the order received.
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        (self.headers.iter())
            .find(|(n, _)| *n == name)
            .map(|(_, v)| v.as_str())
    }

    pub fn json(&self) -> serde_json::Value {
        serde_json::from_str(&self.body).expect("the request body is JSON")
    }
}

/// One scripted answer.
pub struct Reply {
    pub status: u16,
    pub body: String,
}

impl Reply {
    pub fn ok(body: serde_json::Value) -> Self {
        Self {
            status: 200,
            body: body.to_string(),
        }
    }

    pub fn status(status: u16, body: &str) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    /// A `chat` answer whose message content is `content`, from `model`.
    pub fn chat(model: &str, content: serde_json::Value) -> Self {
        Self::ok(serde_json::json!({
            "id": "stub",
            "object": "chat.completion",
            "model": model,
            "choices": [{
                "index": 0,
                "finish_reason": "stop",
                "message": {"role": "assistant", "content": content.to_string()},
            }],
        }))
    }
}

/// The paths of the two protocols: `chat` (`<url>/chat/completions`) and `decisions`
/// (`<url>/api/alpha/decisions`). Any other path gets a 404 and uses no script entry.
const PATHS: [&str; 2] = ["/chat/completions", "/api/alpha/decisions"];

pub struct Stub {
    /// `http://127.0.0.1:<port>`, the base URL to set as `MEADA_DECIDER_URL`.
    pub url: String,
    received: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    /// Starts a stub that answers the n-th protocol request with `script[n]`, and with a 500
    /// once the script has run out. The server lives until the test process ends.
    pub fn start(script: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
        let url = format!("http://{}", listener.local_addr().unwrap());
        let received = Arc::new(Mutex::new(Vec::new()));
        let log = Arc::clone(&received);
        thread::spawn(move || {
            let mut script = script.into_iter();
            for stream in listener.incoming() {
                let Ok(stream) = stream else { continue };
                let Some(request) = read(&stream) else {
                    continue;
                };
                let reply = if PATHS.iter().any(|p| request.path.ends_with(p)) {
                    (script.next()).unwrap_or_else(|| Reply::status(500, "the script ran out"))
                } else {
                    Reply::status(404, "not a decider path")
                };
                // Recorded before answering, so a client that got its answer finds it here.
                log.lock().unwrap().push(request);
                write(stream, &reply);
            }
        });
        Self { url, received }
    }

    /// Every request received so far, in order.
    pub fn requests(&self) -> Vec<Request> {
        self.received.lock().unwrap().clone()
    }
}

fn read(stream: &TcpStream) -> Option<Request> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let mut parts = line.split_whitespace();
    let (method, path) = (parts.next()?.to_string(), parts.next()?.to_string());
    let mut headers = Vec::new();
    loop {
        line.clear();
        reader.read_line(&mut line).ok()?;
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        let (name, value) = header.split_once(':')?;
        headers.push((name.trim().to_ascii_lowercase(), value.trim().to_string()));
    }
    let length = (headers.iter())
        .find(|(n, _)| n == "content-length")
        .map_or(0, |(_, v)| v.parse().unwrap_or(0));
    let mut body = vec![0; length];
    reader.read_exact(&mut body).ok()?;
    Some(Request {
        method,
        path,
        headers,
        body: String::from_utf8_lossy(&body).into_owned(),
    })
}

fn write(mut stream: TcpStream, reply: &Reply) {
    let _ = write!(
        stream,
        "HTTP/1.1 {} Stub\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Connection: close\r\n\r\n{}",
        reply.status,
        reply.body.len(),
        reply.body
    );
}
