//! EC2, EC2 Instance Connect y CloudWatch Logs: construcción de peticiones firmadas y lectura de respuestas.
//! No hace llamadas de red; el cliente HTTP envía los `SignedRequest`.

use crate::models::ServerEnv;
use crate::sigv4::{self, Credentials, SignParams, SignedRequest};
use crate::xml::{self, Ev};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

pub const REGIONS: [&str; 20] = [
    "us-east-1", "us-east-2", "us-west-1", "us-west-2", "sa-east-1", "ca-central-1",
    "eu-west-1", "eu-west-2", "eu-west-3", "eu-central-1", "eu-north-1", "eu-south-1",
    "ap-south-1", "ap-northeast-1", "ap-northeast-2", "ap-northeast-3",
    "ap-southeast-1", "ap-southeast-2", "me-south-1", "af-south-1",
];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AwsAccount {
    #[serde(default = "Uuid::new_v4")]
    pub id: Uuid,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub access_key_id: String,
    #[serde(default = "default_region")]
    pub region: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub env: Option<ServerEnv>,
}

fn default_region() -> String {
    "us-east-1".into()
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ec2Instance {
    pub id: String,
    pub name: String,
    pub state: String,
    pub kind: String,
    pub public_ip: String,
    pub private_ip: String,
    pub public_dns: String,
    pub key_name: String,
    pub zone: String,
    pub region: String,
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{0}")]
pub struct AwsError(pub String);

fn sign_post(host: &str, service: &str, region: &str, headers: &[(&str, &str)], body: Vec<u8>, c: &Credentials, now: DateTime<Utc>) -> SignedRequest {
    let hash = sigv4::sha256_hex(&body);
    sigv4::sign(
        &SignParams {
            method: "POST", scheme: "https", host, canonical_path: "/", canonical_query: "", headers,
            payload_hash: &hash, include_content_sha: false, service, region, creds: c, now,
        },
        body,
    )
}

// MARK: EC2

/// Una página de DescribeInstances.
pub fn describe_instances_request(c: &Credentials, region: &str, token: Option<&str>, now: DateTime<Utc>) -> SignedRequest {
    let mut body = "Action=DescribeInstances&Version=2016-11-15".to_string();
    if let Some(t) = token {
        body.push_str("&NextToken=");
        body.push_str(&sigv4::encode(t));
    }
    sign_post(
        &format!("ec2.{region}.amazonaws.com"), "ec2", region,
        &[("content-type", "application/x-www-form-urlencoded; charset=utf-8")], body.into_bytes(), c, now,
    )
}

const EC2_ROOT: &str = "DescribeInstancesResponse/reservationSet/item/instancesSet/item";

/// Lee una página de DescribeInstances: (instancias, siguiente token).
pub fn parse_describe_instances(data: &[u8], region: &str) -> (Vec<Ec2Instance>, Option<String>) {
    let mut out = Vec::new();
    let mut cur: Option<Ec2Instance> = None;
    let mut tag_key = String::new();
    let mut next = None;
    let tag_item = format!("{EC2_ROOT}/tagSet/item");
    xml::walk(data, |ev, p, t| {
        if ev == Ev::Start {
            if p == EC2_ROOT {
                cur = Some(Ec2Instance { region: region.into(), ..Default::default() });
            }
            if p == tag_item {
                tag_key.clear();
            }
            return;
        }
        {
            let v = t.trim();
            if p == EC2_ROOT {
                if let Some(mut i) = cur.take() {
                    if i.name.is_empty() {
                        i.name = i.id.clone();
                    }
                    out.push(i);
                }
            } else if let (Some(i), Some(rest)) = (cur.as_mut(), p.strip_prefix(&format!("{EC2_ROOT}/"))) {
                match rest {
                    "instanceId" => i.id = v.into(),
                    "instanceState/name" => i.state = v.into(),
                    "instanceType" => i.kind = v.into(),
                    "ipAddress" => i.public_ip = v.into(),
                    "privateIpAddress" => i.private_ip = v.into(),
                    "dnsName" => i.public_dns = v.into(),
                    "keyName" => i.key_name = v.into(),
                    "placement/availabilityZone" => i.zone = v.into(),
                    "tagSet/item/key" => tag_key = v.into(),
                    "tagSet/item/value" if tag_key == "Name" => i.name = v.into(),
                    _ => {}
                }
            } else if p == "DescribeInstancesResponse/nextToken" {
                next = (!v.is_empty()).then(|| v.to_string());
            }
        }
    });
    (out, next)
}

/// EC2 Instance Connect: la llave pública queda válida 60 s en la instancia.
pub fn send_ssh_public_key_request(
    c: &Credentials, region: &str, instance_id: &str, zone: &str, user: &str, public_key: &str, now: DateTime<Utc>,
) -> SignedRequest {
    let body = json!({"InstanceId": instance_id, "InstanceOSUser": user, "SSHPublicKey": public_key, "AvailabilityZone": zone});
    sign_post(
        &format!("ec2-instance-connect.{region}.amazonaws.com"), "ec2-instance-connect", region,
        &[("content-type", "application/x-amz-json-1.1"), ("x-amz-target", "AWSEC2InstanceConnectService.SendSSHPublicKey")],
        body.to_string().into_bytes(), c, now,
    )
}

/// `Err` si AWS contestó 200 pero con `Success: false`.
pub fn check_ssh_public_key_response(data: &[u8]) -> Result<(), AwsError> {
    match serde_json::from_slice::<Value>(data) {
        Ok(v) if v.get("Success") == Some(&Value::Bool(false)) => {
            Err(AwsError("AWS rechazó la llave pública (¿usuario del sistema incorrecto?).".into()))
        }
        _ => Ok(()),
    }
}

/// Mensaje legible de una respuesta de error de AWS (JSON o XML).
pub fn error_message(data: &[u8], status: u16) -> String {
    if let Ok(Value::Object(o)) = serde_json::from_slice::<Value>(data) {
        if let Some(m) = o.get("Message").or_else(|| o.get("message")).and_then(Value::as_str) {
            let t = o.get("__type").and_then(Value::as_str).and_then(|t| t.rsplit('#').next());
            return t.map_or_else(|| m.to_string(), |t| format!("{t}: {m}"));
        }
    }
    let (mut msg, mut code) = (None, None);
    xml::walk(data, |ev, p, t| {
        if ev == Ev::End {
            match p {
                "Response/Errors/Error/Message" => msg = Some(t.trim().to_string()),
                "Response/Errors/Error/Code" => code = Some(t.trim().to_string()),
                _ => {}
            }
        }
    });
    match msg {
        Some(m) => m + &code.map(|c| format!(" ({c})")).unwrap_or_default(),
        None => format!("Error HTTP {status}"),
    }
}

// MARK: CloudWatch Logs

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogGroup {
    pub name: String,
    pub stored_bytes: i64,
    pub retention_days: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LogEvent {
    pub id: String,
    pub stream: String,
    /// Milisegundos desde 1970.
    pub timestamp: i64,
    pub message: String,
}

pub fn logs_request(c: &Credentials, region: &str, target: &str, body: &Value, now: DateTime<Utc>) -> SignedRequest {
    sign_post(
        &format!("logs.{region}.amazonaws.com"), "logs", region,
        &[("content-type", "application/x-amz-json-1.1"), ("x-amz-target", &format!("Logs_20140328.{target}"))],
        body.to_string().into_bytes(), c, now,
    )
}

pub fn describe_log_groups_request(c: &Credentials, region: &str, next: Option<&str>, now: DateTime<Utc>) -> SignedRequest {
    let mut b = json!({"limit": 50});
    if let Some(t) = next {
        b["nextToken"] = t.into();
    }
    logs_request(c, region, "DescribeLogGroups", &b, now)
}

pub fn parse_log_groups(data: &[u8]) -> (Vec<LogGroup>, Option<String>) {
    let v: Value = serde_json::from_slice(data).unwrap_or(Value::Null);
    let groups = v["logGroups"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().filter_map(|g| {
        Some(LogGroup {
            name: g["logGroupName"].as_str()?.into(),
            stored_bytes: g["storedBytes"].as_i64().unwrap_or(0),
            retention_days: g["retentionInDays"].as_i64(),
        })
    });
    (groups.collect(), v["nextToken"].as_str().map(String::from))
}

#[derive(Debug, Clone, Default)]
pub struct FilterParams<'a> {
    pub group: &'a str,
    pub start: i64,
    pub end: Option<i64>,
    pub pattern: &'a str,
    pub stream_prefix: &'a str,
    pub next_token: Option<&'a str>,
    pub limit: u32,
}

pub fn filter_log_events_request(c: &Credentials, region: &str, p: &FilterParams, now: DateTime<Utc>) -> SignedRequest {
    let mut b = json!({"logGroupName": p.group, "startTime": p.start, "limit": if p.limit == 0 { 1000 } else { p.limit }});
    if let Some(e) = p.end {
        b["endTime"] = e.into();
    }
    if !p.pattern.is_empty() {
        b["filterPattern"] = p.pattern.into();
    }
    if !p.stream_prefix.is_empty() {
        b["logStreamNamePrefix"] = p.stream_prefix.into();
    }
    if let Some(t) = p.next_token {
        b["nextToken"] = t.into();
    }
    logs_request(c, region, "FilterLogEvents", &b, now)
}

pub fn parse_log_events(data: &[u8]) -> (Vec<LogEvent>, Option<String>) {
    let v: Value = serde_json::from_slice(data).unwrap_or(Value::Null);
    let events = v["events"].as_array().map(|a| a.as_slice()).unwrap_or(&[]).iter().filter_map(|e| {
        let ts = e["timestamp"].as_i64()?;
        let stream = e["logStreamName"].as_str().unwrap_or("").to_string();
        let message = e["message"].as_str().unwrap_or("").to_string();
        let id = e["eventId"].as_str().map(String::from).unwrap_or_else(|| format!("{stream}|{ts}|{}", message.len()));
        Some(LogEvent { id, stream, timestamp: ts, message })
    });
    (events.collect(), v["nextToken"].as_str().map(String::from))
}

/// Mensaje útil cuando AWS rechaza la llamada por falta de permisos.
pub fn logs_hint(message: &str) -> String {
    let m = message.to_lowercase();
    if m.contains("not authorized") || m.contains("accessdenied") || m.contains("is not authorized to perform") {
        return format!("{message}\n\nPermisos IAM necesarios: logs:DescribeLogGroups y logs:FilterLogEvents.");
    }
    message.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn creds() -> Credentials {
        Credentials { access_key: "AK".into(), secret: "SK".into() }
    }
    fn now() -> DateTime<Utc> {
        Utc.with_ymd_and_hms(2024, 1, 2, 3, 4, 5).unwrap()
    }

    #[test]
    fn describe_instances_parse() {
        let xml = br#"<DescribeInstancesResponse xmlns="http://ec2.amazonaws.com/doc/">
          <reservationSet><item><instancesSet>
            <item><instanceId>i-1</instanceId><instanceState><name>running</name></instanceState>
              <instanceType>t3.micro</instanceType><ipAddress>1.2.3.4</ipAddress><privateIpAddress>10.0.0.1</privateIpAddress>
              <dnsName>ec2-1.compute.amazonaws.com</dnsName><keyName>k</keyName>
              <placement><availabilityZone>us-east-1a</availabilityZone></placement>
              <tagSet><item><key>Env</key><value>x</value></item><item><key>Name</key><value>web</value></item></tagSet></item>
            <item><instanceId>i-2</instanceId></item>
          </instancesSet></item></reservationSet><nextToken>abc</nextToken></DescribeInstancesResponse>"#;
        let (v, next) = parse_describe_instances(xml, "us-east-1");
        assert_eq!(next.as_deref(), Some("abc"));
        assert_eq!(v.len(), 2);
        assert_eq!((v[0].name.as_str(), v[0].state.as_str(), v[0].zone.as_str()), ("web", "running", "us-east-1a"));
        assert_eq!(v[0].public_ip, "1.2.3.4");
        assert_eq!(v[1].name, "i-2");
    }

    #[test]
    fn errors() {
        assert_eq!(error_message(br#"{"__type":"x#AccessDenied","Message":"no"}"#, 403), "AccessDenied: no");
        let xml = b"<Response><Errors><Error><Code>AuthFailure</Code><Message>bad</Message></Error></Errors></Response>";
        assert_eq!(error_message(xml, 401), "bad (AuthFailure)");
        assert_eq!(error_message(b"???", 500), "Error HTTP 500");
    }

    #[test]
    fn requests_are_signed() {
        let r = describe_instances_request(&creds(), "eu-west-1", Some("a b"), now());
        assert_eq!(r.url, "https://ec2.eu-west-1.amazonaws.com/");
        assert_eq!(String::from_utf8(r.body.clone()).unwrap(), "Action=DescribeInstances&Version=2016-11-15&NextToken=a%20b");
        let auth = &r.headers.iter().find(|(k, _)| k == "authorization").unwrap().1;
        assert!(auth.starts_with("AWS4-HMAC-SHA256 Credential=AK/20240102/eu-west-1/ec2/aws4_request"));
        assert!(auth.contains("SignedHeaders=content-type;host;x-amz-date"));
    }

    #[test]
    fn logs_roundtrip() {
        let (g, n) = parse_log_groups(br#"{"logGroups":[{"logGroupName":"/a","storedBytes":5,"retentionInDays":7},{"x":1}],"nextToken":"t"}"#);
        assert_eq!(g, [LogGroup { name: "/a".into(), stored_bytes: 5, retention_days: Some(7) }]);
        assert_eq!(n.as_deref(), Some("t"));
        let (e, n) = parse_log_events(br#"{"events":[{"timestamp":10,"logStreamName":"s","message":"hi","eventId":"E"},{"message":"sin ts"}]}"#);
        assert_eq!(e.len(), 1);
        assert_eq!((e[0].id.as_str(), e[0].timestamp), ("E", 10));
        assert!(n.is_none());
        let r = filter_log_events_request(&creds(), "us-east-1", &FilterParams { group: "g", start: 1, pattern: "ERROR", ..Default::default() }, now());
        let b: Value = serde_json::from_slice(&r.body).unwrap();
        assert_eq!((b["limit"].as_i64(), b["filterPattern"].as_str()), (Some(1000), Some("ERROR")));
        assert!(logs_hint("User is not authorized to perform x").contains("logs:FilterLogEvents"));
    }

    #[test]
    fn instance_connect_rejection() {
        assert!(check_ssh_public_key_response(br#"{"Success":false}"#).is_err());
        assert!(check_ssh_public_key_response(br#"{"Success":true}"#).is_ok());
    }
}
