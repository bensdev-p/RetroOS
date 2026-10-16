//! Client for the greetd IPC protocol (length-prefixed JSON on $GREETD_SOCK).

use serde_json::{json, Value};
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;

pub struct Greetd {
    s: UnixStream,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Reply {
    Success,
    /// (secret?, prompt)
    Prompt(bool, String),
    /// Informational or error text from PAM.
    Message(String),
    Error(String),
}

impl Greetd {
    pub fn connect() -> io::Result<Greetd> {
        let path = std::env::var("GREETD_SOCK")
            .map_err(|_| io::Error::new(io::ErrorKind::NotFound, "GREETD_SOCK not set"))?;
        Ok(Greetd {
            s: UnixStream::connect(path)?,
        })
    }

    fn call(&mut self, v: Value) -> io::Result<Reply> {
        let body = v.to_string();
        self.s.write_all(&(body.len() as u32).to_ne_bytes())?;
        self.s.write_all(body.as_bytes())?;
        let mut len = [0u8; 4];
        self.s.read_exact(&mut len)?;
        let mut buf = vec![0u8; u32::from_ne_bytes(len) as usize];
        self.s.read_exact(&mut buf)?;
        let r: Value = serde_json::from_slice(&buf).map_err(io::Error::other)?;
        Ok(parse(&r))
    }

    pub fn create_session(&mut self, user: &str) -> io::Result<Reply> {
        self.call(json!({"type": "create_session", "username": user}))
    }

    pub fn respond(&mut self, response: Option<&str>) -> io::Result<Reply> {
        self.call(json!({"type": "post_auth_message_response", "response": response}))
    }

    pub fn start_session(&mut self, cmd: &[&str], env: &[String]) -> io::Result<Reply> {
        self.call(json!({"type": "start_session", "cmd": cmd, "env": env}))
    }

    pub fn cancel(&mut self) -> io::Result<Reply> {
        self.call(json!({"type": "cancel_session"}))
    }
}

pub fn parse(r: &Value) -> Reply {
    match r["type"].as_str() {
        Some("success") => Reply::Success,
        Some("auth_message") => {
            let msg = r["auth_message"].as_str().unwrap_or("").to_string();
            match r["auth_message_type"].as_str() {
                Some("secret") => Reply::Prompt(true, msg),
                Some("visible") => Reply::Prompt(false, msg),
                _ => Reply::Message(msg),
            }
        }
        _ => Reply::Error(
            r["description"]
                .as_str()
                .unwrap_or("login failed")
                .to_string(),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_replies() {
        assert_eq!(parse(&json!({"type":"success"})), Reply::Success);
        assert_eq!(
            parse(
                &json!({"type":"auth_message","auth_message_type":"secret","auth_message":"Password: "})
            ),
            Reply::Prompt(true, "Password: ".into())
        );
        assert_eq!(
            parse(&json!({"type":"error","error_type":"auth_error","description":"bad"})),
            Reply::Error("bad".into())
        );
    }
}
