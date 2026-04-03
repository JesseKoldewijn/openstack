use std::borrow::Cow;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use chrono::Utc;
use openstack_service_framework::traits::{
    DispatchError, DispatchResponse, RequestContext, ResponseBody, ServiceProvider,
};
use openstack_state::AccountRegionBundle;
use serde_json::{Value, json};
use uuid::Uuid;

use crate::store::{EcrStore, Image, Repository};

pub struct EcrProvider {
    store: Arc<AccountRegionBundle<EcrStore>>,
}

impl EcrProvider {
    pub fn new() -> Self {
        Self {
            store: Arc::new(AccountRegionBundle::new()),
        }
    }
}

impl Default for EcrProvider {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Helpers — ECR uses JSON protocol (X-Amz-Target + application/x-amz-json-1.1)
// ---------------------------------------------------------------------------

fn json_ok(body: Value) -> DispatchResponse {
    DispatchResponse {
        status_code: 200,
        body: ResponseBody::Buffered(Bytes::from(serde_json::to_vec(&body).unwrap())),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn json_error(code: &str, message: &str, status: u16) -> DispatchResponse {
    DispatchResponse {
        status_code: status,
        body: ResponseBody::Buffered(Bytes::from(
            serde_json::to_vec(&json!({
                "__type": code,
                "message": message,
            }))
            .unwrap(),
        )),
        content_type: Cow::Borrowed("application/x-amz-json-1.1"),
        headers: Vec::new(),
    }
}

fn str_param(ctx: &RequestContext, key: &str) -> Option<String> {
    ctx.request_body
        .get(key)
        .and_then(|v| v.as_str())
        .map(String::from)
}

fn repo_arn(account_id: &str, region: &str, name: &str) -> String {
    format!("arn:aws:ecr:{region}:{account_id}:repository/{name}")
}

fn repo_uri(account_id: &str, region: &str, name: &str) -> String {
    format!("{account_id}.dkr.ecr.{region}.amazonaws.com/{name}")
}

// ---------------------------------------------------------------------------
// ServiceProvider
// ---------------------------------------------------------------------------

#[async_trait]
impl ServiceProvider for EcrProvider {
    fn service_name(&self) -> &str {
        "ecr"
    }

    async fn dispatch(&self, ctx: &RequestContext) -> Result<DispatchResponse, DispatchError> {
        let region = &ctx.region;
        let account_id = &ctx.account_id;

        match ctx.operation.as_str() {
            // ----------------------------------------------------------------
            // CreateRepository
            // ----------------------------------------------------------------
            "CreateRepository" => {
                let name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let arn = repo_arn(account_id, region, &name);
                let uri = repo_uri(account_id, region, &name);
                let now = Utc::now();
                let repo = Repository {
                    name: name.clone(),
                    registry_id: account_id.clone(),
                    arn: arn.clone(),
                    uri: uri.clone(),
                    created: now,
                };
                let mut store = self.store.get_or_create(account_id, region);
                if store.repositories.contains_key(&name) {
                    return Ok(json_error(
                        "RepositoryAlreadyExistsException",
                        &format!("Repository {name} already exists"),
                        400,
                    ));
                }
                store.repositories.insert(name.clone(), repo);
                Ok(json_ok(json!({
                    "repository": {
                        "repositoryName": name,
                        "repositoryArn": arn,
                        "registryId": account_id,
                        "repositoryUri": uri,
                        "createdAt": now.timestamp(),
                    }
                })))
            }

            // ----------------------------------------------------------------
            // DeleteRepository
            // ----------------------------------------------------------------
            "DeleteRepository" => {
                let name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                match store.repositories.remove(&name) {
                    Some(repo) => {
                        store.remove_repo_images(&name);
                        Ok(json_ok(json!({
                            "repository": {
                                "repositoryName": repo.name,
                                "repositoryArn": repo.arn,
                                "registryId": repo.registry_id,
                                "repositoryUri": repo.uri,
                            }
                        })))
                    }
                    None => Ok(json_error(
                        "RepositoryNotFoundException",
                        &format!("Repository {name} not found"),
                        400,
                    )),
                }
            }

            // ----------------------------------------------------------------
            // DescribeRepositories
            // ----------------------------------------------------------------
            "DescribeRepositories" => {
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "repositories": [] })));
                };
                let repos: Vec<Value> = store
                    .repositories
                    .values()
                    .map(|r| {
                        json!({
                            "repositoryName": r.name,
                            "repositoryArn": r.arn,
                            "registryId": r.registry_id,
                            "repositoryUri": r.uri,
                            "createdAt": r.created.timestamp(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "repositories": repos })))
            }

            // ----------------------------------------------------------------
            // PutImage
            // ----------------------------------------------------------------
            "PutImage" => {
                let repo_name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let image_manifest = str_param(ctx, "imageManifest").unwrap_or_default();
                let image_tag = str_param(ctx, "imageTag");
                let digest = format!("sha256:{}", Uuid::new_v4().to_string().replace('-', ""));

                let mut store = self.store.get_or_create(account_id, region);
                if !store.repositories.contains_key(&repo_name) {
                    return Ok(json_error(
                        "RepositoryNotFoundException",
                        &format!("Repository {repo_name} not found"),
                        400,
                    ));
                }
                let mut tags = Vec::new();
                if let Some(tag) = &image_tag {
                    tags.push(tag.clone());
                }
                let image = Image {
                    repository_name: repo_name.clone(),
                    image_digest: digest.clone(),
                    image_tags: tags.clone(),
                    image_manifest: image_manifest.clone(),
                    pushed_at: Utc::now(),
                    size_bytes: image_manifest.len() as u64,
                };
                store.insert_image(digest.clone(), image);
                Ok(json_ok(json!({
                    "image": {
                        "repositoryName": repo_name,
                        "imageId": {
                            "imageDigest": digest,
                            "imageTag": image_tag,
                        },
                        "imageManifest": image_manifest,
                    }
                })))
            }

            // ----------------------------------------------------------------
            // BatchGetImage
            // ----------------------------------------------------------------
            "BatchGetImage" => {
                let repo_name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "images": [], "failures": [] })));
                };
                let image_ids = ctx
                    .request_body
                    .get("imageIds")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();

                let mut images: Vec<Value> = Vec::new();
                for id in &image_ids {
                    let tag = id.get("imageTag").and_then(|v| v.as_str());
                    let digest = id.get("imageDigest").and_then(|v| v.as_str());

                    // Resolve to a digest using the index — O(1) per lookup.
                    // When both digest and tag are provided, both must match the
                    // same image (unified imageId semantics).
                    let resolved = if let Some(d) = digest {
                        let img = store.images.get(d);
                        // If a tag is also specified, verify it belongs to the same image.
                        if let Some(t) = tag {
                            img.filter(|img| img.image_tags.iter().any(|it| it == t))
                        } else {
                            img
                        }
                    } else if let Some(t) = tag {
                        store
                            .digest_for_tag(&repo_name, t)
                            .and_then(|d| store.images.get(d))
                    } else {
                        None
                    };

                    if let Some(img) = resolved.filter(|img| img.repository_name == repo_name) {
                        images.push(json!({
                            "repositoryName": img.repository_name,
                            "imageId": {
                                "imageDigest": img.image_digest,
                                "imageTag": img.image_tags.first(),
                            },
                            "imageManifest": img.image_manifest,
                        }));
                    }
                }
                Ok(json_ok(json!({ "images": images, "failures": [] })))
            }

            // ----------------------------------------------------------------
            // BatchDeleteImage
            // ----------------------------------------------------------------
            "BatchDeleteImage" => {
                let repo_name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let mut store = self.store.get_or_create(account_id, region);
                // Verify the repository exists before the empty fast-path.
                if !store.repositories.contains_key(repo_name.as_str()) {
                    return Ok(json_error(
                        "RepositoryNotFoundException",
                        &format!("The repository with name '{repo_name}' does not exist"),
                        400,
                    ));
                }
                let image_ids = ctx
                    .request_body
                    .get("imageIds")
                    .and_then(|v| v.as_array())
                    .cloned()
                    .unwrap_or_default();
                if image_ids.is_empty() {
                    return Ok(json_ok(json!({ "imageIds": [], "failures": [] })));
                }

                let mut deleted: Vec<Value> = Vec::new();
                let mut failures: Vec<Value> = Vec::new();

                for id in &image_ids {
                    let tag = id.get("imageTag").and_then(|v| v.as_str());
                    let digest = id.get("imageDigest").and_then(|v| v.as_str());

                    // Resolve digest via index — O(1) for both tag and digest lookups.
                    // Scope to repo_name to prevent cross-repo deletions.
                    let target_digest = if let Some(d) = digest {
                        // Verify the digest exists AND belongs to this repo
                        if store
                            .images
                            .get(d)
                            .is_some_and(|img| img.repository_name == repo_name)
                        {
                            Some(d.to_string())
                        } else {
                            None
                        }
                    } else if let Some(t) = tag {
                        store.digest_for_tag(&repo_name, t).map(String::from)
                    } else {
                        None
                    };

                    match target_digest {
                        Some(d) => {
                            store.remove_image(&d);
                            deleted.push(json!({
                                "imageDigest": d,
                                "imageTag": tag,
                            }));
                        }
                        None => {
                            failures.push(json!({
                                "imageId": {
                                    "imageTag": tag,
                                    "imageDigest": digest,
                                },
                                "failureCode": "ImageNotFoundException",
                                "failureMessage": "Requested image not found",
                            }));
                        }
                    }
                }

                Ok(json_ok(
                    json!({ "imageIds": deleted, "failures": failures }),
                ))
            }

            // ----------------------------------------------------------------
            // DescribeImages
            // ----------------------------------------------------------------
            "DescribeImages" => {
                let repo_name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "imageDetails": [] })));
                };

                // Build a list of imageId filter entries.  Each entry may specify
                // imageDigest, imageTag, or both.  When both are present, an image
                // must satisfy BOTH constraints (unified imageId semantics).
                let id_filters: Vec<(Option<String>, Option<String>)> = ctx
                    .request_body
                    .get("imageIds")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .map(|id| {
                                let d = id
                                    .get("imageDigest")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                let t = id
                                    .get("imageTag")
                                    .and_then(|v| v.as_str())
                                    .map(String::from);
                                (d, t)
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                let image_details: Vec<Value> = store
                    .images_for_repo(&repo_name)
                    .filter(|img| {
                        if id_filters.is_empty() {
                            return true;
                        }
                        // An image matches if it satisfies at least one filter entry.
                        id_filters.iter().any(|(filter_digest, filter_tag)| {
                            let digest_ok = match filter_digest {
                                Some(d) => img.image_digest == *d,
                                None => true,
                            };
                            let tag_ok = match filter_tag {
                                Some(t) => img.image_tags.iter().any(|it| it == t),
                                None => true,
                            };
                            // At least one constraint must be specified, and all
                            // specified constraints must match.
                            (filter_digest.is_some() || filter_tag.is_some())
                                && digest_ok
                                && tag_ok
                        })
                    })
                    .map(|img| {
                        json!({
                            "repositoryName": img.repository_name,
                            "registryId": account_id,
                            "imageDigest": img.image_digest,
                            "imageTags": img.image_tags,
                            "imageSizeInBytes": img.size_bytes,
                            "imagePushedAt": img.pushed_at.timestamp(),
                            "imageManifestMediaType": "application/vnd.docker.distribution.manifest.v2+json",
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "imageDetails": image_details })))
            }

            // ----------------------------------------------------------------
            // ListImages
            // ----------------------------------------------------------------
            "ListImages" => {
                let repo_name = match str_param(ctx, "repositoryName") {
                    Some(n) => n,
                    None => {
                        return Ok(json_error(
                            "InvalidParameterException",
                            "repositoryName required",
                            400,
                        ));
                    }
                };
                let Some(store) = self.store.get(account_id, region) else {
                    return Ok(json_ok(json!({ "imageIds": [] })));
                };
                let image_ids: Vec<Value> = store
                    .images_for_repo(&repo_name)
                    .map(|img| {
                        json!({
                            "imageDigest": img.image_digest,
                            "imageTag": img.image_tags.first(),
                        })
                    })
                    .collect();
                Ok(json_ok(json!({ "imageIds": image_ids })))
            }

            _ => Err(DispatchError::NotImplemented(ctx.operation.clone())),
        }
    }

    async fn storage_snapshot(&self) -> Option<serde_json::Value> {
        use serde_json::json;
        let mut repositories = Vec::new();
        for entry in self.store.iter() {
            let store = entry.value();
            let mut image_counts: std::collections::HashMap<&str, usize> =
                std::collections::HashMap::new();
            for img in store.images.values() {
                *image_counts
                    .entry(img.repository_name.as_str())
                    .or_insert(0) += 1;
            }

            for repo in store.repositories.values() {
                let image_count = image_counts.get(repo.name.as_str()).copied().unwrap_or(0);
                repositories.push(json!({
                    "id": repo.arn.clone(),
                    "kind": "repository",
                    "created_at": repo.created.to_rfc3339(),
                    "attributes": [
                        {"key": "name", "value": repo.name.clone()},
                        {"key": "uri", "value": repo.uri.clone()},
                        {"key": "image_count", "value": image_count.to_string()},
                    ]
                }));
            }
        }
        Some(json!({ "kind": "ecr", "repositories": repositories }))
    }
}
