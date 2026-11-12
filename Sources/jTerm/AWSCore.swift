import Foundation
import CryptoKit

// MARK: - Modelos

struct AWSAccount: Identifiable, Codable, Hashable {
    var id = UUID()
    var name = ""
    var accessKeyId = ""
    var region = "us-east-1"
}

struct AWSCredentials { let accessKey: String; let secret: String }

struct EC2Instance: Identifiable, Hashable {
    var id = ""
    var name = ""
    var state = ""
    var type = ""
    var publicIP = ""
    var privateIP = ""
    var publicDNS = ""
    var keyName = ""
    var zone = ""
    var region = ""
}

struct AWSError: LocalizedError {
    let message: String
    var errorDescription: String? { message }
}

// MARK: - Firma AWS Signature V4

enum SigV4 {
    static func hex<D: Sequence>(_ d: D) -> String where D.Element == UInt8 {
        d.map { String(format: "%02x", $0) }.joined()
    }
    private static func hmac(_ key: Data, _ msg: String) -> Data {
        Data(HMAC<SHA256>.authenticationCode(for: Data(msg.utf8), using: SymmetricKey(data: key)))
    }

    /// Devuelve la petición firmada. `query` debe venir ya ordenada y codificada.
    static func sign(method: String, url: URL, query: String = "", headers: [String: String], body: Data,
                     service: String, region: String, accessKey: String, secret: String,
                     date: Date = Date()) -> URLRequest {
        let fmt = DateFormatter()
        fmt.locale = Locale(identifier: "en_US_POSIX")
        fmt.timeZone = TimeZone(identifier: "UTC")
        fmt.dateFormat = "yyyyMMdd'T'HHmmss'Z'"
        let amzDate = fmt.string(from: date)
        let dateStamp = String(amzDate.prefix(8))

        var all = headers
        all["host"] = url.host ?? ""
        all["x-amz-date"] = amzDate
        var lowered: [String: String] = [:]
        for (k, v) in all { lowered[k.lowercased()] = v.trimmingCharacters(in: .whitespaces) }
        let names = lowered.keys.sorted()
        let canonicalHeaders = names.map { "\($0):\(lowered[$0]!)\n" }.joined()
        let signedHeaders = names.joined(separator: ";")
        let payloadHash = hex(SHA256.hash(data: body))
        let path = url.path.isEmpty ? "/" : url.path
        let canonical = [method, path, query, canonicalHeaders, signedHeaders, payloadHash].joined(separator: "\n")

        let scope = "\(dateStamp)/\(region)/\(service)/aws4_request"
        let toSign = "AWS4-HMAC-SHA256\n\(amzDate)\n\(scope)\n" + hex(SHA256.hash(data: Data(canonical.utf8)))
        var key = hmac(Data("AWS4\(secret)".utf8), dateStamp)
        for part in [region, service, "aws4_request"] { key = hmac(key, part) }
        let signature = hex(hmac(key, toSign))

        var comps = URLComponents(url: url, resolvingAgainstBaseURL: false)!
        if !query.isEmpty { comps.percentEncodedQuery = query }
        var req = URLRequest(url: comps.url!)
        req.httpMethod = method
        req.httpBody = body.isEmpty ? nil : body
        for (k, v) in lowered where k != "host" { req.setValue(v, forHTTPHeaderField: k) }
        req.setValue("AWS4-HMAC-SHA256 Credential=\(accessKey)/\(scope), SignedHeaders=\(signedHeaders), Signature=\(signature)",
                     forHTTPHeaderField: "Authorization")
        req.timeoutInterval = 20
        return req
    }
}

// MARK: - Parser de DescribeInstances (XML)

final class EC2Parser: NSObject, XMLParserDelegate {
    private var path: [String] = []
    private var text = ""
    private var cur: EC2Instance?
    private var tagKey = ""
    private let root = "DescribeInstancesResponse/reservationSet/item/instancesSet/item"
    let region: String
    var instances: [EC2Instance] = []
    var nextToken: String?
    var errorMessage: String?
    var errorCode: String?

    init(region: String) { self.region = region }

    func parser(_ p: XMLParser, didStartElement el: String, namespaceURI: String?, qualifiedName: String?,
                attributes: [String: String] = [:]) {
        path.append(el); text = ""
        let full = path.joined(separator: "/")
        if full == root { cur = EC2Instance(region: region) }
        if full == root + "/tagSet/item" { tagKey = "" }
    }
    func parser(_ p: XMLParser, foundCharacters s: String) { text += s }
    func parser(_ p: XMLParser, didEndElement el: String, namespaceURI: String?, qualifiedName: String?) {
        defer { path.removeLast(); text = "" }
        let full = path.joined(separator: "/")
        let value = text.trimmingCharacters(in: .whitespacesAndNewlines)
        if full == root, var i = cur {
            if i.name.isEmpty { i.name = i.id }
            instances.append(i); cur = nil; return
        }
        if cur != nil, full.hasPrefix(root + "/") {
            switch String(full.dropFirst(root.count + 1)) {
            case "instanceId": cur!.id = value
            case "instanceState/name": cur!.state = value
            case "instanceType": cur!.type = value
            case "ipAddress": cur!.publicIP = value
            case "privateIpAddress": cur!.privateIP = value
            case "dnsName": cur!.publicDNS = value
            case "keyName": cur!.keyName = value
            case "placement/availabilityZone": cur!.zone = value
            case "tagSet/item/key": tagKey = value
            case "tagSet/item/value": if tagKey == "Name" { cur!.name = value }
            default: break
            }
        } else if full == "DescribeInstancesResponse/nextToken" {
            nextToken = value.isEmpty ? nil : value
        } else if full == "Response/Errors/Error/Message" {
            errorMessage = value
        } else if full == "Response/Errors/Error/Code" {
            errorCode = value
        }
    }
}

// MARK: - Cliente

enum AWSClient {
    static let regions = [
        "us-east-1", "us-east-2", "us-west-1", "us-west-2", "sa-east-1", "ca-central-1",
        "eu-west-1", "eu-west-2", "eu-west-3", "eu-central-1", "eu-north-1", "eu-south-1",
        "ap-south-1", "ap-northeast-1", "ap-northeast-2", "ap-northeast-3",
        "ap-southeast-1", "ap-southeast-2", "me-south-1", "af-south-1",
    ]

    private static func encode(_ s: String) -> String {
        s.addingPercentEncoding(withAllowedCharacters: CharacterSet(charactersIn: "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_.~")) ?? s
    }

    /// Salida de red. Las pruebas la sustituyen por una simulación para no hablar nunca con AWS.
    static var transport: (URLRequest) async throws -> (Data, URLResponse) = { try await URLSession.shared.data(for: $0) }

    private static func perform(_ req: URLRequest) async throws -> Data {
        let (data, resp) = try await transport(req)
        let code = (resp as? HTTPURLResponse)?.statusCode ?? 0
        guard (200..<300).contains(code) else { throw AWSError(message: errorMessage(data, status: code)) }
        return data
    }

    static func errorMessage(_ data: Data, status: Int) -> String {
        if let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any],
           let m = (obj["Message"] ?? obj["message"]) as? String {
            let t = (obj["__type"] as? String)?.split(separator: "#").last.map(String.init)
            return t.map { "\($0): \(m)" } ?? m
        }
        let p = EC2Parser(region: "")
        let xp = XMLParser(data: data); xp.delegate = p; xp.parse()
        if let m = p.errorMessage { return m + (p.errorCode.map { " (\($0))" } ?? "") }
        return "Error HTTP \(status)"
    }

    static func describeInstances(_ c: AWSCredentials, region: String) async throws -> [EC2Instance] {
        let url = URL(string: "https://ec2.\(region).amazonaws.com/")!
        var all: [EC2Instance] = []
        var token: String?
        repeat {
            var body = "Action=DescribeInstances&Version=2016-11-15"
            if let token { body += "&NextToken=" + encode(token) }
            let req = SigV4.sign(method: "POST", url: url,
                                 headers: ["content-type": "application/x-www-form-urlencoded; charset=utf-8"],
                                 body: Data(body.utf8), service: "ec2", region: region,
                                 accessKey: c.accessKey, secret: c.secret)
            let data = try await perform(req)
            let p = EC2Parser(region: region)
            let xp = XMLParser(data: data); xp.delegate = p; xp.parse()
            all += p.instances
            token = p.nextToken
        } while token != nil
        return all
    }

    /// EC2 Instance Connect: la llave pública queda válida 60 s en la instancia.
    static func sendSSHPublicKey(_ c: AWSCredentials, region: String, instanceId: String, zone: String,
                                 user: String, publicKey: String) async throws {
        let url = URL(string: "https://ec2-instance-connect.\(region).amazonaws.com/")!
        let body = try JSONSerialization.data(withJSONObject: [
            "InstanceId": instanceId, "InstanceOSUser": user,
            "SSHPublicKey": publicKey, "AvailabilityZone": zone,
        ])
        let req = SigV4.sign(method: "POST", url: url,
                             headers: ["content-type": "application/x-amz-json-1.1",
                                       "x-amz-target": "AWSEC2InstanceConnectService.SendSSHPublicKey"],
                             body: body, service: "ec2-instance-connect", region: region,
                             accessKey: c.accessKey, secret: c.secret)
        let data = try await perform(req)
        if let obj = try? JSONSerialization.jsonObject(with: data) as? [String: Any], obj["Success"] as? Bool == false {
            throw AWSError(message: "AWS rechazó la llave pública (¿usuario del sistema incorrecto?).")
        }
    }
}


// MARK: - CloudWatch Logs

struct LogGroup: Hashable {
    var name: String
    var storedBytes: Int64 = 0
    var retentionDays: Int? = nil
}

struct LogEvent: Hashable {
    var id: String
    var stream: String
    var timestamp: Int64      // milisegundos desde 1970
    var message: String
}

extension AWSClient {
    /// Llamada JSON 1.1 a CloudWatch Logs.
    static func logsCall(_ c: AWSCredentials, region: String, target: String, body: [String: Any]) async throws -> [String: Any] {
        let url = URL(string: "https://logs.\(region).amazonaws.com/")!
        let data = try JSONSerialization.data(withJSONObject: body)
        let req = SigV4.sign(method: "POST", url: url,
                             headers: ["content-type": "application/x-amz-json-1.1", "x-amz-target": "Logs_20140328.\(target)"],
                             body: data, service: "logs", region: region, accessKey: c.accessKey, secret: c.secret)
        let out = try await perform(req)
        return (try JSONSerialization.jsonObject(with: out) as? [String: Any]) ?? [:]
    }

    /// Una página de grupos de logs.
    static func describeLogGroups(_ c: AWSCredentials, region: String, nextToken: String?) async throws -> (groups: [LogGroup], nextToken: String?) {
        var body: [String: Any] = ["limit": 50]
        if let nextToken { body["nextToken"] = nextToken }
        let r = try await logsCall(c, region: region, target: "DescribeLogGroups", body: body)
        let groups = (r["logGroups"] as? [[String: Any]] ?? []).compactMap { g -> LogGroup? in
            guard let name = g["logGroupName"] as? String else { return nil }
            return LogGroup(name: name, storedBytes: (g["storedBytes"] as? NSNumber)?.int64Value ?? 0,
                            retentionDays: (g["retentionInDays"] as? NSNumber)?.intValue)
        }
        return (groups, r["nextToken"] as? String)
    }

    /// Una página de eventos (orden cronológico). `start`/`end` en milisegundos.
    static func filterLogEvents(_ c: AWSCredentials, region: String, group: String, start: Int64, end: Int64? = nil,
                                pattern: String = "", streamPrefix: String = "", nextToken: String? = nil,
                                limit: Int = 1000) async throws -> (events: [LogEvent], nextToken: String?) {
        var body: [String: Any] = ["logGroupName": group, "startTime": start, "limit": limit]
        if let end { body["endTime"] = end }
        if !pattern.isEmpty { body["filterPattern"] = pattern }
        if !streamPrefix.isEmpty { body["logStreamNamePrefix"] = streamPrefix }
        if let nextToken { body["nextToken"] = nextToken }
        let r = try await logsCall(c, region: region, target: "FilterLogEvents", body: body)
        let events = (r["events"] as? [[String: Any]] ?? []).compactMap { e -> LogEvent? in
            guard let ts = (e["timestamp"] as? NSNumber)?.int64Value else { return nil }
            let stream = e["logStreamName"] as? String ?? ""
            let msg = e["message"] as? String ?? ""
            return LogEvent(id: e["eventId"] as? String ?? "\(stream)|\(ts)|\(msg.hashValue)", stream: stream, timestamp: ts, message: msg)
        }
        return (events, r["nextToken"] as? String)
    }

    /// Mensaje útil cuando AWS rechaza la llamada por falta de permisos.
    static func logsHint(for message: String) -> String {
        let m = message.lowercased()
        if m.contains("not authorized") || m.contains("accessdenied") || m.contains("is not authorized to perform") {
            return message + "\n\nPermisos IAM necesarios: logs:DescribeLogGroups y logs:FilterLogEvents."
        }
        return message
    }
}
