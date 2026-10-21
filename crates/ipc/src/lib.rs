//! Local control channel between a netusb service and its programs.
//!
//! One JSON object per line. A request is `{"id":1,"method":"state","params":{}}` and the reply is
//! `{"id":1,"result":...}` or `{"id":1,"error":"..."}`. The transport is a Unix socket on Unix and
//! a named pipe on Windows, so Qt's `QLocalSocket` speaks it on both.

use std::future::Future;
use std::sync::Arc;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader};

#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(unix)]
use unix as transport;
#[cfg(windows)]
use windows as transport;

const MAX_LINE: usize = 1 << 20;

/// The two services a program can talk to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Service {
    Server,
    Client,
}

impl Service {
    pub fn name(self) -> &'static str {
        match self {
            Service::Server => "server",
            Service::Client => "client",
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Reply {
    pub id: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Decode `params` into the type a method expects.
pub fn params<T: serde::de::DeserializeOwned>(req: &Request) -> Result<T, String> {
    serde_json::from_value(req.params.clone()).map_err(|e| format!("bad parameters for {}: {e}", req.method))
}

type Handler<F> = Arc<F>;

/// Answer requests on the service's endpoint until the process ends.
pub async fn serve<F, Fut>(service: Service, handler: F) -> Result<()>
where
    F: Fn(Request) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<Value, String>> + Send + 'static,
{
    let handler: Handler<F> = Arc::new(handler);
    let mut listener = transport::Listener::bind(service).context("creating the control endpoint")?;
    loop {
        let stream = match listener.accept().await {
            Ok(Some(s)) => s,
            Ok(None) => continue,
            Err(e) => {
                eprintln!("control endpoint: {e:#}");
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
                continue;
            }
        };
        let handler = handler.clone();
        tokio::spawn(async move {
            if let Err(e) = connection(stream, handler).await {
                eprintln!("control connection: {e:#}");
            }
        });
    }
}

async fn connection<S, F, Fut>(stream: S, handler: Handler<F>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
    F: Fn(Request) -> Fut,
    Fut: Future<Output = Result<Value, String>>,
{
    let (r, mut w) = tokio::io::split(stream);
    let mut r = BufReader::new(r).take(MAX_LINE as u64);
    let mut line = String::new();
    loop {
        line.clear();
        r.set_limit(MAX_LINE as u64);
        if r.read_line(&mut line).await? == 0 {
            return Ok(());
        }
        if !line.ends_with('\n') {
            bail!("request longer than {MAX_LINE} bytes");
        }
        let reply = match serde_json::from_str::<Request>(&line) {
            Ok(req) => {
                let id = req.id;
                match handler(req).await {
                    Ok(result) => Reply { id, result: Some(result), error: None },
                    Err(error) => Reply { id, result: None, error: Some(error) },
                }
            }
            Err(e) => Reply { id: 0, result: None, error: Some(format!("malformed request: {e}")) },
        };
        let mut out = serde_json::to_vec(&reply)?;
        out.push(b'\n');
        w.write_all(&out).await?;
    }
}

/// A connection to a running service.
pub struct Client {
    reader: BufReader<Box<dyn AsyncRead + Unpin + Send>>,
    writer: Box<dyn AsyncWrite + Unpin + Send>,
    next_id: u64,
}

impl Client {
    pub async fn connect(service: Service) -> Result<Client> {
        let (r, w) = transport::connect(service).await?;
        Ok(Client { reader: BufReader::new(r), writer: w, next_id: 1 })
    }

    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        let mut out = serde_json::to_vec(&Request { id, method: method.into(), params })?;
        out.push(b'\n');
        self.writer.write_all(&out).await?;
        let mut line = String::new();
        if self.reader.read_line(&mut line).await? == 0 {
            bail!("the service closed the connection");
        }
        let reply: Reply = serde_json::from_str(&line).context("unreadable reply from the service")?;
        match reply.error {
            Some(e) => bail!("{e}"),
            None => Ok(reply.result.unwrap_or(Value::Null)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn round_trip_and_errors() {
        let dir = std::env::temp_dir().join(format!("netusb-ipc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::env::set_var("NETUSB_IPC_DIR", &dir);
        tokio::spawn(serve(Service::Client, |req| async move {
            match req.method.as_str() {
                "echo" => Ok(req.params),
                _ => Err(format!("unknown method {}", req.method)),
            }
        }));
        let mut c = loop {
            match Client::connect(Service::Client).await {
                Ok(c) => break c,
                Err(_) => tokio::time::sleep(std::time::Duration::from_millis(20)).await,
            }
        };
        assert_eq!(c.call("echo", serde_json::json!({"a": 1})).await.unwrap()["a"], 1);
        assert!(c.call("nope", Value::Null).await.unwrap_err().to_string().contains("unknown method"));
        assert_eq!(c.call("echo", serde_json::json!("again")).await.unwrap(), "again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
