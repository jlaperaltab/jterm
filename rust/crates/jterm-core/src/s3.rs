//! S3: firma, URLs prefirmadas, lectura de respuestas XML y rutas del explorador. Sin red.

use crate::sigv4::{self, Credentials, SignParams, SignedRequest};
use crate::xml::{self, Ev};
use chrono::{DateTime, Utc};
use std::path::{Component, Path, PathBuf};

pub const UNSIGNED_PAYLOAD: &str = "UNSIGNED-PAYLOAD";
/// Límite de S3 para un solo PUT.
pub const MAX_SINGLE_UPLOAD: u64 = 5 * 1024 * 1024 * 1024 - 1;

pub fn empty_hash() -> String {
    sigv4::sha256_hex(b"")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bucket {
    pub name: String,
    pub created: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub key: String,
    pub size: u64,
    pub modified: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listing {
    pub prefixes: Vec<String>,
    pub objects: Vec<Object>,
}

/// Endpoint de una región: (esquema, host). Los tests y servidores locales lo reemplazan.
pub fn default_endpoint(region: &str) -> (String, String) {
    ("https".into(), format!("s3.{region}.amazonaws.com"))
}

pub struct RequestSpec<'a> {
    pub method: &'a str,
    pub scheme: &'a str,
    pub host: &'a str,
    pub path: &'a str,
    pub query: &'a [(&'a str, &'a str)],
    pub headers: &'a [(&'a str, &'a str)],
    pub payload_hash: &'a str,
    pub region: &'a str,
}

/// Petición firmada con cabeceras (listar, subir, bajar, borrar…). `path` sin codificar.
pub fn sign(spec: &RequestSpec, creds: &Credentials, now: DateTime<Utc>) -> SignedRequest {
    let path = if spec.path.is_empty() { "/" } else { spec.path };
    sigv4::sign(
        &SignParams {
            method: spec.method, scheme: spec.scheme, host: spec.host, canonical_path: &sigv4::encode_path(path),
            canonical_query: &sigv4::canonical_query(spec.query), headers: spec.headers, payload_hash: spec.payload_hash,
            include_content_sha: true, service: "s3", region: spec.region, creds, now,
        },
        Vec::new(),
    )
}

/// URL de descarga válida `expires` segundos sin credenciales.
pub fn presign(scheme: &str, host: &str, path: &str, region: &str, creds: &Credentials, expires: u32, now: DateTime<Utc>) -> String {
    sigv4::presign("GET", scheme, host, &sigv4::encode_path(path), "s3", region, creds, expires, now)
}

fn parse_date(s: &str) -> Option<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(s.trim()).ok().map(|d| d.with_timezone(&Utc))
}

#[derive(Debug, Default)]
pub struct Parsed {
    pub listing: Listing,
    pub buckets: Vec<Bucket>,
    pub truncated: bool,
    pub next_token: Option<String>,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

pub fn parse(data: &[u8]) -> Parsed {
    let mut out = Parsed::default();
    let (mut key, mut size, mut modified) = (String::new(), 0u64, None);
    let (mut bname, mut bdate) = (String::new(), None);
    // `start` y `end` comparten `out`: se recogen los eventos primero.
    let mut events: Vec<(bool, String, String)> = Vec::new();
    xml::walk(data, |ev, p, t| events.push((ev == Ev::Start, p.into(), t.into())));
    for (is_start, p, t) in events {
        if is_start {
            match p.as_str() {
                "ListBucketResult/Contents" => { key.clear(); size = 0; modified = None; }
                "ListAllMyBucketsResult/Buckets/Bucket" => { bname.clear(); bdate = None; }
                _ => {}
            }
            continue;
        }
        match p.as_str() {
            "ListBucketResult/Contents/Key" => key = t,
            "ListBucketResult/Contents/Size" => size = t.trim().parse().unwrap_or(0),
            "ListBucketResult/Contents/LastModified" => modified = parse_date(&t),
            "ListBucketResult/Contents" => out.listing.objects.push(Object { key: key.clone(), size, modified }),
            "ListBucketResult/CommonPrefixes/Prefix" => out.listing.prefixes.push(t),
            "ListBucketResult/IsTruncated" => out.truncated = t.trim() == "true",
            "ListBucketResult/NextContinuationToken" => out.next_token = (!t.is_empty()).then_some(t),
            "ListAllMyBucketsResult/Buckets/Bucket/Name" => bname = t,
            "ListAllMyBucketsResult/Buckets/Bucket/CreationDate" => bdate = parse_date(&t),
            "ListAllMyBucketsResult/Buckets/Bucket" => out.buckets.push(Bucket { name: bname.clone(), created: bdate }),
            "Error/Code" => out.error_code = Some(t),
            "Error/Message" => out.error_message = Some(t),
            _ => {}
        }
    }
    out
}

/// Mensaje legible de un error de S3.
pub fn error_message(data: &[u8], status: u16) -> String {
    let p = parse(data);
    let msg = p.error_message.unwrap_or_else(|| format!("Error HTTP {status}"));
    let full = msg + &p.error_code.as_deref().map(|c| format!(" ({c})")).unwrap_or_default();
    if p.error_code.as_deref() == Some("AccessDenied") || status == 403 {
        return format!("{full}\n\nPermisos IAM necesarios: s3:ListAllMyBuckets (listar buckets), s3:ListBucket, s3:GetObject, s3:PutObject y s3:DeleteObject.");
    }
    full
}

/// Rutas del explorador: "/" = buckets, "/bucket/carpeta/archivo" = clave "carpeta/archivo".
pub mod path {
    use super::*;

    /// Separa una ruta en (bucket, clave sin "/" final). Acepta también `s3://bucket/clave`.
    pub fn parse(raw: &str) -> (Option<String>, String) {
        let mut p = raw.trim().to_string();
        if p.to_lowercase().starts_with("s3://") {
            p = format!("/{}", &p[5..]);
        }
        let mut comps = p.split('/').filter(|c| !c.is_empty()).map(String::from);
        match comps.next() {
            None => (None, String::new()),
            Some(b) => (Some(b), comps.collect::<Vec<_>>().join("/")),
        }
    }

    pub fn join(bucket: &str, key: &str) -> String {
        if key.is_empty() { format!("/{bucket}") } else { format!("/{bucket}/{key}") }
    }

    /// Prefijo para listar lo que cuelga de una "carpeta".
    pub fn prefix(key: &str) -> String {
        if key.is_empty() { String::new() } else { format!("{key}/") }
    }

    /// Destino local seguro para `relative` dentro de `base`; `None` si la clave intenta salirse (`..`, rutas absolutas).
    pub fn safe_destination(base: &Path, relative: &str) -> Option<PathBuf> {
        let parts: Vec<&str> = relative.split('/').filter(|p| !p.is_empty()).collect();
        if parts.is_empty() || parts.iter().any(|p| *p == ".." || *p == ".") {
            return None;
        }
        let mut out = base.to_path_buf();
        for p in parts {
            // Un segmento con separadores propios del sistema o raíz no debe interpretarse como ruta.
            if Path::new(p).components().any(|c| !matches!(c, Component::Normal(_))) {
                return None;
            }
            out.push(p);
        }
        out.starts_with(base).then_some(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    #[test]
    fn parse_listing_and_buckets() {
        let p = parse(br#"<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>tok</NextContinuationToken>
          <Contents><Key>a/b.txt</Key><Size>12</Size><LastModified>2024-01-02T03:04:05.000Z</LastModified></Contents>
          <CommonPrefixes><Prefix>a/c/</Prefix></CommonPrefixes></ListBucketResult>"#);
        assert!(p.truncated);
        assert_eq!(p.next_token.as_deref(), Some("tok"));
        assert_eq!(p.listing.prefixes, ["a/c/"]);
        assert_eq!(p.listing.objects[0].size, 12);
        assert_eq!(p.listing.objects[0].modified, Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).single());
        let b = parse(b"<ListAllMyBucketsResult><Buckets><Bucket><Name>x</Name><CreationDate>2020-05-06T07:08:09Z</CreationDate></Bucket></Buckets></ListAllMyBucketsResult>");
        assert_eq!(b.buckets[0].name, "x");
    }

    #[test]
    fn errors_and_hint() {
        let m = error_message(b"<Error><Code>AccessDenied</Code><Message>No</Message></Error>", 403);
        assert!(m.starts_with("No (AccessDenied)") && m.contains("s3:ListBucket"));
        assert_eq!(error_message(b"", 500), "Error HTTP 500");
    }

    #[test]
    fn signing_includes_content_sha() {
        let c = Credentials { access_key: "AK".into(), secret: "SK".into() };
        let now = Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap();
        let h = empty_hash();
        let r = sign(
            &RequestSpec { method: "GET", scheme: "https", host: "s3.us-east-1.amazonaws.com", path: "/b/a b.txt",
                query: &[("list-type", "2"), ("prefix", "x y")], headers: &[], payload_hash: &h, region: "us-east-1" },
            &c, now,
        );
        assert_eq!(r.url, "https://s3.us-east-1.amazonaws.com/b/a%20b.txt?list-type=2&prefix=x%20y");
        assert!(r.headers.iter().any(|(k, v)| k == "x-amz-content-sha256" && *v == h));
    }

    #[test]
    fn paths() {
        assert_eq!(path::parse("s3://bk/a/b/"), (Some("bk".into()), "a/b".into()));
        assert_eq!(path::parse("/"), (None, String::new()));
        assert_eq!(path::join("b", ""), "/b");
        assert_eq!(path::prefix("a"), "a/");
    }

    #[test]
    fn safe_destination_blocks_escape() {
        let base = Path::new("/tmp/dl");
        assert_eq!(path::safe_destination(base, "a/b.txt"), Some(PathBuf::from("/tmp/dl/a/b.txt")));
        assert_eq!(path::safe_destination(base, "../x"), None);
        assert_eq!(path::safe_destination(base, "a/../../x"), None);
        assert_eq!(path::safe_destination(base, "/"), None);
    }
}
