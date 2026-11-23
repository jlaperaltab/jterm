//! Cliente mínimo de S3 sobre un [`Transport`]. Las credenciales se piden al usarlo, no al crearlo.

use crate::transport::{HttpResponse, Progress, Transport};
use chrono::Utc;
use futures_util::future::try_join_all;
use jterm_core::aws::AwsError;
use jterm_core::s3::{self, Bucket, Listing, RequestSpec, MAX_SINGLE_UPLOAD, UNSIGNED_PAYLOAD};
use jterm_core::sigv4::{self, Credentials, SignedRequest};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

type CredFn = Box<dyn Fn() -> Result<Credentials, AwsError> + Send + Sync>;
type EndpointFn = Box<dyn Fn(&str) -> (String, String) + Send + Sync>;

pub struct S3Client<T: Transport> {
    transport: T,
    credentials: CredFn,
    default_region: String,
    regions: Mutex<HashMap<String, String>>,
    /// (esquema, host) para una región. Las pruebas lo apuntan a un servidor de mentira.
    endpoint: EndpointFn,
}

impl<T: Transport> S3Client<T> {
    pub fn new(transport: T, default_region: &str, credentials: impl Fn() -> Result<Credentials, AwsError> + Send + Sync + 'static) -> Self {
        S3Client {
            transport,
            credentials: Box::new(credentials),
            default_region: default_region.into(),
            regions: Mutex::new(HashMap::new()),
            endpoint: Box::new(s3::default_endpoint),
        }
    }

    pub fn with_endpoint(mut self, f: impl Fn(&str) -> (String, String) + Send + Sync + 'static) -> Self {
        self.endpoint = Box::new(f);
        self
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    // MARK: Base

    async fn send(&self, req: SignedRequest) -> Result<HttpResponse, AwsError> {
        let r = self.transport.send(req).await?;
        if r.is_success() { Ok(r) } else { Err(AwsError(s3::error_message(&r.body, r.status))) }
    }

    fn failure(r: &HttpResponse) -> AwsError {
        AwsError(s3::error_message(&r.body, r.status))
    }

    /// `bucket = None` → petición al servicio (lista de buckets). `key` sin codificar.
    async fn request(
        &self, method: &str, bucket: Option<&str>, key: &str, query: &[(&str, &str)], headers: &[(&str, &str)], payload_hash: &str,
    ) -> Result<SignedRequest, AwsError> {
        let region = match bucket {
            Some(b) => self.region_of(b).await?,
            None => "us-east-1".to_string(),
        };
        let (scheme, host) = (self.endpoint)(&region);
        let path = match bucket {
            None => "/".to_string(),
            Some(b) if key.is_empty() => format!("/{b}"),
            Some(b) => format!("/{b}/{key}"),
        };
        let creds = (self.credentials)()?;
        Ok(s3::sign(
            &RequestSpec { method, scheme: &scheme, host: &host, path: &path, query, headers, payload_hash, region: &region },
            &creds,
            Utc::now(),
        ))
    }

    /// Región de un bucket (cabecera `x-amz-bucket-region` de un HEAD sin firmar), con caché.
    pub async fn region_of(&self, bucket: &str) -> Result<String, AwsError> {
        if let Some(r) = self.regions.lock().unwrap().get(bucket) {
            return Ok(r.clone());
        }
        let (scheme, host) = (self.endpoint)("us-east-1");
        let req = SignedRequest {
            method: "HEAD".into(),
            url: format!("{scheme}://{host}/{}", sigv4::encode(bucket)),
            headers: vec![],
            body: vec![],
        };
        let r = self.transport.send(req).await?;
        if r.status == 404 {
            return Err(AwsError(format!("El bucket “{bucket}” no existe (NoSuchBucket).")));
        }
        let region = r.header("x-amz-bucket-region").map(String::from).unwrap_or_else(|| self.default_region.clone());
        self.regions.lock().unwrap().insert(bucket.into(), region.clone());
        Ok(region)
    }

    // MARK: Listado

    /// Buckets de la cuenta, ordenados sin distinguir mayúsculas.
    pub async fn buckets(&self) -> Result<Vec<Bucket>, AwsError> {
        let r = self.send(self.request("GET", None, "", &[], &[], &s3::empty_hash()).await?).await?;
        let mut v = s3::parse(&r.body).buckets;
        v.sort_by_key(|b| b.name.to_lowercase());
        Ok(v)
    }

    /// Lista un nivel (con delimitador "/") o todo lo que cuelga del prefijo (`recursive`). Recorre las páginas.
    pub async fn list(&self, bucket: &str, prefix: &str, recursive: bool) -> Result<Listing, AwsError> {
        let mut all = Listing::default();
        let mut token: Option<String> = None;
        let mut pages = 0;
        loop {
            let mut q: Vec<(&str, &str)> = vec![("list-type", "2"), ("max-keys", "1000")];
            if !prefix.is_empty() { q.push(("prefix", prefix)); }
            if !recursive { q.push(("delimiter", "/")); }
            if let Some(t) = &token { q.push(("continuation-token", t)); }
            let r = self.send(self.request("GET", Some(bucket), "", &q, &[], &s3::empty_hash()).await?).await?;
            let mut p = s3::parse(&r.body);
            all.prefixes.append(&mut p.listing.prefixes);
            all.objects.append(&mut p.listing.objects);
            pages += 1;
            token = if p.truncated { p.next_token } else { None };
            if token.is_none() || pages >= 500 {
                return Ok(all);
            }
        }
    }

    // MARK: Cambios

    /// Borra los objetos, de 8 en 8 para no saturar la conexión ni el límite de peticiones.
    pub async fn delete(&self, bucket: &str, keys: &[String]) -> Result<(), AwsError> {
        for chunk in keys.chunks(8) {
            try_join_all(chunk.iter().map(|k| async move {
                let req = self.request("DELETE", Some(bucket), k, &[], &[], &s3::empty_hash()).await?;
                self.send(req).await.map(|_| ())
            }))
            .await?;
        }
        Ok(())
    }

    /// Crea una "carpeta" (un objeto vacío cuya clave termina en "/").
    pub async fn create_folder(&self, bucket: &str, prefix: &str) -> Result<(), AwsError> {
        let key = if prefix.ends_with('/') { prefix.to_string() } else { format!("{prefix}/") };
        let req = self.request("PUT", Some(bucket), &key, &[], &[], &s3::empty_hash()).await?;
        self.send(req).await.map(|_| ())
    }

    /// Copia un objeto dentro del mismo bucket (para renombrar).
    pub async fn copy(&self, bucket: &str, from: &str, to: &str) -> Result<(), AwsError> {
        let src = sigv4::encode_path(&format!("/{bucket}/{from}"));
        let req = self.request("PUT", Some(bucket), to, &[], &[("x-amz-copy-source", &src)], &s3::empty_hash()).await?;
        self.send(req).await.map(|_| ())
    }

    /// URL que permite descargar el objeto sin credenciales hasta que expire.
    pub async fn presigned_url(&self, bucket: &str, key: &str, expires: u32) -> Result<String, AwsError> {
        let region = self.region_of(bucket).await?;
        let (scheme, host) = (self.endpoint)(&region);
        let creds = (self.credentials)()?;
        Ok(s3::presign(&scheme, &host, &format!("/{bucket}/{key}"), &region, &creds, expires, Utc::now()))
    }

    // MARK: Transferencias (en streaming: no cargan el archivo en memoria)

    pub async fn upload(&self, file: &Path, bucket: &str, key: &str, progress: Progress) -> Result<(), AwsError> {
        let size = tokio::fs::metadata(file).await.map_err(|e| AwsError(e.to_string()))?.len();
        if size > MAX_SINGLE_UPLOAD {
            let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            return Err(AwsError(format!("“{name}” supera los 5 GB, el máximo de una subida simple a S3.")));
        }
        let mime = file.extension().and_then(|e| e.to_str()).and_then(|e| mime_guess::from_ext(e).first()).map(|m| m.essence_str().to_string());
        let mut headers: Vec<(&str, &str)> = vec![];
        if let Some(m) = &mime {
            headers.push(("content-type", m));
        }
        let req = self.request("PUT", Some(bucket), key, &[], &headers, UNSIGNED_PAYLOAD).await?;
        let r = self.transport.upload_file(req, file, progress.clone()).await?;
        if !r.is_success() {
            return Err(Self::failure(&r));
        }
        progress(size);
        Ok(())
    }

    /// Descarga a `dest` (escribe en un archivo `.part` y lo mueve al terminar, así un corte no deja un archivo a medias).
    pub async fn download(&self, bucket: &str, key: &str, dest: &Path, progress: Progress) -> Result<(), AwsError> {
        let io = |e: std::io::Error| AwsError(e.to_string());
        if let Some(dir) = dest.parent() {
            tokio::fs::create_dir_all(dir).await.map_err(io)?;
        }
        let mut part = dest.as_os_str().to_owned();
        part.push(".part");
        let part = std::path::PathBuf::from(part);
        let req = self.request("GET", Some(bucket), key, &[], &[], &s3::empty_hash()).await?;
        let r = self.transport.download_to(req, &part, progress.clone()).await?;
        if !r.is_success() {
            let _ = tokio::fs::remove_file(&part).await;
            return Err(Self::failure(&r));
        }
        let _ = tokio::fs::remove_file(dest).await;
        tokio::fs::rename(&part, dest).await.map_err(io)?;
        if let Ok(m) = tokio::fs::metadata(dest).await {
            progress(m.len());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{response, FakeTransport};
    use std::sync::atomic::{AtomicU64, Ordering};
    use std::sync::Arc;

    fn client(handler: impl Fn(&SignedRequest) -> HttpResponse + Send + Sync + 'static) -> S3Client<FakeTransport> {
        S3Client::new(FakeTransport::new(handler), "us-east-1", || Ok(Credentials { access_key: "AK".into(), secret: "SK".into() }))
    }

    fn head_region(region: &'static str) -> impl Fn(&SignedRequest) -> HttpResponse {
        move |req| {
            let mut r = response(200, "");
            if req.method == "HEAD" {
                r.headers.insert("x-amz-bucket-region".into(), region.into());
            }
            r
        }
    }

    fn no_progress() -> Progress {
        Arc::new(|_| {})
    }

    #[tokio::test]
    async fn list_follows_pages_in_the_buckets_region_and_caches_it() {
        let c = client(|req| {
            if req.method == "HEAD" {
                let mut r = response(200, "");
                r.headers.insert("x-amz-bucket-region".into(), "eu-west-1".into());
                return r;
            }
            if req.url.contains("continuation-token=TOK") {
                response(200, "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>b</Key><Size>2</Size></Contents></ListBucketResult>")
            } else {
                response(200, "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>TOK</NextContinuationToken><Contents><Key>a</Key><Size>1</Size></Contents><CommonPrefixes><Prefix>d/</Prefix></CommonPrefixes></ListBucketResult>")
            }
        });
        let l = c.list("mi-bucket", "", false).await.unwrap();
        assert_eq!(l.objects.iter().map(|o| o.key.as_str()).collect::<Vec<_>>(), ["a", "b"]);
        assert_eq!(l.prefixes, ["d/"]);
        c.list("mi-bucket", "x/", true).await.unwrap();
        let reqs = c.transport().recorded();
        assert_eq!(reqs.iter().filter(|r| r.method == "HEAD").count(), 1, "la región se cachea");
        assert!(reqs[1].url.starts_with("https://s3.eu-west-1.amazonaws.com/mi-bucket?"));
        assert!(reqs[1].url.contains("delimiter=%2F") && reqs[1].url.contains("list-type=2"));
        assert!(!reqs.last().unwrap().url.contains("delimiter"));
    }

    #[tokio::test]
    async fn missing_bucket_and_access_denied() {
        let c = client(|req| if req.method == "HEAD" { response(404, "") } else { response(200, "") });
        let e = c.list("nada", "", false).await.unwrap_err();
        assert!(e.0.contains("no existe"), "{}", e.0);

        let c = client(|req| {
            if req.method == "HEAD" { response(200, "") } else { response(403, "<Error><Code>AccessDenied</Code><Message>Denegado</Message></Error>") }
        });
        let e = c.buckets().await.unwrap_err();
        assert!(e.0.starts_with("Denegado (AccessDenied)") && e.0.contains("s3:ListBucket"));
    }

    #[tokio::test]
    async fn buckets_sorted_case_insensitively() {
        let c = client(|_| response(200, "<ListAllMyBucketsResult><Buckets><Bucket><Name>beta</Name></Bucket><Bucket><Name>Alfa</Name></Bucket></Buckets></ListAllMyBucketsResult>"));
        let names: Vec<String> = c.buckets().await.unwrap().into_iter().map(|b| b.name).collect();
        assert_eq!(names, ["Alfa", "beta"]);
        assert_eq!(c.transport().recorded()[0].url, "https://s3.us-east-1.amazonaws.com/");
    }

    #[tokio::test]
    async fn delete_sends_one_request_per_key() {
        let c = client(head_region("us-east-1"));
        let keys: Vec<String> = (0..19).map(|i| format!("dir/k {i}")).collect();
        c.delete("b", &keys).await.unwrap();
        let reqs = c.transport().recorded();
        assert_eq!(reqs.iter().filter(|r| r.method == "DELETE").count(), 19);
        assert!(reqs.iter().any(|r| r.url.ends_with("/b/dir/k%200")));
    }

    #[tokio::test]
    async fn folder_and_copy() {
        let c = client(head_region("us-east-1"));
        c.create_folder("b", "a/b").await.unwrap();
        c.copy("b", "viejo nombre.txt", "nuevo.txt").await.unwrap();
        let reqs = c.transport().recorded();
        let put: Vec<_> = reqs.iter().filter(|r| r.method == "PUT").collect();
        assert!(put[0].url.ends_with("/b/a/b/"));
        assert!(put[1].headers.iter().any(|(k, v)| k == "x-amz-copy-source" && v == "/b/viejo%20nombre.txt"));
    }

    #[tokio::test]
    async fn presigned_url_is_built_locally() {
        let c = client(head_region("sa-east-1"));
        let u = c.presigned_url("b", "a b.txt", 600).await.unwrap();
        assert!(u.starts_with("https://s3.sa-east-1.amazonaws.com/b/a%20b.txt?X-Amz-Algorithm=AWS4-HMAC-SHA256"));
        assert!(u.contains("X-Amz-Expires=600") && u.contains("X-Amz-Signature="));
        assert_eq!(c.transport().recorded().len(), 1, "solo la consulta de región");
    }

    #[tokio::test]
    async fn upload_sets_type_and_reports_progress() {
        let dir = tempfile::tempdir().unwrap();
        let f = dir.path().join("nota.txt");
        std::fs::write(&f, "hola").unwrap();
        let c = client(head_region("us-east-1"));
        let seen = Arc::new(AtomicU64::new(0));
        let s2 = seen.clone();
        c.upload(&f, "b", "x/nota.txt", Arc::new(move |n| s2.store(n, Ordering::SeqCst))).await.unwrap();
        assert_eq!(seen.load(Ordering::SeqCst), 4);
        let put = c.transport().recorded().into_iter().find(|r| r.method == "PUT").unwrap();
        assert!(put.headers.iter().any(|(k, v)| k == "content-type" && v == "text/plain"));
        assert!(put.headers.iter().any(|(k, v)| k == "x-amz-content-sha256" && v == UNSIGNED_PAYLOAD));
    }

    #[tokio::test]
    async fn download_moves_part_file_and_cleans_up_on_error() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("sub/dir/archivo.bin");
        let c = client(|req| {
            if req.method == "HEAD" { response(200, "") } else { response(200, "contenido") }
        });
        c.download("b", "k", &dest, no_progress()).await.unwrap();
        assert_eq!(std::fs::read_to_string(&dest).unwrap(), "contenido");
        assert!(!dest.with_extension("bin.part").exists());

        let bad = dir.path().join("malo.bin");
        let c = client(|req| {
            if req.method == "HEAD" { response(200, "") } else { response(404, "<Error><Code>NoSuchKey</Code><Message>No hay</Message></Error>") }
        });
        let e = c.download("b", "k", &bad, no_progress()).await.unwrap_err();
        assert!(e.0.starts_with("No hay (NoSuchKey)"));
        assert!(!bad.exists());
    }
}
