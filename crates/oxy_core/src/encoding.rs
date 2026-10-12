//! Small encodings for tool protocols.

/// Standard base64 with padding (RFC 4648), e.g. for PNG images sent as JSON.
pub fn base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Standard base64 back to bytes. Whitespace and line breaks are skipped, padding is optional
/// and a `data:…;base64,` prefix (a data URL) is accepted; any other character is an error.
pub fn decode_base64(text: &str) -> Result<Vec<u8>, String> {
    let text = match text.trim_start().strip_prefix("data:") {
        Some(url) => url
            .split_once(";base64,")
            .map(|(_, data)| data)
            .ok_or("URL de dados sem ;base64,")?,
        None => text,
    };
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let (mut bits, mut count) = (0u32, 0u32);
    let mut padding = 0;
    for (index, byte) in text.bytes().enumerate() {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            b'=' => {
                padding += 1;
                continue;
            }
            b' ' | b'\t' | b'\r' | b'\n' => continue,
            _ => return Err(format!("Caractere inválido em base64 na posição {index}")),
        };
        if padding > 0 {
            return Err("Dados depois do = em base64".into());
        }
        bits = (bits << 6) | u32::from(value);
        count += 6;
        if count >= 8 {
            count -= 8;
            out.push((bits >> count) as u8);
            bits &= (1 << count) - 1;
        }
    }
    // A last group of one character cannot hold a byte.
    if count >= 6 || padding > 2 {
        return Err("base64 incompleto".into());
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    #[test]
    fn base64_matches_rfc_4648_examples() {
        for (input, expected) in [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ] {
            assert_eq!(super::base64(input.as_bytes()), expected);
            assert_eq!(
                super::decode_base64(expected).unwrap(),
                input.as_bytes(),
                "{expected}"
            );
        }
    }

    #[test]
    fn base64_decodes_wrapped_unpadded_and_data_urls_and_refuses_the_rest() {
        let bytes: Vec<u8> = (0..=255).collect();
        let text = super::base64(&bytes);
        let wrapped: String = text
            .as_bytes()
            .chunks(76)
            .map(|line| format!("{}\r\n", std::str::from_utf8(line).unwrap()))
            .collect();
        assert_eq!(super::decode_base64(&wrapped).unwrap(), bytes);
        assert_eq!(super::decode_base64("Zm9vYg").unwrap(), b"foob");
        assert_eq!(
            super::decode_base64("data:image/png;base64,Zm9v").unwrap(),
            b"foo"
        );
        for bad in ["Zm9v!", "Zg==Zg", "Z", "data:image/png,Zm9v", "Zg==="] {
            assert!(super::decode_base64(bad).is_err(), "{bad}");
        }
    }
}
