//! EC2, EC2 Instance Connect y CloudWatch Logs sobre un [`Transport`].

use crate::transport::{HttpResponse, Transport};
use chrono::Utc;
use jterm_core::aws::{self, AwsError, Ec2Instance, FilterParams, LogEvent, LogGroup};
use jterm_core::sigv4::{Credentials, SignedRequest};

pub struct AwsClient<T: Transport> {
    transport: T,
}

impl<T: Transport> AwsClient<T> {
    pub fn new(transport: T) -> Self {
        AwsClient { transport }
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    async fn perform(&self, req: SignedRequest) -> Result<HttpResponse, AwsError> {
        let r = self.transport.send(req).await?;
        if r.is_success() { Ok(r) } else { Err(AwsError(aws::error_message(&r.body, r.status))) }
    }

    /// Todas las instancias de la región (recorre las páginas).
    pub async fn describe_instances(&self, c: &Credentials, region: &str) -> Result<Vec<Ec2Instance>, AwsError> {
        let mut all = Vec::new();
        let mut token: Option<String> = None;
        let mut pages = 0;
        loop {
            let r = self.perform(aws::describe_instances_request(c, region, token.as_deref(), Utc::now())).await?;
            let (mut items, next) = aws::parse_describe_instances(&r.body, region);
            all.append(&mut items);
            pages += 1;
            match next {
                Some(t) if pages < 500 => token = Some(t),
                _ => return Ok(all),
            }
        }
    }

    /// Instancias de varias regiones a la vez. Devuelve las encontradas y los errores (uno por región fallida).
    pub async fn describe_instances_regions(&self, c: &Credentials, regions: &[String]) -> (Vec<Ec2Instance>, Vec<String>) {
        let results = futures_util::future::join_all(regions.iter().map(|r| async move {
            self.describe_instances(c, r).await.map_err(|e| format!("{r}: {}", e.0))
        }))
        .await;
        let (mut all, mut errs) = (Vec::new(), Vec::new());
        for r in results {
            match r {
                Ok(mut v) => all.append(&mut v),
                Err(e) => errs.push(e),
            }
        }
        (all, errs)
    }

    /// EC2 Instance Connect: la llave pública queda válida 60 s en la instancia.
    pub async fn send_ssh_public_key(
        &self, c: &Credentials, region: &str, instance_id: &str, zone: &str, user: &str, public_key: &str,
    ) -> Result<(), AwsError> {
        let req = aws::send_ssh_public_key_request(c, region, instance_id, zone, user, public_key, Utc::now());
        let r = self.perform(req).await?;
        aws::check_ssh_public_key_response(&r.body)
    }

    /// Una página de grupos de logs.
    pub async fn describe_log_groups(
        &self, c: &Credentials, region: &str, next: Option<&str>,
    ) -> Result<(Vec<LogGroup>, Option<String>), AwsError> {
        let r = self.perform(aws::describe_log_groups_request(c, region, next, Utc::now())).await?;
        Ok(aws::parse_log_groups(&r.body))
    }

    /// Una página de eventos (orden cronológico).
    pub async fn filter_log_events(
        &self, c: &Credentials, region: &str, p: &FilterParams<'_>,
    ) -> Result<(Vec<LogEvent>, Option<String>), AwsError> {
        let r = self.perform(aws::filter_log_events_request(c, region, p, Utc::now())).await?;
        Ok(aws::parse_log_events(&r.body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transport::{response, FakeTransport};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    fn creds() -> Credentials {
        Credentials { access_key: "AK".into(), secret: "SK".into() }
    }

    #[tokio::test]
    async fn describe_instances_follows_pages() {
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        let t = FakeTransport::new(move |req| {
            let body = String::from_utf8_lossy(&req.body).to_string();
            if n2.fetch_add(1, Ordering::SeqCst) == 0 {
                assert!(!body.contains("NextToken"));
                response(200, "<DescribeInstancesResponse><reservationSet><item><instancesSet><item><instanceId>i-1</instanceId></item></instancesSet></item></reservationSet><nextToken>T1</nextToken></DescribeInstancesResponse>")
            } else {
                assert!(body.ends_with("&NextToken=T1"), "{body}");
                response(200, "<DescribeInstancesResponse><reservationSet><item><instancesSet><item><instanceId>i-2</instanceId></item></instancesSet></item></reservationSet></DescribeInstancesResponse>")
            }
        });
        let c = AwsClient::new(t);
        let v = c.describe_instances(&creds(), "us-east-1").await.unwrap();
        assert_eq!(v.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(), ["i-1", "i-2"]);
        let reqs = c.transport().recorded();
        assert_eq!(reqs.len(), 2);
        assert_eq!(reqs[0].url, "https://ec2.us-east-1.amazonaws.com/");
    }

    #[tokio::test]
    async fn several_regions_are_combined_and_failures_reported() {
        let t = FakeTransport::new(|req| {
            if req.url.contains("eu-west-1") {
                response(403, r#"{"message":"denegado"}"#)
            } else {
                response(200, "<DescribeInstancesResponse><reservationSet><item><instancesSet><item><instanceId>i-ok</instanceId></item></instancesSet></item></reservationSet></DescribeInstancesResponse>")
            }
        });
        let c = AwsClient::new(t);
        let regions = vec!["us-east-1".to_string(), "eu-west-1".to_string(), "sa-east-1".to_string()];
        let (found, errs) = c.describe_instances_regions(&creds(), &regions).await;
        assert_eq!(found.len(), 2);
        assert_eq!(found.iter().map(|i| i.region.as_str()).collect::<Vec<_>>(), ["us-east-1", "sa-east-1"]);
        assert_eq!(errs, ["eu-west-1: denegado"]);
    }

    #[tokio::test]
    async fn http_errors_become_messages() {
        let t = FakeTransport::new(|_| response(403, r#"{"__type":"x#AccessDeniedException","message":"nope"}"#));
        let c = AwsClient::new(t);
        let e = c.describe_log_groups(&creds(), "us-east-1", None).await.unwrap_err();
        assert_eq!(e.0, "AccessDeniedException: nope");
    }

    #[tokio::test]
    async fn instance_connect_rejection_is_an_error() {
        let ok = AwsClient::new(FakeTransport::new(|_| response(200, r#"{"Success":true}"#)));
        assert!(ok.send_ssh_public_key(&creds(), "us-east-1", "i-1", "us-east-1a", "ec2-user", "ssh-ed25519 AAA").await.is_ok());
        let bad = AwsClient::new(FakeTransport::new(|_| response(200, r#"{"Success":false}"#)));
        assert!(bad.send_ssh_public_key(&creds(), "us-east-1", "i-1", "z", "u", "k").await.is_err());
        let req = &ok.transport().recorded()[0];
        assert_eq!(req.url, "https://ec2-instance-connect.us-east-1.amazonaws.com/");
        assert!(req.headers.iter().any(|(k, v)| k == "x-amz-target" && v.ends_with("SendSSHPublicKey")));
    }

    #[tokio::test]
    async fn logs_pages() {
        let t = FakeTransport::new(|req| {
            if req.headers.iter().any(|(_, v)| v.ends_with("FilterLogEvents")) {
                response(200, r#"{"events":[{"timestamp":5,"logStreamName":"s","message":"m","eventId":"e"}],"nextToken":"n"}"#)
            } else {
                response(200, r#"{"logGroups":[{"logGroupName":"/g"}]}"#)
            }
        });
        let c = AwsClient::new(t);
        let (g, n) = c.describe_log_groups(&creds(), "eu-west-1", None).await.unwrap();
        assert_eq!((g[0].name.as_str(), n), ("/g", None));
        let (e, n) = c.filter_log_events(&creds(), "eu-west-1", &FilterParams { group: "/g", start: 0, ..Default::default() }).await.unwrap();
        assert_eq!((e.len(), n.as_deref()), (1, Some("n")));
    }
}
