//! User-facing diagnostics: never format SDK Debug output, headers, or response bodies.
use aws_sdk_s3::error::{ProvideErrorMetadata, SdkError};

use crate::models::FmError;

pub fn s3_sdk_error<E>(operation: &str, error: &SdkError<E>) -> FmError
where
    E: ProvideErrorMetadata,
{
    let reason = match error {
        SdkError::ResponseError(_) => "The server returned an incomplete or invalid S3 response. A dropped connection, malformed XML, or a proxy/gateway error can cause this.",
        SdkError::TimeoutError(_) => "The request timed out.",
        SdkError::DispatchFailure(context) if context.is_timeout() => "The connection timed out.",
        SdkError::DispatchFailure(_) => "The request could not be completed due to a connection, DNS, TLS, or network failure.",
        SdkError::ConstructionFailure(_) => "The request could not be prepared. Check the connection configuration and credentials.",
        SdkError::ServiceError(_) => "The S3 service rejected the request.",
        _ => "The S3 request failed.",
    };
    let mut details = vec![format!("{operation}: {reason}")];
    if let Some(response) = error.raw_response() {
        details.push(format!("HTTP {}", response.status().as_u16()));
        // Only allowlisted diagnostic headers; never include authorization, URLs,
        // SSE-C headers, or bodies (which may contain object data or proxy pages).
        for header in ["x-amz-request-id", "x-amz-id-2"] {
            if let Some(value) = response.headers().get(header) {
                details.push(format!("{header}: {}", bounded(value)));
            }
        }
    }
    if let Some(service) = error.as_service_error() {
        if let Some(code) = service.code() {
            details.push(format!("Code: {}", bounded(code)));
        }
        if let Some(message) = service.message() {
            details.push(format!("Message: {}", bounded(message)));
        }
    }
    FmError::S3(details.join("\n"))
}

fn bounded(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .take(512)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use aws_sdk_s3::operation::list_objects_v2::ListObjectsV2Error;
    use aws_smithy_runtime_api::http::{Response, StatusCode};
    use aws_smithy_types::{body::SdkBody, error::ErrorMetadata};

    fn response(status: u16) -> Response<SdkBody> {
        Response::new(
            StatusCode::try_from(status).unwrap(),
            SdkBody::from("secret response body"),
        )
    }

    #[test]
    fn malformed_response_includes_status_and_request_id_not_raw_data() {
        let mut raw = response(502);
        raw.headers_mut().insert("x-amz-request-id", "request-123");
        raw.headers_mut()
            .insert("authorization", "secret authorization");
        let error = SdkError::<ListObjectsV2Error>::response_error("secret parser context", raw);
        let message = s3_sdk_error("ListObjectsV2 (page 2)", &error).to_string();
        assert!(message.contains("page 2"));
        assert!(message.contains("incomplete or invalid S3 response"));
        assert!(message.contains("HTTP 502"));
        assert!(message.contains("request-123"));
        assert!(!message.contains("secret"));
    }

    #[test]
    fn service_error_preserves_provider_code_and_message() {
        let metadata = ErrorMetadata::builder()
            .code("AccessDenied")
            .message("Access Denied")
            .build();
        let error = SdkError::service_error(ListObjectsV2Error::generic(metadata), response(403));
        let message = s3_sdk_error("ListObjectsV2", &error).to_string();
        assert!(message.contains("HTTP 403"));
        assert!(message.contains("Code: AccessDenied"));
        assert!(message.contains("Message: Access Denied"));
    }

    #[test]
    fn timeout_is_not_reported_as_a_response_error() {
        let error = SdkError::<ListObjectsV2Error>::timeout_error("secret transport context");
        let message = s3_sdk_error("ListObjectsV2", &error).to_string();
        assert!(message.contains("timed out"));
        assert!(!message.contains("secret"));
        assert!(!message.contains("invalid S3 response"));
    }

    #[tokio::test]
    async fn listing_paths_surface_http_diagnostics() {
        for recursive in [false, true] {
            let mut raw = response(502);
            raw.headers_mut()
                .insert("x-amz-request-id", "listing-request");
            let (http, _) = aws_smithy_http_client::test_util::capture_request(Some(raw.try_into_http1x().unwrap()));
            let client = aws_sdk_s3::Client::from_conf(
                aws_sdk_s3::Config::builder()
                    .behavior_version_latest()
                    .region(aws_sdk_s3::config::Region::new("us-east-1"))
                    .credentials_provider(aws_credential_types::Credentials::new(
                        "test", "test", None, None, "test",
                    ))
                    .retry_config(aws_sdk_s3::config::retry::RetryConfig::disabled())
                    .http_client(http)
                    .build(),
            );
            let error = if recursive {
                crate::s3::helpers::list_all_objects(&client, "bucket", "")
                    .await
                    .err()
                    .unwrap()
            } else {
                crate::s3::service::S3Service::new(client, "bucket".into())
                    .list_objects("")
                    .await
                    .err()
                    .unwrap()
            };
            let message = error.to_string();
            assert!(message.contains("ListObjectsV2"));
            assert!(message.contains("page 1"));
            assert!(message.contains("HTTP 502"));
            assert!(message.contains("listing-request"));
            assert!(!message.contains("secret response body"));
        }
    }
}
