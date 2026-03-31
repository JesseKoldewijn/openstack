use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Repository
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Repository {
    pub name: String,
    pub registry_id: String,
    pub arn: String,
    pub uri: String,
    pub created: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Image
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Image {
    pub repository_name: String,
    pub image_digest: String,
    pub image_tags: Vec<String>,
    pub image_manifest: String,
    pub pushed_at: DateTime<Utc>,
    pub size_bytes: u64,
}

// ---------------------------------------------------------------------------
// Store
// ---------------------------------------------------------------------------

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct EcrStore {
    /// repository_name -> Repository
    pub repositories: HashMap<String, Repository>,

    /// image_digest -> Image  (primary index)
    pub images: HashMap<String, Image>,

    /// repository_name -> Vec<digest>  (secondary index: O(1) repo lookup)
    ///
    /// Maintained in sync with `images` by all mutating operations:
    /// `insert_image`, `remove_image`, and `remove_repo_images`.
    /// Never access directly for writes — use the helper methods below.
    pub(crate) repo_index: HashMap<String, Vec<String>>,

    /// (repository_name, tag) -> digest  (tag lookup index: O(1) tag resolution)
    ///
    /// Allows BatchGetImage and BatchDeleteImage to resolve a tag to its
    /// digest in O(1) instead of a full `images` scan.
    pub(crate) tag_index: HashMap<(String, String), String>,
}

impl EcrStore {
    /// Insert an image into the primary store and both secondary indexes.
    pub fn insert_image(&mut self, digest: String, image: Image) {
        let repo = image.repository_name.clone();
        let tags = image.image_tags.clone();

        self.images.insert(digest.clone(), image);

        // repo_index
        self.repo_index
            .entry(repo.clone())
            .or_default()
            .push(digest.clone());

        // tag_index
        for tag in tags {
            self.tag_index.insert((repo.clone(), tag), digest.clone());
        }
    }

    /// Remove a single image by digest, cleaning up all indexes.
    ///
    /// Returns the removed Image if it existed.
    pub fn remove_image(&mut self, digest: &str) -> Option<Image> {
        let image = self.images.remove(digest)?;

        // repo_index: remove digest from the repo's list
        if let Some(digests) = self.repo_index.get_mut(&image.repository_name) {
            digests.retain(|d| d != digest);
            if digests.is_empty() {
                self.repo_index.remove(&image.repository_name);
            }
        }

        // tag_index: remove all (repo, tag) -> digest entries for this image
        for tag in &image.image_tags {
            self.tag_index
                .remove(&(image.repository_name.clone(), tag.clone()));
        }

        Some(image)
    }

    /// Remove all images for a repository (used by DeleteRepository).
    ///
    /// More efficient than calling `remove_image` in a loop because it
    /// drains the repo_index entry in one pass.
    pub fn remove_repo_images(&mut self, repo_name: &str) {
        let digests = match self.repo_index.remove(repo_name) {
            Some(d) => d,
            None => return,
        };
        for digest in &digests {
            if let Some(image) = self.images.remove(digest) {
                for tag in &image.image_tags {
                    self.tag_index.remove(&(repo_name.to_string(), tag.clone()));
                }
            }
        }
    }

    /// Return an iterator over all images belonging to a repository.
    ///
    /// O(k) where k is the number of images in the repo — no scanning of
    /// unrelated repos.
    pub fn images_for_repo<'a>(&'a self, repo_name: &str) -> impl Iterator<Item = &'a Image> + 'a {
        let digests = self
            .repo_index
            .get(repo_name)
            .map(|v| v.as_slice())
            .unwrap_or_default();
        digests
            .iter()
            .filter_map(move |d| self.images.get(d.as_str()))
    }

    /// Resolve a tag to a digest in O(1).
    pub fn digest_for_tag(&self, repo_name: &str, tag: &str) -> Option<&str> {
        self.tag_index
            .get(&(repo_name.to_string(), tag.to_string()))
            .map(|s| s.as_str())
    }
}
