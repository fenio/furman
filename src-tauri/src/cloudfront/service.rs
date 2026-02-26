use aws_sdk_cloudfront::Client as CfClient;
use aws_sdk_cloudfront::types::{
    Aliases, AllowedMethods, CustomErrorResponse as AwsCustomErrorResponse,
    CustomErrorResponses, DefaultCacheBehavior, DistributionConfig, InvalidationBatch,
    Method, Origin, Origins, Paths, ViewerProtocolPolicy,
};
use crate::models::{
    CfCustomErrorResponse, CfDistribution, CfDistributionConfig, CfDistributionSummary,
    CfInvalidation, FmError,
};
use crate::s3::s3err;

/// AWS Managed CachingOptimized cache policy ID.
const CACHING_OPTIMIZED_POLICY: &str = "658327ea-f89d-4fab-a63d-7e88639e58f6";

pub struct CloudFrontService {
    client: CfClient,
    bucket: String,
    region: String,
}

fn format_datetime(dt: &aws_sdk_cloudfront::primitives::DateTime) -> String {
    let secs = dt.secs();
    chrono::DateTime::from_timestamp(secs, 0)
        .map(|dt| dt.to_rfc3339())
        .unwrap_or_default()
}

impl CloudFrontService {
    pub fn new(client: CfClient, bucket: String, region: String) -> Self {
        Self { client, bucket, region }
    }

    fn origin_domain(&self) -> String {
        format!("{}.s3.{}.amazonaws.com", self.bucket, self.region)
    }

    fn origin_id(&self) -> String {
        format!("S3-{}", self.bucket)
    }

    // ── List Distributions ────────────────────────────────────────────────

    pub async fn list_distributions(&self) -> Result<Vec<CfDistributionSummary>, FmError> {
        let origin_domain = self.origin_domain();
        let mut results = Vec::new();
        let mut marker: Option<String> = None;

        loop {
            let mut req = self.client.list_distributions();
            if let Some(m) = &marker {
                req = req.marker(m.as_str());
            }
            let resp = req.send().await.map_err(|e| s3err(format!("CloudFront ListDistributions: {e}")))?;

            if let Some(list) = resp.distribution_list() {
                for item in list.items() {
                    // Filter: only include distributions whose origins reference this bucket
                    let matches = item.origins()
                        .map(|o| o.items().iter().any(|origin| origin.domain_name() == origin_domain))
                        .unwrap_or(false);
                    if !matches {
                        continue;
                    }

                    let aliases: Vec<String> = item
                        .aliases()
                        .map(|a| a.items().iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default();

                    results.push(CfDistributionSummary {
                        id: item.id().to_string(),
                        domain_name: item.domain_name().to_string(),
                        status: item.status().to_string(),
                        enabled: item.enabled(),
                        comment: item.comment().to_string(),
                        last_modified: format_datetime(item.last_modified_time()),
                        price_class: item.price_class().as_str().to_string(),
                        http_version: item.http_version().as_str().to_string(),
                        aliases,
                    });
                }

                if list.is_truncated() {
                    marker = list.next_marker().map(|s| s.to_string());
                    if marker.is_none() {
                        break;
                    }
                } else {
                    break;
                }
            } else {
                break;
            }
        }

        Ok(results)
    }

    // ── Get Distribution ──────────────────────────────────────────────────

    pub async fn get_distribution(&self, id: &str) -> Result<CfDistribution, FmError> {
        let resp = self.client
            .get_distribution()
            .id(id)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront GetDistribution: {e}")))?;

        let etag = resp.e_tag().unwrap_or_default().to_string();
        let dist = resp.distribution().ok_or_else(|| s3err("No distribution in response"))?;
        let config = dist.distribution_config()
            .ok_or_else(|| s3err("No distribution config"))?;

        let aliases: Vec<String> = config
            .aliases()
            .map(|a| a.items().iter().map(|s| s.to_string()).collect())
            .unwrap_or_default();

        let custom_error_responses: Vec<CfCustomErrorResponse> = config
            .custom_error_responses()
            .map(|r| {
                r.items()
                    .iter()
                    .map(|e| CfCustomErrorResponse {
                        error_code: e.error_code(),
                        response_page_path: e.response_page_path().map(|s| s.to_string()),
                        response_code: e.response_code().map(|s| s.to_string()),
                        error_caching_min_ttl: e.error_caching_min_ttl(),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let viewer_policy = config.default_cache_behavior()
            .map(|dcb| dcb.viewer_protocol_policy().as_str().to_string())
            .unwrap_or_else(|| "allow-all".to_string());

        let price_class = config.price_class()
            .map(|pc| pc.as_str().to_string())
            .unwrap_or_else(|| "PriceClass_All".to_string());

        let http_version = config.http_version()
            .map(|hv| hv.as_str().to_string())
            .unwrap_or_else(|| "http2".to_string());

        Ok(CfDistribution {
            id: dist.id().to_string(),
            domain_name: dist.domain_name().to_string(),
            status: dist.status().to_string(),
            etag,
            config: CfDistributionConfig {
                comment: config.comment().to_string(),
                enabled: config.enabled(),
                default_root_object: config.default_root_object().unwrap_or_default().to_string(),
                price_class,
                http_version,
                viewer_protocol_policy: viewer_policy,
                aliases,
                custom_error_responses,
            },
        })
    }

    // ── Create Distribution ───────────────────────────────────────────────

    pub async fn create_distribution(&self, cfg: CfDistributionConfig) -> Result<CfDistribution, FmError> {
        let caller_ref = uuid::Uuid::new_v4().to_string();
        let dist_config = self.build_distribution_config(&caller_ref, &cfg)?;

        let resp = self.client
            .create_distribution()
            .distribution_config(dist_config)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront CreateDistribution: {e}")))?;

        let etag = resp.e_tag().unwrap_or_default().to_string();
        let dist = resp.distribution().ok_or_else(|| s3err("No distribution in response"))?;

        Ok(CfDistribution {
            id: dist.id().to_string(),
            domain_name: dist.domain_name().to_string(),
            status: dist.status().to_string(),
            etag,
            config: cfg,
        })
    }

    // ── Update Distribution ───────────────────────────────────────────────

    pub async fn update_distribution(
        &self,
        id: &str,
        cfg: CfDistributionConfig,
        etag: &str,
    ) -> Result<CfDistribution, FmError> {
        // Fetch existing to preserve CallerReference
        let existing = self.client
            .get_distribution()
            .id(id)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront GetDistribution: {e}")))?;

        let existing_dist = existing.distribution().ok_or_else(|| s3err("No distribution"))?;
        let existing_config = existing_dist.distribution_config()
            .ok_or_else(|| s3err("No distribution config"))?;
        let caller_ref = existing_config.caller_reference().to_string();

        let dist_config = self.build_distribution_config(&caller_ref, &cfg)?;

        let resp = self.client
            .update_distribution()
            .id(id)
            .distribution_config(dist_config)
            .if_match(etag)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront UpdateDistribution: {e}")))?;

        let new_etag = resp.e_tag().unwrap_or_default().to_string();
        let dist = resp.distribution().ok_or_else(|| s3err("No distribution in response"))?;

        Ok(CfDistribution {
            id: dist.id().to_string(),
            domain_name: dist.domain_name().to_string(),
            status: dist.status().to_string(),
            etag: new_etag,
            config: cfg,
        })
    }

    // ── Delete Distribution ───────────────────────────────────────────────

    pub async fn delete_distribution(&self, id: &str, etag: &str) -> Result<(), FmError> {
        self.client
            .delete_distribution()
            .id(id)
            .if_match(etag)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront DeleteDistribution: {e}")))?;
        Ok(())
    }

    // ── Create Invalidation ───────────────────────────────────────────────

    pub async fn create_invalidation(
        &self,
        dist_id: &str,
        paths: Vec<String>,
    ) -> Result<CfInvalidation, FmError> {
        let caller_ref = chrono::Utc::now().timestamp_millis().to_string();
        let quantity = paths.len() as i32;
        let paths_obj = Paths::builder()
            .quantity(quantity)
            .set_items(Some(paths.clone()))
            .build()
            .map_err(|e| s3err(format!("CloudFront build Paths: {e}")))?;

        let batch = InvalidationBatch::builder()
            .paths(paths_obj)
            .caller_reference(&caller_ref)
            .build()
            .map_err(|e| s3err(format!("CloudFront build InvalidationBatch: {e}")))?;

        let resp = self.client
            .create_invalidation()
            .distribution_id(dist_id)
            .invalidation_batch(batch)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront CreateInvalidation: {e}")))?;

        let inv = resp.invalidation().ok_or_else(|| s3err("No invalidation in response"))?;

        let caller_ref_from_batch = inv
            .invalidation_batch()
            .map(|b| b.caller_reference().to_string())
            .unwrap_or_default();

        Ok(CfInvalidation {
            id: inv.id().to_string(),
            status: inv.status().to_string(),
            create_time: caller_ref_from_batch,
            paths,
        })
    }

    // ── List Invalidations ────────────────────────────────────────────────

    pub async fn list_invalidations(&self, dist_id: &str) -> Result<Vec<CfInvalidation>, FmError> {
        let resp = self.client
            .list_invalidations()
            .distribution_id(dist_id)
            .max_items(20)
            .send()
            .await
            .map_err(|e| s3err(format!("CloudFront ListInvalidations: {e}")))?;

        let list = resp.invalidation_list()
            .ok_or_else(|| s3err("No invalidation list in response"))?;

        let mut results = Vec::new();
        for item in list.items() {
            results.push(CfInvalidation {
                id: item.id().to_string(),
                status: item.status().to_string(),
                create_time: format_datetime(item.create_time()),
                paths: Vec::new(), // Summary doesn't include paths
            });
        }

        Ok(results)
    }

    // ── Build DistributionConfig ──────────────────────────────────────────

    fn build_distribution_config(
        &self,
        caller_ref: &str,
        cfg: &CfDistributionConfig,
    ) -> Result<DistributionConfig, FmError> {
        let origin = Origin::builder()
            .domain_name(self.origin_domain())
            .id(self.origin_id())
            .build()
            .map_err(|e| s3err(format!("CloudFront build Origin: {e}")))?;

        let origins = Origins::builder()
            .quantity(1)
            .items(origin)
            .build()
            .map_err(|e| s3err(format!("CloudFront build Origins: {e}")))?;

        let viewer_policy = match cfg.viewer_protocol_policy.as_str() {
            "redirect-to-https" => ViewerProtocolPolicy::RedirectToHttps,
            "https-only" => ViewerProtocolPolicy::HttpsOnly,
            _ => ViewerProtocolPolicy::AllowAll,
        };

        let allowed_methods = AllowedMethods::builder()
            .quantity(2)
            .items(Method::Get)
            .items(Method::Head)
            .build()
            .map_err(|e| s3err(format!("CloudFront build AllowedMethods: {e}")))?;

        let default_cache = DefaultCacheBehavior::builder()
            .target_origin_id(self.origin_id())
            .viewer_protocol_policy(viewer_policy)
            .cache_policy_id(CACHING_OPTIMIZED_POLICY)
            .allowed_methods(allowed_methods)
            .compress(true)
            .build()
            .map_err(|e| s3err(format!("CloudFront build DefaultCacheBehavior: {e}")))?;

        let price_class: aws_sdk_cloudfront::types::PriceClass = cfg.price_class.as_str().into();
        let http_version: aws_sdk_cloudfront::types::HttpVersion = cfg.http_version.as_str().into();

        let mut builder = DistributionConfig::builder()
            .caller_reference(caller_ref)
            .origins(origins)
            .default_cache_behavior(default_cache)
            .comment(&cfg.comment)
            .enabled(cfg.enabled)
            .price_class(price_class)
            .http_version(http_version);

        if !cfg.default_root_object.is_empty() {
            builder = builder.default_root_object(&cfg.default_root_object);
        }

        // Aliases/CNAMEs
        if !cfg.aliases.is_empty() {
            let aliases = Aliases::builder()
                .quantity(cfg.aliases.len() as i32)
                .set_items(Some(cfg.aliases.clone()))
                .build()
                .map_err(|e| s3err(format!("CloudFront build Aliases: {e}")))?;
            builder = builder.aliases(aliases);
        } else {
            let aliases = Aliases::builder()
                .quantity(0)
                .build()
                .map_err(|e| s3err(format!("CloudFront build Aliases: {e}")))?;
            builder = builder.aliases(aliases);
        }

        // Custom error responses
        if !cfg.custom_error_responses.is_empty() {
            let mut items = Vec::new();
            for er in &cfg.custom_error_responses {
                let mut b = AwsCustomErrorResponse::builder()
                    .error_code(er.error_code);
                if let Some(ref path) = er.response_page_path {
                    b = b.response_page_path(path);
                }
                if let Some(ref code) = er.response_code {
                    b = b.response_code(code);
                }
                if let Some(ttl) = er.error_caching_min_ttl {
                    b = b.error_caching_min_ttl(ttl);
                }
                items.push(b.build().map_err(|e| s3err(format!("CloudFront build CustomErrorResponse: {e}")))?);
            }
            let custom_errors = CustomErrorResponses::builder()
                .quantity(items.len() as i32)
                .set_items(Some(items))
                .build()
                .map_err(|e| s3err(format!("CloudFront build CustomErrorResponses: {e}")))?;
            builder = builder.custom_error_responses(custom_errors);
        }

        // Cache behaviors (none — we only use default)
        let cache_behaviors = aws_sdk_cloudfront::types::CacheBehaviors::builder()
            .quantity(0)
            .build()
            .map_err(|e| s3err(format!("CloudFront build CacheBehaviors: {e}")))?;
        builder = builder.cache_behaviors(cache_behaviors);

        builder
            .build()
            .map_err(|e| s3err(format!("CloudFront build DistributionConfig: {e}")))
    }
}
