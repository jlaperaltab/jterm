//! Firma AWS Signature V4 (sin red: devuelve la petición firmada para que cualquier cliente HTTP la envíe).

use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;

#[derive(Debug, Clone)]
pub struct Credentials {
    pub access_key: String,
    pub secret: String,
}

/// Petición lista para enviar. `headers` NO incluye `host` (el cliente HTTP lo deriva de la URL).
#[derive(Debug, Clone, PartialEq)]
pub struct SignedRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn sha256_hex(b: &[u8]) -> String {
    hex(&Sha256::digest(b))
}

fn hmac(key: &[u8], msg: &str) -> Vec<u8> {
    let mut m = <Hmac<Sha256> as Mac>::new_from_slice(key).expect("HMAC acepta cualquier longitud");
    m.update(msg.as_bytes());
    m.finalize().into_bytes().to_vec()
}

pub fn amz_date(t: DateTime<Utc>) -> String {
    t.format("%Y%m%dT%H%M%SZ").to_string()
}

/// Codifica todo salvo `A-Za-z0-9-._~` (en EC2 también se deja `-_.~`; `.` es seguro en ambos).
pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

/// Codifica cada segmento una sola vez y conserva las "/".
pub fn encode_path(p: &str) -> String {
    p.split('/').map(encode).collect::<Vec<_>>().join("/")
}

pub fn canonical_query(q: &[(&str, &str)]) -> String {
    let mut pairs: Vec<(String, String)> = q.iter().map(|(k, v)| (encode(k), encode(v))).collect();
    pairs.sort();
    pairs.iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join("&")
}

pub fn signing_key(secret: &str, date_stamp: &str, region: &str, service: &str) -> Vec<u8> {
    let mut key = hmac(format!("AWS4{secret}").as_bytes(), date_stamp);
    for part in [region, service, "aws4_request"] {
        key = hmac(&key, part);
    }
    key
}

pub struct SignParams<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub host: &'a str,
    /// Ruta ya en su forma canónica de la URL (sin la consulta).
    pub canonical_path: &'a str,
    /// Consulta ya canónica (ordenada y codificada).
    pub canonical_query: &'a str,
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
    /// S3 exige enviar `x-amz-content-sha256` entre las cabeceras firmadas.
    pub include_content_sha: bool,
    pub service: &'a str,
    pub region: &'a str,
    pub creds: &'a Credentials,
    pub now: DateTime<Utc>,
}

/// Firma con cabeceras (`Authorization`).
pub fn sign(p: &SignParams, body: Vec<u8>) -> SignedRequest {
    let amz = amz_date(p.now);
    let date_stamp = &amz[..8];
    let mut all: BTreeMap<String, String> = BTreeMap::new();
    for (k, v) in p.headers {
        all.insert(k.to_lowercase(), v.trim().to_string());
    }
    all.insert("host".into(), p.host.into());
    all.insert("x-amz-date".into(), amz.clone());
    if p.include_content_sha {
        all.insert("x-amz-content-sha256".into(), p.payload_hash.into());
    }
    let canonical_headers: String = all.iter().map(|(k, v)| format!("{k}:{v}\n")).collect();
    let signed = all.keys().cloned().collect::<Vec<_>>().join(";");
    let path = if p.canonical_path.is_empty() { "/" } else { p.canonical_path };
    let canonical = [p.method, path, p.canonical_query, &canonical_headers, &signed, p.payload_hash].join("\n");
    let scope = format!("{date_stamp}/{}/{}/aws4_request", p.region, p.service);
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let sig = hex(&hmac(&signing_key(&p.creds.secret, date_stamp, p.region, p.service), &to_sign));

    let mut headers: Vec<(String, String)> = all.into_iter().filter(|(k, _)| k != "host").collect();
    headers.push((
        "authorization".into(),
        format!("AWS4-HMAC-SHA256 Credential={}/{scope}, SignedHeaders={signed}, Signature={sig}", p.creds.access_key),
    ));
    let url = if p.canonical_query.is_empty() {
        format!("{}://{}{}", p.scheme, p.host, path)
    } else {
        format!("{}://{}{}?{}", p.scheme, p.host, path, p.canonical_query)
    };
    SignedRequest { method: p.method.into(), url, headers, body }
}

/// URL prefirmada (la firma va en la URL).
pub fn presign(
    method: &str, scheme: &str, host: &str, encoded_path: &str, service: &str, region: &str,
    creds: &Credentials, expires: u32, now: DateTime<Utc>,
) -> String {
    let amz = amz_date(now);
    let date_stamp = &amz[..8];
    let scope = format!("{date_stamp}/{region}/{service}/aws4_request");
    let cred = format!("{}/{scope}", creds.access_key);
    let exp = expires.to_string();
    let cq = canonical_query(&[
        ("X-Amz-Algorithm", "AWS4-HMAC-SHA256"),
        ("X-Amz-Credential", &cred),
        ("X-Amz-Date", &amz),
        ("X-Amz-Expires", &exp),
        ("X-Amz-SignedHeaders", "host"),
    ]);
    let canonical = [method, encoded_path, &cq, &format!("host:{host}\n"), "host", "UNSIGNED-PAYLOAD"].join("\n");
    let to_sign = format!("AWS4-HMAC-SHA256\n{amz}\n{scope}\n{}", sha256_hex(canonical.as_bytes()));
    let sig = hex(&hmac(&signing_key(&creds.secret, date_stamp, region, service), &to_sign));
    format!("{scheme}://{host}{encoded_path}?{cq}&X-Amz-Signature={sig}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    /// Vector "get-vanilla" de la suite oficial de pruebas de SigV4.
    #[test]
    fn get_vanilla() {
        let creds = Credentials { access_key: "AKIDEXAMPLE".into(), secret: "wJalrXUtnFEMI/K7MDENG+bPxRfiCYEXAMPLEKEY".into() };
        let r = sign(
            &SignParams {
                method: "GET", scheme: "https", host: "example.amazonaws.com", canonical_path: "/", canonical_query: "",
                headers: &[], payload_hash: &sha256_hex(b""), include_content_sha: false,
                service: "service", region: "us-east-1", creds: &creds,
                now: Utc.with_ymd_and_hms(2015, 8, 30, 12, 36, 0).unwrap(),
            },
            vec![],
        );
        let auth = &r.headers.iter().find(|(k, _)| k == "authorization").unwrap().1;
        assert_eq!(
            auth,
            "AWS4-HMAC-SHA256 Credential=AKIDEXAMPLE/20150830/us-east-1/service/aws4_request, \
             SignedHeaders=host;x-amz-date, Signature=5fa00fa31553b73ebf1942676e86291e8372ff2a2260956d9b8aae1d763fbf31"
        );
    }

    /// Ejemplo de URL prefirmada de la documentación de S3.
    #[test]
    fn s3_presign_example() {
        let creds = Credentials { access_key: "AKIAIOSFODNN7EXAMPLE".into(), secret: "wJalrXUtnFEMI/K7MDENG/bPxRfiCYEXAMPLEKEY".into() };
        let u = presign(
            "GET", "https", "examplebucket.s3.amazonaws.com", "/test.txt", "s3", "us-east-1", &creds, 86400,
            Utc.with_ymd_and_hms(2013, 5, 24, 0, 0, 0).unwrap(),
        );
        assert!(u.ends_with("X-Amz-Signature=aeeed9bbccd4d02ee5c0109b86d86835f995330da4c265957d157751f604d404"), "{u}");
    }

    #[test]
    fn encoding() {
        assert_eq!(encode("a b/ñ~"), "a%20b%2F%C3%B1~");
        assert_eq!(encode_path("a b/c d"), "a%20b/c%20d");
        assert_eq!(canonical_query(&[("b", "2"), ("a", "1 2")]), "a=1%202&b=2");
    }
}
