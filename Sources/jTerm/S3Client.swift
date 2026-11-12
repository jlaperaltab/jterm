import Foundation
import CryptoKit
import UniformTypeIdentifiers

// MARK: - Modelos

struct S3Bucket: Hashable { var name: String; var created: Date? }
struct S3Object: Hashable { var key: String; var size: Int64; var modified: Date? }
struct S3Listing { var prefixes: [String] = []; var objects: [S3Object] = [] }

// MARK: - Firma AWS Signature V4 para S3 (direcciones con la ruta /bucket/clave)

enum S3Signer {
    private static let unreserved = CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~")

    static func encode(_ s: String) -> String { s.addingPercentEncoding(withAllowedCharacters: unreserved) ?? s }
    /// Codifica cada segmento de la ruta una sola vez (S3 no codifica dos veces) y conserva las "/".
    static func encodePath(_ p: String) -> String {
        p.split(separator: "/", omittingEmptySubsequences: false).map { encode(String($0)) }.joined(separator: "/")
    }
    static func canonicalQuery(_ q: [(String, String)]) -> String {
        var pairs: [(k: String, v: String)] = []
        for (k, v) in q { pairs.append((encode(k), encode(v))) }
        pairs.sort { a, b in a.k == b.k ? a.v < b.v : a.k < b.k }
        return pairs.map { $0.k + "=" + $0.v }.joined(separator: "&")
    }

    static let emptyHash = SigV4.hex(SHA256.hash(data: Data()))
    static let unsignedPayload = "UNSIGNED-PAYLOAD"

    private static func hmac(_ key: Data, _ msg: String) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: Data(msg.utf8), using: SymmetricKey(data: key)))
    }
    private static func stamp(_ d: Date) -> String {
        let f = DateFormatter(); f.locale = Locale(identifier: "en_US_POSIX"); f.timeZone = TimeZone(identifier: "UTC")
        f.dateFormat = "yyyyMMdd'T'HHmmss'Z'"; return f.string(from: d)
    }
    private static func signature(_ toSign: String, secret: String, dateStamp: String, region: String) -> String {
        var key = hmac(Data("AWS4\(secret)".utf8), dateStamp)
        for part in [region, "s3", "aws4_request"] { key = hmac(key, part) }
        return SigV4.hex(hmac(key, toSign))
    }
    private static func url(_ scheme: String, _ host: String, _ path: String, _ query: String) -> URL {
        URL(string: "\(scheme)://\(host)\(encodePath(path))" + (query.isEmpty ? "" : "?\(query)"))!
    }

    /// Petición firmada con cabeceras (para listar, subir, bajar, borrar…).
    static func sign(method: String, scheme: String = "https", host: String, path: String, query: [(String, String)] = [],
                     headers: [String: String] = [:], payloadHash: String, region: String, creds: AWSCredentials,
                     date: Date = Date()) -> URLRequest {
        let amz = stamp(date), dateStamp = String(amz.prefix(8))
        var all: [String: String] = [:]
        for (k, v) in headers { all[k.lowercased()] = v.trimmingCharacters(in: .whitespaces) }
        all["host"] = host; all["x-amz-date"] = amz; all["x-amz-content-sha256"] = payloadHash
        let names = all.keys.sorted()
        let canonicalHeaders = names.map { "\($0):\(all[$0]!)\n" }.joined()
        let signed = names.joined(separator: ";")
        let cq = canonicalQuery(query)
        let canonical = [method, encodePath(path.isEmpty ? "/" : path), cq, canonicalHeaders, signed, payloadHash].joined(separator: "\n")
        let scope = "\(dateStamp)/\(region)/s3/aws4_request"
        let toSign = "AWS4-HMAC-SHA256\n\(amz)\n\(scope)\n" + SigV4.hex(SHA256.hash(data: Data(canonical.utf8)))
        let sig = signature(toSign, secret: creds.secret, dateStamp: dateStamp, region: region)

        var req = URLRequest(url: url(scheme, host, path.isEmpty ? "/" : path, cq))
        req.httpMethod = method
        for (k, v) in all where k != "host" { req.setValue(v, forHTTPHeaderField: k) }
        req.setValue("AWS4-HMAC-SHA256 Credential=\(creds.accessKey)/\(scope), SignedHeaders=\(signed), Signature=\(sig)",
                     forHTTPHeaderField: "Authorization")
        req.timeoutInterval = 30
        return req
    }

    /// URL prefirmada (la firma va en la propia URL): quien la tenga puede usarla hasta que expire.
    static func presign(method: String = "GET", scheme: String = "https", host: String, path: String, region: String,
                        creds: AWSCredentials, expires: Int, date: Date = Date()) -> URL {
        let amz = stamp(date), dateStamp = String(amz.prefix(8))
        let scope = "\(dateStamp)/\(region)/s3/aws4_request"
        let q: [(String, String)] = [
            ("X-Amz-Algorithm", "AWS4-HMAC-SHA256"), ("X-Amz-Credential", "\(creds.accessKey)/\(scope)"),
            ("X-Amz-Date", amz), ("X-Amz-Expires", String(expires)), ("X-Amz-SignedHeaders", "host"),
        ]
        let cq = canonicalQuery(q)
        let canonical = [method, encodePath(path), cq, "host:\(host)\n", "host", unsignedPayload].joined(separator: "\n")
        let toSign = "AWS4-HMAC-SHA256\n\(amz)\n\(scope)\n" + SigV4.hex(SHA256.hash(data: Data(canonical.utf8)))
        let sig = signature(toSign, secret: creds.secret, dateStamp: dateStamp, region: region)
        return url(scheme, host, path, cq + "&X-Amz-Signature=" + sig)
    }
}

// MARK: - Lector de las respuestas XML de S3

final class S3XMLParser: NSObject, XMLParserDelegate {
    private var path: [String] = []
    private var text = ""
    private var key = "", size: Int64 = 0, modified: Date?
    private var bucketName = "", bucketDate: Date?
    var listing = S3Listing()
    var buckets: [S3Bucket] = []
    var truncated = false
    var nextToken: String?
    var errorCode: String?, errorMessage: String?

    static func date(_ s: String) -> Date? {
        let f = ISO8601DateFormatter()
        f.formatOptions = [.withInternetDateTime, .withFractionalSeconds]
        if let d = f.date(from: s) { return d }
        f.formatOptions = [.withInternetDateTime]
        return f.date(from: s)
    }

    func parser(_ p: XMLParser, didStartElement el: String, namespaceURI: String?, qualifiedName: String?, attributes: [String: String] = [:]) {
        path.append(el); text = ""
        let full = path.joined(separator: "/")
        if full == "ListBucketResult/Contents" { key = ""; size = 0; modified = nil }
        if full == "ListAllMyBucketsResult/Buckets/Bucket" { bucketName = ""; bucketDate = nil }
    }
    func parser(_ p: XMLParser, foundCharacters s: String) { text += s }
    func parser(_ p: XMLParser, didEndElement el: String, namespaceURI: String?, qualifiedName: String?) {
        defer { path.removeLast(); text = "" }
        let full = path.joined(separator: "/")
        switch full {
        case "ListBucketResult/Contents/Key": key = text
        case "ListBucketResult/Contents/Size": size = Int64(text.trimmingCharacters(in: .whitespaces)) ?? 0
        case "ListBucketResult/Contents/LastModified": modified = Self.date(text.trimmingCharacters(in: .whitespaces))
        case "ListBucketResult/Contents": listing.objects.append(S3Object(key: key, size: size, modified: modified))
        case "ListBucketResult/CommonPrefixes/Prefix": listing.prefixes.append(text)
        case "ListBucketResult/IsTruncated": truncated = text.trimmingCharacters(in: .whitespaces) == "true"
        case "ListBucketResult/NextContinuationToken": nextToken = text.isEmpty ? nil : text
        case "ListAllMyBucketsResult/Buckets/Bucket/Name": bucketName = text
        case "ListAllMyBucketsResult/Buckets/Bucket/CreationDate": bucketDate = Self.date(text.trimmingCharacters(in: .whitespaces))
        case "ListAllMyBucketsResult/Buckets/Bucket": buckets.append(S3Bucket(name: bucketName, created: bucketDate))
        case "Error/Code": errorCode = text
        case "Error/Message": errorMessage = text
        default: break
        }
    }
}

// MARK: - Cliente

/// Cliente mínimo de S3 (sin dependencias). Las credenciales se piden al usarlo, no al crearlo.
final class S3Client {
    /// Destino de las peticiones: (esquema, host) para una región. Las pruebas lo apuntan a un servidor local de mentira.
    static var endpoint: (_ region: String) -> (scheme: String, host: String) = { ("https", "s3.\($0).amazonaws.com") }
    /// Tamaño máximo de una subida simple (límite de S3 para un solo PUT).
    static let maxSingleUpload: Int64 = 5 * 1024 * 1024 * 1024 - 1

    private let credentials: () throws -> AWSCredentials
    private let lock = NSLock()
    private var regions: [String: String] = [:]
    private let defaultRegion: String

    init(defaultRegion: String = "us-east-1", credentials: @escaping () throws -> AWSCredentials) {
        self.defaultRegion = defaultRegion; self.credentials = credentials
    }

    // MARK: Base

    private func send(_ req: URLRequest) async throws -> (Data, HTTPURLResponse) {
        let (data, resp) = try await AWSClient.transport(req)
        guard let http = resp as? HTTPURLResponse else { throw AWSError(message: "Respuesta no válida de S3.") }
        guard (200..<300).contains(http.statusCode) else { throw AWSError(message: Self.message(data, status: http.statusCode)) }
        return (data, http)
    }

    static func message(_ data: Data, status: Int) -> String {
        let p = S3XMLParser()
        let xp = XMLParser(data: data); xp.delegate = p; xp.parse()
        let msg = p.errorMessage ?? "Error HTTP \(status)"
        let full = msg + (p.errorCode.map { " (\($0))" } ?? "")
        if p.errorCode == "AccessDenied" || status == 403 {
            return full + "\n\nPermisos IAM necesarios: s3:ListAllMyBuckets (listar buckets), s3:ListBucket, s3:GetObject, s3:PutObject y s3:DeleteObject."
        }
        return full
    }

    private func request(_ method: String, bucket: String?, key: String = "", query: [(String, String)] = [],
                         headers: [String: String] = [:], payloadHash: String = S3Signer.emptyHash) async throws -> URLRequest {
        let region = bucket == nil ? "us-east-1" : try await region(of: bucket!)
        let ep = Self.endpoint(region)
        let path = bucket == nil ? "/" : "/" + bucket! + (key.isEmpty ? "" : "/" + key)
        return S3Signer.sign(method: method, scheme: ep.scheme, host: ep.host, path: path, query: query, headers: headers,
                             payloadHash: payloadHash, region: region, creds: try credentials())
    }

    /// Región de un bucket (la cabecera x-amz-bucket-region de una consulta HEAD sin firmar), con caché.
    func region(of bucket: String) async throws -> String {
        if let cached = cachedRegion(bucket) { return cached }
        let ep = Self.endpoint("us-east-1")
        var req = URLRequest(url: URL(string: "\(ep.scheme)://\(ep.host)/\(S3Signer.encode(bucket))")!)
        req.httpMethod = "HEAD"; req.timeoutInterval = 20
        let (_, resp) = try await AWSClient.transport(req)
        guard let http = resp as? HTTPURLResponse else { throw AWSError(message: "Respuesta no válida de S3.") }
        if http.statusCode == 404 { throw AWSError(message: "El bucket “\(bucket)” no existe (NoSuchBucket).") }
        let region = http.value(forHTTPHeaderField: "x-amz-bucket-region") ?? defaultRegion
        storeRegion(bucket, region)
        return region
    }

    private func cachedRegion(_ b: String) -> String? { lock.lock(); defer { lock.unlock() }; return regions[b] }
    private func storeRegion(_ b: String, _ r: String) { lock.lock(); defer { lock.unlock() }; regions[b] = r }

    // MARK: Listado

    func buckets() async throws -> [S3Bucket] {
        let (data, _) = try await send(try await request("GET", bucket: nil))
        let p = S3XMLParser(); let xp = XMLParser(data: data); xp.delegate = p; xp.parse()
        return p.buckets.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
    }

    /// Lista un nivel (con delimitador "/") o todo lo que cuelga del prefijo (`recursive`). Recorre todas las páginas.
    func list(bucket: String, prefix: String, recursive: Bool = false) async throws -> S3Listing {
        var all = S3Listing(); var token: String?; var pages = 0
        repeat {
            try Task.checkCancellation()
            var q: [(String, String)] = [("list-type", "2"), ("max-keys", "1000")]
            if !prefix.isEmpty { q.append(("prefix", prefix)) }
            if !recursive { q.append(("delimiter", "/")) }
            if let token { q.append(("continuation-token", token)) }
            let (data, _) = try await send(try await request("GET", bucket: bucket, query: q))
            let p = S3XMLParser(); let xp = XMLParser(data: data); xp.delegate = p; xp.parse()
            all.prefixes += p.listing.prefixes; all.objects += p.listing.objects
            token = p.truncated ? p.nextToken : nil
            pages += 1
        } while token != nil && pages < 500
        return all
    }

    // MARK: Cambios

    func delete(bucket: String, keys: [String]) async throws {
        // De 8 en 8 para no saturar la conexión ni el límite de peticiones.
        for chunk in stride(from: 0, to: keys.count, by: 8).map({ Array(keys[$0..<min($0 + 8, keys.count)]) }) {
            try await withThrowingTaskGroup(of: Void.self) { group in
                for k in chunk {
                    group.addTask { _ = try await self.send(try await self.request("DELETE", bucket: bucket, key: k)) }
                }
                try await group.waitForAll()
            }
        }
    }

    /// Crea una "carpeta" (un objeto vacío cuya clave termina en "/").
    func createFolder(bucket: String, prefix: String) async throws {
        let key = prefix.hasSuffix("/") ? prefix : prefix + "/"
        var req = try await request("PUT", bucket: bucket, key: key)
        req.httpBody = Data()                       // S3 exige Content-Length: 0
        _ = try await send(req)
    }

    /// Copia un objeto dentro del mismo bucket (para renombrar).
    func copy(bucket: String, from: String, to: String) async throws {
        let src = "/" + bucket + "/" + from
        var req = try await request("PUT", bucket: bucket, key: to, headers: ["x-amz-copy-source": S3Signer.encodePath(src)])
        req.httpBody = Data()
        _ = try await send(req)
    }

    /// URL que permite descargar el objeto sin credenciales hasta que expire.
    func presignedURL(bucket: String, key: String, expires: Int = 3600) async throws -> URL {
        let region = try await region(of: bucket)
        let ep = Self.endpoint(region)
        return S3Signer.presign(scheme: ep.scheme, host: ep.host, path: "/" + bucket + "/" + key, region: region,
                                creds: try credentials(), expires: expires)
    }

    // MARK: Transferencias (en streaming: no cargan el archivo en memoria)

    /// Observa el avance de una tarea (bytes enviados o recibidos) y lo informa cada 0,2 s.
    private final class Progress: NSObject, URLSessionTaskDelegate {
        let report: (Int64) -> Void
        private var task: URLSessionTask?
        private var timer: DispatchSourceTimer?
        init(_ report: @escaping (Int64) -> Void) { self.report = report }
        func urlSession(_ s: URLSession, didCreateTask t: URLSessionTask) {
            task = t
            let tm = DispatchSource.makeTimerSource(queue: .global(qos: .utility))
            tm.schedule(deadline: .now() + 0.2, repeating: 0.2)
            tm.setEventHandler { [weak self] in
                guard let t = self?.task else { return }
                self?.report(max(t.countOfBytesSent, t.countOfBytesReceived))
            }
            tm.resume(); timer = tm
        }
        func stop() { timer?.cancel(); timer = nil }
    }

    func upload(file: URL, bucket: String, key: String, progress: @escaping (Int64) -> Void) async throws {
        let size = (try? file.resourceValues(forKeys: [.fileSizeKey]).fileSize).map(Int64.init) ?? 0
        guard size <= Self.maxSingleUpload else {
            throw AWSError(message: "“\(file.lastPathComponent)” supera los 5 GB, el máximo de una subida simple a S3.")
        }
        var req = try await request("PUT", bucket: bucket, key: key, payloadHash: S3Signer.unsignedPayload)
        if let type = UTType(filenameExtension: file.pathExtension)?.preferredMIMEType { req.setValue(type, forHTTPHeaderField: "Content-Type") }
        req.timeoutInterval = 3600
        let tracker = Progress(progress); defer { tracker.stop() }
        let (data, resp) = try await URLSession.shared.upload(for: req, fromFile: file, delegate: tracker)
        guard let http = resp as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            throw AWSError(message: Self.message(data, status: (resp as? HTTPURLResponse)?.statusCode ?? 0))
        }
        progress(size)
    }

    func download(bucket: String, key: String, to dest: URL, progress: @escaping (Int64) -> Void) async throws {
        var req = try await request("GET", bucket: bucket, key: key)
        req.timeoutInterval = 3600
        let tracker = Progress(progress); defer { tracker.stop() }
        let (tmp, resp) = try await URLSession.shared.download(for: req, delegate: tracker)
        guard let http = resp as? HTTPURLResponse, (200..<300).contains(http.statusCode) else {
            let body = (try? Data(contentsOf: tmp)) ?? Data()
            try? FileManager.default.removeItem(at: tmp)
            throw AWSError(message: Self.message(body, status: (resp as? HTTPURLResponse)?.statusCode ?? 0))
        }
        let fm = FileManager.default
        try fm.createDirectory(at: dest.deletingLastPathComponent(), withIntermediateDirectories: true)
        try? fm.removeItem(at: dest)
        try fm.moveItem(at: tmp, to: dest)
        if let n = (try? fm.attributesOfItem(atPath: dest.path)[.size] as? NSNumber)?.int64Value { progress(n) }
    }
}

// MARK: - Rutas de S3 en el explorador

/// El explorador muestra S3 como un árbol: "/" = lista de buckets, "/bucket/carpeta/archivo" = clave "carpeta/archivo".
enum S3Path {
    /// Separa una ruta del explorador en bucket y clave (sin "/" al final). Acepta también "s3://bucket/clave".
    static func parse(_ raw: String) -> (bucket: String?, key: String) {
        var p = raw.trimmingCharacters(in: .whitespaces)
        if p.lowercased().hasPrefix("s3://") { p = "/" + p.dropFirst(5) }
        let comps = p.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
        guard let bucket = comps.first else { return (nil, "") }
        return (bucket, comps.dropFirst().joined(separator: "/"))
    }
    static func path(bucket: String, key: String = "") -> String { key.isEmpty ? "/" + bucket : "/" + bucket + "/" + key }
    /// Prefijo para listar lo que cuelga de una "carpeta" (la clave más una "/").
    static func prefix(_ key: String) -> String { key.isEmpty ? "" : key + "/" }

    /// Destino local seguro para descargar `relative` dentro de `base`: nil si la clave intenta salirse de la carpeta
    /// (".." o rutas absolutas), algo que un bucket ajeno podría usar para escribir fuera del destino elegido.
    static func safeDestination(base: URL, relative: String) -> URL? {
        let parts = relative.split(separator: "/", omittingEmptySubsequences: true).map(String.init)
        guard !parts.isEmpty, !parts.contains(".."), !parts.contains(".") else { return nil }
        var url = base
        for p in parts { url.appendPathComponent(p) }
        let b = base.standardizedFileURL.path, d = url.standardizedFileURL.path
        return d.hasPrefix(b.hasSuffix("/") ? b : b + "/") ? url : nil
    }
}
