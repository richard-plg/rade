//! Speech-to-text (R124, D5, D6): the WAV for a take, the `curl` command and
//! its config (the URL, the form and the API key go on a pipe, never argv),
//! the reply, the level of a PCM chunk and the error texts.

use crate::config::VoiceConfig;

pub const SAMPLE_RATE: u32 = 16_000;
/// Words at the end of a live partial that may still change.
const TENTATIVE_WORDS: usize = 3;

/// A 16 kHz mono s16le WAV for `pcm`, built in memory.
pub fn wav(pcm: &[i16]) -> Vec<u8> {
    let data = (pcm.len() * 2) as u32;
    let mut v = Vec::with_capacity(44 + data as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&1u16.to_le_bytes()); // PCM
    v.extend_from_slice(&1u16.to_le_bytes()); // mono
    v.extend_from_slice(&SAMPLE_RATE.to_le_bytes());
    v.extend_from_slice(&(SAMPLE_RATE * 2).to_le_bytes());
    v.extend_from_slice(&2u16.to_le_bytes()); // block align
    v.extend_from_slice(&16u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data.to_le_bytes());
    for s in pcm {
        v.extend_from_slice(&s.to_le_bytes());
    }
    v
}

/// The meter's level of a chunk: RMS / 32768, square-rooted so quiet speech
/// still moves the bars, 0..1.
pub fn level(chunk: &[i16]) -> f32 {
    if chunk.is_empty() {
        return 0.0;
    }
    let sum: f64 = chunk.iter().map(|&s| f64::from(s) * f64::from(s)).sum();
    let rms = (sum / chunk.len() as f64).sqrt() / 32768.0;
    (rms.sqrt() as f32).clamp(0.0, 1.0)
}

/// The curl command; the WAV goes on stdin, the rest on fd 3 ([`curl_config`]).
pub fn curl_argv() -> Vec<String> {
    [
        "curl",
        "-sS",
        "--max-time",
        "30",
        "-K",
        "/dev/fd/3",
        "-F",
        "file=@-;filename=take.wav;type=audio/wav",
    ]
    .map(String::from)
    .to_vec()
}

/// A value inside a curl config double-quoted string.
fn quote(s: &str) -> String {
    let mut o = String::from("\"");
    for c in s.chars() {
        match c {
            '\\' => o.push_str("\\\\"),
            '"' => o.push_str("\\\""),
            '\n' => o.push_str("\\n"),
            '\r' => o.push_str("\\r"),
            '\t' => o.push_str("\\t"),
            c => o.push(c),
        }
    }
    o.push('"');
    o
}

/// What goes to `curl -K` on fd 3. `key` is the output of `stt_key_command`;
/// `write-out` appends the HTTP status after the body for [`parse`].
pub fn curl_config(cfg: &VoiceConfig, key: Option<&str>, vocabulary: &str) -> String {
    let mut s = format!("url = {}\n", quote(&cfg.stt_url));
    if let Some(k) = key.map(str::trim).filter(|k| !k.is_empty()) {
        s.push_str(&format!(
            "header = {}\n",
            quote(&format!("Authorization: Bearer {k}"))
        ));
    }
    s.push_str(&format!(
        "form = {}\n",
        quote(&format!("model={}", cfg.stt_model))
    ));
    s.push_str("form = \"response_format=json\"\n");
    let lang = cfg.stt_language();
    if lang != "auto" {
        s.push_str(&format!("form = {}\n", quote(&format!("language={lang}"))));
    }
    if !vocabulary.trim().is_empty() {
        s.push_str(&format!(
            "form-string = {}\n",
            quote(&format!("prompt={vocabulary}"))
        ));
    }
    s.push_str("write-out = \"\\n%{http_code}\"\n");
    s
}

/// The reply: `text`, trimmed. The body may be followed by a line with the
/// HTTP status (`write-out`); a status of 400 or more, or a body without
/// `text`, is `Speech-to-text failed: <status> <first line of body>`.
pub fn parse(output: &str) -> Result<String, String> {
    let (body, status) = match output.rsplit_once('\n') {
        Some((b, s)) if s.len() == 3 && s.bytes().all(|c| c.is_ascii_digit()) => {
            (b, s.parse::<u16>().ok())
        }
        _ => (output, None),
    };
    let first = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let json: Option<serde_json::Value> = serde_json::from_str(body).ok();
    let failed = |detail: &str| {
        let s = status.map_or(String::new(), |s| format!("{s} "));
        format!("Speech-to-text failed: {s}{detail}")
            .trim_end()
            .to_string()
    };
    if status.is_some_and(|s| s >= 400) {
        let msg = json
            .as_ref()
            .and_then(|j| {
                j["error"]["message"]
                    .as_str()
                    .or_else(|| j["error"].as_str())
            })
            .unwrap_or(first);
        return Err(failed(msg));
    }
    match json.as_ref().and_then(|j| j["text"].as_str()) {
        Some(t) => Ok(t.trim().to_string()),
        None => Err(failed(first)),
    }
}

/// `(stable, tentative)`: the last three words of a live partial may still
/// change.
pub fn split_tentative(text: &str) -> (String, String) {
    let t = text.trim();
    let starts: Vec<usize> = t
        .char_indices()
        .filter(|&(i, c)| !c.is_whitespace() && (i == 0 || t[..i].ends_with(char::is_whitespace)))
        .map(|(i, _)| i)
        .collect();
    if starts.len() <= TENTATIVE_WORDS {
        return (String::new(), t.to_string());
    }
    let at = starts[starts.len() - TENTATIVE_WORDS];
    (t[..at].trim_end().to_string(), t[at..].to_string())
}

/// `host:port` of a URL (the scheme's default port when it has none).
pub fn host_port(url: &str) -> String {
    let (scheme, rest) = url.split_once("://").unwrap_or(("http", url));
    let host = rest.split(['/', '?', '#']).next().unwrap_or(rest);
    let host = host.rsplit_once('@').map_or(host, |(_, h)| h);
    if host.contains(':') && !host.ends_with(']') {
        host.to_string()
    } else {
        format!("{host}:{}", if scheme == "https" { 443 } else { 80 })
    }
}

/// The message of a `curl` that did not get an answer: no server (exit 6
/// can't resolve, 7 can't connect) or another failure.
pub fn error_text(url: &str, exit: Option<i32>, stderr: &str) -> String {
    if matches!(exit, Some(6 | 7)) {
        return format!("No speech-to-text server at {}", host_port(url));
    }
    let first = stderr
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("curl failed");
    format!(
        "Speech-to-text failed: {}",
        first.strip_prefix("curl: ").unwrap_or(first)
    )
}

#[cfg(test)]
#[path = "stt_tests.rs"]
mod tests;
