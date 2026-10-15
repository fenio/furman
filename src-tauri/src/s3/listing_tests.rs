use super::service::S3Service;
use aws_sdk_s3::{config::retry::RetryConfig, config::Region, Client, Config};
use aws_smithy_http_client::test_util::{ReplayEvent, StaticReplayClient};
use aws_smithy_types::body::SdkBody;
use tauri::http::{Request, Response};

fn service(pages: &[&str]) -> (S3Service, StaticReplayClient) {
    let http = StaticReplayClient::new(
        pages
            .iter()
            .map(|xml| {
                ReplayEvent::new(
                    Request::builder()
                        .uri("https://listing.test/bucket/")
                        .body(SdkBody::empty())
                        .unwrap(),
                    Response::builder()
                        .status(200)
                        .body(SdkBody::from(xml.to_string()))
                        .unwrap(),
                )
            })
            .collect(),
    );
    let client = Client::from_conf(
        Config::builder()
            .behavior_version_latest()
            .region(Region::new("us-east-1"))
            .credentials_provider(aws_credential_types::Credentials::new(
                "test", "test", None, None, "test",
            ))
            .endpoint_url("https://listing.test")
            .force_path_style(true)
            .retry_config(RetryConfig::disabled())
            .http_client(http.clone())
            .build(),
    );
    (S3Service::new(client, "bucket".into()), http)
}

const FIRST: &str = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>page-two</NextContinuationToken><CommonPrefixes><Prefix>prefix/folder/</Prefix></CommonPrefixes><Contents><Key>prefix/</Key><Size>0</Size></Contents><Contents><Key>prefix/file.txt</Key><Size>7</Size><StorageClass>STANDARD</StorageClass></Contents></ListBucketResult>";
const LAST: &str = "<ListBucketResult><IsTruncated>false</IsTruncated><Contents><Key>prefix/last.bin</Key><Size>3</Size></Contents></ListBucketResult>";

#[tokio::test]
async fn page_returns_without_fetching_the_rest_of_the_prefix() {
    let (service, http) = service(&[FIRST]);
    let page = service.list_objects_page("prefix/", None).await.unwrap();
    assert_eq!(
        page.entries
            .iter()
            .map(|e| e.name.as_str())
            .collect::<Vec<_>>(),
        ["..", "folder", "file.txt"]
    );
    assert_eq!(page.entries[0].path, "s3://bucket/");
    assert!(page.entries[1].is_dir);
    assert_eq!(page.entries[2].extension.as_deref(), Some("txt"));
    assert_eq!(page.entries[2].storage_class.as_deref(), Some("STANDARD"));
    assert_eq!(page.total_size, 7);
    assert_eq!(page.next_token.as_deref(), Some("page-two"));
    let requests: Vec<_> = http.actual_requests().collect();
    assert_eq!(requests.len(), 1);
    let uri = requests[0].uri().to_string();
    assert!(uri.contains("max-keys=1000"));
    assert!(uri.contains("prefix=prefix%2F"));
    assert!(uri.contains("delimiter=%2F"));
}

#[tokio::test]
async fn continuation_pages_do_not_repeat_the_parent_entry() {
    let (service, http) = service(&[LAST]);
    let page = service
        .list_objects_page("prefix/", Some("page-two"))
        .await
        .unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "last.bin");
    assert_eq!(page.total_size, 3);
    assert!(page.next_token.is_none());
    assert!(http
        .actual_requests()
        .next()
        .unwrap()
        .uri()
        .to_string()
        .contains("continuation-token=page-two"));
}

#[tokio::test]
async fn legacy_complete_listing_still_collects_all_pages() {
    let (service, http) = service(&[FIRST, LAST]);
    let listing = service.list_objects("prefix/").await.unwrap();
    assert_eq!(listing.entries.len(), 4);
    assert_eq!(listing.entries.iter().filter(|e| e.name == "..").count(), 1);
    assert_eq!(listing.total_size, 10);
    assert_eq!(http.actual_requests().count(), 2);
}

#[tokio::test]
async fn truncated_pages_require_a_valid_advancing_cursor() {
    for (xml, token) in [
        ("<ListBucketResult><IsTruncated>true</IsTruncated></ListBucketResult>", None),
        ("<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken></NextContinuationToken></ListBucketResult>", None),
        ("<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>same</NextContinuationToken></ListBucketResult>", Some("same")),
    ] {
        let (service, _) = service(&[xml]);
        let error = service.list_objects_page("", token).await.unwrap_err().to_string();
        assert!(error.contains("continuation token"));
    }
}

#[tokio::test]
async fn legacy_listing_rejects_longer_cursor_cycles() {
    let a = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>a</NextContinuationToken></ListBucketResult>";
    let b = "<ListBucketResult><IsTruncated>true</IsTruncated><NextContinuationToken>b</NextContinuationToken></ListBucketResult>";
    let (service, http) = service(&[a, b, a]);
    let error = service.list_objects("").await.err().unwrap().to_string();
    assert!(error.contains("continuation token"));
    assert_eq!(http.actual_requests().count(), 3);
}

#[tokio::test]
async fn empty_bucket_has_one_parent_and_no_continuation() {
    let (service, _) =
        service(&["<ListBucketResult><IsTruncated>false</IsTruncated></ListBucketResult>"]);
    let page = service.list_objects_page("", None).await.unwrap();
    assert_eq!(page.entries.len(), 1);
    assert_eq!(page.entries[0].name, "..");
    assert_eq!(page.total_size, 0);
    assert!(page.next_token.is_none());
}
