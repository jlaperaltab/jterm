use futures_util::StreamExt;
use jterm_core::aws::AwsError;
use jterm_core::sigv4::SignedRequest;
use std::collections::HashMap;
use std::future::Future;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct HttpResponse {
    pub status: u16,
    /// Nombres en minúsculas.
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

impl HttpResponse {
    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.get(&name.to_lowercase()).map(String::as_str)
    }
}

/// Recibe los bytes transferidos hasta ahora.
pub type Progress = Arc<dyn Fn(u64) + Send + Sync>;

/// Salida de red. Los clientes dependen de este trait para poder simularlo en las pruebas.
pub trait Transport: Send + Sync {
    /// Envía la petición con su cuerpo en memoria.
    fn send(&self, req: SignedRequest) -> impl Future<Output = Result<HttpResponse, AwsError>> + Send;

    /// Sube `file` como cuerpo (en streaming, sin cargarlo en memoria). `req.body` se ignora.
    fn upload_file(&self, req: SignedRequest, file: &Path, progress: Progress) -> impl Future<Output = Result<HttpResponse, AwsError>> + Send;

    /// Descarga el cuerpo a `dest`. Solo se escribe si la respuesta es 2xx; si no, el cuerpo (el error) va en la respuesta.
    fn download_to(&self, req: SignedRequest, dest: &Path, progress: Progress) -> impl Future<Output = Result<HttpResponse, AwsError>> + Send;
}

fn err(e: impl std::fmt::Display) -> AwsError {
    AwsError(e.to_string())
}

fn headers_of(r: &reqwest::Response) -> HashMap<String, String> {
    r.headers().iter().filter_map(|(k, v)| Some((k.as_str().to_lowercase(), v.to_str().ok()?.to_string()))).collect()
}

/// Transporte real con reqwest (rustls).
#[derive(Clone)]
pub struct ReqwestTransport {
    client: reqwest::Client,
    /// Tiempo máximo de las peticiones normales.
    pub timeout: Duration,
    /// Tiempo máximo de las transferencias de archivos.
    pub transfer_timeout: Duration,
}

impl ReqwestTransport {
    pub fn new() -> Result<Self, AwsError> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .user_agent("jTerm")
            .build()
            .map_err(err)?;
        Ok(ReqwestTransport { client, timeout: Duration::from_secs(30), transfer_timeout: Duration::from_secs(3600) })
    }

    fn builder(&self, req: &SignedRequest, timeout: Duration) -> Result<reqwest::RequestBuilder, AwsError> {
        let method = reqwest::Method::from_bytes(req.method.as_bytes()).map_err(err)?;
        let mut b = self.client.request(method, &req.url).timeout(timeout);
        for (k, v) in &req.headers {
            b = b.header(k.as_str(), v.as_str());
        }
        Ok(b)
    }

    async fn into_response(r: reqwest::Response) -> Result<HttpResponse, AwsError> {
        let status = r.status().as_u16();
        let headers = headers_of(&r);
        let body = r.bytes().await.map_err(err)?.to_vec();
        Ok(HttpResponse { status, headers, body })
    }
}

impl Transport for ReqwestTransport {
    async fn send(&self, req: SignedRequest) -> Result<HttpResponse, AwsError> {
        let mut b = self.builder(&req, self.timeout)?;
        // PUT/POST sin cuerpo necesitan `Content-Length: 0`; reqwest lo añade con un cuerpo vacío.
        if !req.body.is_empty() || matches!(req.method.as_str(), "PUT" | "POST") {
            b = b.body(req.body.clone());
        }
        Self::into_response(b.send().await.map_err(err)?).await
    }

    async fn upload_file(&self, req: SignedRequest, file: &Path, progress: Progress) -> Result<HttpResponse, AwsError> {
        let f = tokio::fs::File::open(file).await.map_err(err)?;
        let size = f.metadata().await.map_err(err)?.len();
        let sent = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let stream = tokio_util::io::ReaderStream::new(f).inspect(move |chunk| {
            if let Ok(c) = chunk {
                let total = sent.fetch_add(c.len() as u64, std::sync::atomic::Ordering::Relaxed) + c.len() as u64;
                progress(total);
            }
        });
        let b = self
            .builder(&req, self.transfer_timeout)?
            .header(reqwest::header::CONTENT_LENGTH, size)
            .body(reqwest::Body::wrap_stream(stream));
        Self::into_response(b.send().await.map_err(err)?).await
    }

    async fn download_to(&self, req: SignedRequest, dest: &Path, progress: Progress) -> Result<HttpResponse, AwsError> {
        let mut r = self.builder(&req, self.transfer_timeout)?.send().await.map_err(err)?;
        if !r.status().is_success() {
            return Self::into_response(r).await;
        }
        let (status, headers) = (r.status().as_u16(), headers_of(&r));
        let mut f = tokio::fs::File::create(dest).await.map_err(err)?;
        let mut total = 0u64;
        let result: Result<(), AwsError> = async {
            while let Some(chunk) = r.chunk().await.map_err(err)? {
                f.write_all(&chunk).await.map_err(err)?;
                total += chunk.len() as u64;
                progress(total);
            }
            f.flush().await.map_err(err)
        }
        .await;
        if let Err(e) = result {
            drop(f);
            let _ = tokio::fs::remove_file(dest).await;
            return Err(e);
        }
        Ok(HttpResponse { status, headers, body: Vec::new() })
    }
}

/// Transporte simulado: responde con un manejador y guarda las peticiones. No abre ninguna conexión.
pub struct FakeTransport {
    handler: Box<dyn Fn(&SignedRequest) -> HttpResponse + Send + Sync>,
    pub requests: std::sync::Mutex<Vec<SignedRequest>>,
}

impl FakeTransport {
    pub fn new(handler: impl Fn(&SignedRequest) -> HttpResponse + Send + Sync + 'static) -> Self {
        FakeTransport { handler: Box::new(handler), requests: Default::default() }
    }

    pub fn recorded(&self) -> Vec<SignedRequest> {
        self.requests.lock().unwrap().clone()
    }

    fn handle(&self, req: &SignedRequest) -> HttpResponse {
        self.requests.lock().unwrap().push(req.clone());
        (self.handler)(req)
    }
}

pub fn response(status: u16, body: &str) -> HttpResponse {
    HttpResponse { status, headers: HashMap::new(), body: body.as_bytes().to_vec() }
}

impl Transport for FakeTransport {
    async fn send(&self, req: SignedRequest) -> Result<HttpResponse, AwsError> {
        Ok(self.handle(&req))
    }

    async fn upload_file(&self, req: SignedRequest, file: &Path, progress: Progress) -> Result<HttpResponse, AwsError> {
        let size = tokio::fs::metadata(file).await.map_err(err)?.len();
        progress(size);
        Ok(self.handle(&req))
    }

    async fn download_to(&self, req: SignedRequest, dest: &Path, progress: Progress) -> Result<HttpResponse, AwsError> {
        let mut r = self.handle(&req);
        if r.is_success() {
            tokio::fs::write(dest, &r.body).await.map_err(err)?;
            progress(r.body.len() as u64);
            r.body.clear();
        }
        Ok(r)
    }
}
