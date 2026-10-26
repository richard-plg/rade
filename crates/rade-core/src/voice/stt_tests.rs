use super::*;

#[test]
fn r124_i_stt_request_reply_and_tentative_words() {
    let cfg = VoiceConfig {
        stt_key_command: "echo sk-test".into(),
        ..VoiceConfig::default()
    };
    // The key (the command's output) is only in the config text.
    let conf = curl_config(&cfg, Some("sk-test\n"), "applyPromo handleSubmit");
    assert!(conf.contains("header = \"Authorization: Bearer sk-test\"\n"));
    assert!(conf.contains("url = \"http://127.0.0.1:8178/v1/audio/transcriptions\"\n"));
    assert!(conf.contains("form = \"model=whisper-1\"\n"));
    assert!(conf.contains("form = \"response_format=json\"\n"));
    assert!(conf.contains("form-string = \"prompt=applyPromo handleSubmit\"\n"));
    assert!(!conf.contains("language="), "auto sends no language");
    assert!(!curl_argv().concat().contains("sk-test"));
    assert_eq!(&curl_argv()[4..6], ["-K", "/dev/fd/3"]);
    assert!(curl_argv().contains(&"file=@-;filename=take.wav;type=audio/wav".to_string()));
    // No key, no header; a language code is sent; values are escaped.
    let vi = VoiceConfig {
        stt_language: "vi".into(),
        ..VoiceConfig::default()
    };
    let conf = curl_config(&vi, None, "say \"hi\"\n");
    assert!(!conf.contains("Authorization") && conf.contains("form = \"language=vi\"\n"));
    assert!(conf.contains("prompt=say \\\"hi\\\"\\n\""));
    let bad = VoiceConfig {
        stt_language: "Viet".into(),
        ..VoiceConfig::default()
    };
    assert!(!curl_config(&bad, None, "").contains("language="));

    // Replies.
    assert_eq!(
        parse("{\"text\":\" ừ thì sửa cái này \"}").unwrap(),
        "ừ thì sửa cái này"
    );
    // whisper.cpp ends each segment with a newline: one line of words.
    assert_eq!(
        parse("{\"text\":\" Ừ thì sửa nó.\\n À ở cái này.\\n\"}").unwrap(),
        "Ừ thì sửa nó. À ở cái này."
    );
    assert_eq!(parse("{\"text\":\" ok \"}\n200").unwrap(), "ok");
    assert_eq!(parse("{\"text\":\"\"}").unwrap(), "");
    assert_eq!(
        parse("{\"error\":{\"message\":\"bad key\"}}\n401").unwrap_err(),
        "Speech-to-text failed: 401 bad key"
    );
    assert_eq!(
        parse("oops\nsecond\n500").unwrap_err(),
        "Speech-to-text failed: 500 oops"
    );

    // Last three words are tentative.
    assert_eq!(
        split_tentative(" ừ thì cái phần promo code á "),
        ("ừ thì cái phần".to_string(), "promo code á".to_string())
    );
    assert_eq!(
        split_tentative("one two"),
        (String::new(), "one two".to_string())
    );

    // Errors, WAV, level.
    assert_eq!(host_port("http://127.0.0.1:8178/v1/x"), "127.0.0.1:8178");
    assert_eq!(
        host_port("https://api.openai.com/v1/audio"),
        "api.openai.com:443"
    );
    assert_eq!(
        error_text(
            "http://127.0.0.1:8178/v1/x",
            Some(7),
            "curl: (7) Failed to connect"
        ),
        "No speech-to-text server at 127.0.0.1:8178"
    );
    assert_eq!(
        error_text("http://h/", Some(28), "curl: (28) Operation timed out"),
        "Speech-to-text failed: (28) Operation timed out"
    );
    let w = wav(&[1, -1, 2]);
    assert_eq!((&w[..4], &w[8..16]), (&b"RIFF"[..], &b"WAVEfmt "[..]));
    assert_eq!(u32::from_le_bytes(w[24..28].try_into().unwrap()), 16_000);
    assert_eq!(u32::from_le_bytes(w[40..44].try_into().unwrap()), 6);
    assert_eq!(w.len(), 50);
    assert_eq!(level(&[]), 0.0);
    assert_eq!(level(&[0; 100]), 0.0);
    assert!(level(&[i16::MAX; 100]) > 0.99 && level(&[i16::MAX; 100]) <= 1.0);
    assert!(level(&[1000; 100]) > level(&[100; 100]));
}

/// 50 ms of a tone at `amp` (0..1 of full scale), or silence at 0.
fn frames(amp: f64, n: usize) -> Vec<i16> {
    (0..n * 800)
        .map(|i| ((i as f64 * 0.17).sin() * amp * 32767.0) as i16)
        .collect()
}

#[test]
fn r124_s_silence_gate_and_hallucinations() {
    // Silence and a quiet room (-50 dBFS) hold no speech.
    assert_eq!(speech_ms(&frames(0.0, 40), 0.1), 0);
    assert_eq!(speech_ms(&frames(0.003, 40), 0.1), 0);
    // Talking (-20 dBFS) for 0.5 s within 2 s of room noise does.
    let mut take = frames(0.003, 20);
    take.extend(frames(0.1, 10));
    take.extend(frames(0.003, 10));
    assert_eq!(speech_ms(&take, 0.1), 500);
    assert!(has_speech(&take, 0.1));
    // A cough-length blip (100 ms) is not a take.
    let mut blip = frames(0.003, 20);
    blip.extend(frames(0.1, 2));
    assert!(!has_speech(&blip, 0.1));
    // A loud room raises the bar: noise at -26 dBFS everywhere is not speech.
    assert!(!has_speech(&frames(0.05, 40), 0.1));
    // …up to a point: a loud steady voice (-10 dBFS, no pauses) still counts.
    assert!(has_speech(&frames(0.3, 40), 0.1));

    // Whisper's YouTube outros on silence or noise go; real words stay.
    assert_eq!(
        drop_hallucinations(
            "Hãy subscribe cho kênh Ghiền Mì Gõ để không bỏ lỡ những video hấp dẫn."
        ),
        ""
    );
    assert_eq!(
        drop_hallucinations("Sửa cái hàm useTotals. Cảm ơn các bạn đã theo dõi!"),
        "Sửa cái hàm useTotals."
    );
    assert_eq!(drop_hallucinations("Thanks for watching!"), "");
    assert_eq!(
        drop_hallucinations("thêm nút subscribe vào trang settings"),
        "thêm nút subscribe vào trang settings"
    );
}
