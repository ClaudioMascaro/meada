use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    pub path: String,
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

const PATHS: [&str; 2] = ["/chat/completions", "/api/alpha/decisions"];

pub struct Stub {
    pub url: String,
    received: Arc<Mutex<Vec<Request>>>,
}

impl Stub {
    pub fn start(script: Vec<Reply>) -> Self {
        Self::observed(script, |_| {})
    }

    pub fn observed(script: Vec<Reply>, mut observe: impl FnMut(usize) + Send + 'static) -> Self {
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
                let count = {
                    let mut log = log.lock().unwrap();
                    log.push(request);
                    log.len()
                };
                observe(count);
                write(stream, &reply);
            }
        });
        Self { url, received }
    }

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
