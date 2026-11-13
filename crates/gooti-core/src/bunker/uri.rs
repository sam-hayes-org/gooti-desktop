//! `bunker://` links this desktop hands out, and `nostrconnect://` links apps hand to it.

use crate::backup::document::is_hex_key;
use crate::error::{Error, Result};

/// `bunker://<signer-pubkey>?relay=…&relay=…&secret=…&gooti=1`
///
/// `gooti=1` tells the Gooti extension that Gooti Desktop holds the key, so the extension does not
/// ask before a request and sends the site's name along. Other apps ignore the parameter.
pub fn bunker_uri(signer_pubkey: &str, relays: &[String], secret: &str) -> String {
    let mut uri = format!("bunker://{signer_pubkey}?");
    let mut params: Vec<String> = relays
        .iter()
        .map(|relay| format!("relay={}", encode(relay)))
        .collect();
    params.push(format!("secret={}", encode(secret)));
    params.push("gooti=1".into());
    uri.push_str(&params.join("&"));
    uri
}

/// A parsed `nostrconnect://` link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NostrConnect {
    pub client_pubkey: String,
    pub relays: Vec<String>,
    pub secret: String,
    pub perms: Option<String>,
    pub name: Option<String>,
    pub url: Option<String>,
    pub image: Option<String>,
}

pub fn parse_nostrconnect(input: &str) -> Result<NostrConnect> {
    let input = input.trim();
    let rest = input
        .strip_prefix("nostrconnect://")
        .ok_or(Error::InvalidConnectLink)?;
    let (pubkey, query) = rest.split_once('?').unwrap_or((rest, ""));
    let client_pubkey = pubkey.trim_end_matches('/').to_lowercase();
    if !is_hex_key(&client_pubkey) {
        return Err(Error::InvalidConnectLink);
    }
    let mut link = NostrConnect {
        client_pubkey,
        relays: Vec::new(),
        secret: String::new(),
        perms: None,
        name: None,
        url: None,
        image: None,
    };
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let value = decode(value);
        match key {
            "relay" => {
                let relay = value.trim().to_string();
                if (relay.starts_with("wss://") || relay.starts_with("ws://"))
                    && !link.relays.contains(&relay)
                {
                    link.relays.push(relay);
                }
            }
            "secret" => link.secret = value,
            "perms" if !value.is_empty() => link.perms = Some(value),
            "name" if !value.is_empty() => link.name = Some(value),
            "url" if !value.is_empty() => link.url = Some(value),
            "image" if !value.is_empty() => link.image = Some(value),
            // Older links carry `metadata` as JSON.
            "metadata" => {
                if let Ok(meta) = serde_json::from_str::<serde_json::Value>(&value) {
                    let text = |k: &str| meta.get(k).and_then(|v| v.as_str()).map(String::from);
                    link.name = link.name.take().or_else(|| text("name"));
                    link.url = link.url.take().or_else(|| text("url"));
                    link.image = link.image.take().or_else(|| text("image"));
                }
            }
            _ => {}
        }
    }
    if link.relays.is_empty() || link.secret.is_empty() {
        return Err(Error::InvalidConnectLink);
    }
    Ok(link)
}

/// Percent-encoding for query values: everything but unreserved characters.
fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' if i + 2 < bytes.len() => match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                (Some(high), Some(low)) => {
                    out.push(high << 4 | low);
                    i += 3;
                }
                _ => {
                    out.push(b'%');
                    i += 1;
                }
            },
            b'+' => {
                out.push(b' ');
                i += 1;
            }
            byte => {
                out.push(byte);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    const PUB: &str = "eff37350d839ce3707332348af4549a96051bd695d3223af4aabce4993531d86";

    #[test]
    fn bunker_links_encode_relays_and_secret() {
        let uri = bunker_uri(
            PUB,
            &[
                "wss://relay.getgooti.com".into(),
                "wss://r.example/a b".into(),
            ],
            "s3 cr&t",
        );
        assert_eq!(
            uri,
            format!(
                "bunker://{PUB}?relay=wss%3A%2F%2Frelay.getgooti.com&relay=wss%3A%2F%2Fr.example%2Fa%20b&secret=s3%20cr%26t&gooti=1"
            )
        );
    }

    #[test]
    fn parses_a_nostrconnect_link() {
        let link = parse_nostrconnect(&format!(
            " nostrconnect://{}?relay=wss%3A%2F%2Frelay1.example.com&perms=nip44_encrypt%2Csign_event%3A13&name=My+Client&secret=0s8j2djs&relay=wss://relay2.example2.com&url=https%3A%2F%2Fapp.example ",
            PUB.to_uppercase()
        ))
        .unwrap();
        assert_eq!(link.client_pubkey, PUB);
        assert_eq!(
            link.relays,
            ["wss://relay1.example.com", "wss://relay2.example2.com"]
        );
        assert_eq!(link.secret, "0s8j2djs");
        assert_eq!(link.perms.as_deref(), Some("nip44_encrypt,sign_event:13"));
        assert_eq!(link.name.as_deref(), Some("My Client"));
        assert_eq!(link.url.as_deref(), Some("https://app.example"));
    }

    #[test]
    fn reads_older_metadata_json() {
        let link = parse_nostrconnect(&format!(
            "nostrconnect://{PUB}?relay=wss://r.example&secret=x&metadata=%7B%22name%22%3A%22Coracle%22%7D"
        ))
        .unwrap();
        assert_eq!(link.name.as_deref(), Some("Coracle"));
    }

    #[test]
    fn decoding_survives_odd_percent_signs() {
        assert_eq!(decode("100%"), "100%");
        assert_eq!(decode("%zz%41"), "%zzA");
        assert_eq!(decode("%C3%A9t%C3%A9"), "été");
        assert_eq!(decode("%é"), "%é");
    }

    #[test]
    fn rejects_links_without_key_relay_or_secret() {
        for bad in [
            "bunker://abc".to_string(),
            "nostrconnect://nothex?relay=wss://r&secret=x".to_string(),
            format!("nostrconnect://{PUB}?secret=x"),
            format!("nostrconnect://{PUB}?relay=wss://r"),
            format!("nostrconnect://{PUB}?relay=https://r&secret=x"),
        ] {
            assert!(parse_nostrconnect(&bad).is_err(), "{bad}");
        }
    }
}
