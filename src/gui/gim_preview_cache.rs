use crate::texture::GimImage;
use anyhow::Result;
use eframe::egui;
use std::time::Instant;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct GimPreviewCacheKey {
    pub stream_index: usize,
    pub pzz_revision: u64,
    pub data_identity: u64,
}

#[derive(Default)]
pub(super) struct GimPreviewCache {
    key: Option<GimPreviewCacheKey>,
    image: Option<GimImage>,
    flat_rgba: Vec<u8>,
    texture: Option<egui::TextureHandle>,
}

impl GimPreviewCache {
    pub(super) fn is_valid_for(&self, key: GimPreviewCacheKey) -> bool {
        self.key == Some(key)
    }

    pub(super) fn ensure_decoded(&mut self, key: GimPreviewCacheKey, data: &[u8]) -> Result<()> {
        if self.is_valid_for(key) {
            return Ok(());
        }

        let started = Instant::now();
        let image = GimImage::decode(data)?;
        let flat_rgba = image.rgba.iter().flat_map(|p| *p).collect::<Vec<_>>();
        eprintln!(
            "[gui] decoded GIM preview stream={} revision={} data=0x{:016X} in {:?}",
            key.stream_index,
            key.pzz_revision,
            key.data_identity,
            started.elapsed()
        );

        self.key = Some(key);
        self.image = Some(image);
        self.flat_rgba = flat_rgba;
        self.texture = None;
        Ok(())
    }

    pub(super) fn image(&self) -> Option<&GimImage> {
        self.image.as_ref()
    }

    pub(super) fn texture_handle(
        &mut self,
        ctx: &egui::Context,
        texture_name: String,
    ) -> Option<egui::TextureHandle> {
        if self.texture.is_none() {
            let image = self.image.as_ref()?;
            let started = Instant::now();
            let color_image = egui::ColorImage::from_rgba_unmultiplied(
                [image.metadata.width, image.metadata.height],
                &self.flat_rgba,
            );
            self.texture =
                Some(ctx.load_texture(texture_name, color_image, egui::TextureOptions::NEAREST));
            eprintln!(
                "[gui] uploaded GIM preview texture {}x{} in {:?}",
                image.metadata.width,
                image.metadata.height,
                started.elapsed()
            );
        }
        self.texture.clone()
    }

    #[cfg(test)]
    fn store_test_key(&mut self, key: GimPreviewCacheKey) {
        self.key = Some(key);
    }
}

pub(super) fn gim_data_identity(data: &[u8]) -> u64 {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in data {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash ^ data.len() as u64
}

#[cfg(test)]
mod tests {
    use super::{GimPreviewCache, GimPreviewCacheKey, gim_data_identity};

    #[test]
    fn gim_preview_cache_key_hits_only_for_same_stream_and_revision() {
        let mut cache = GimPreviewCache::default();
        let key = GimPreviewCacheKey {
            stream_index: 7,
            pzz_revision: 3,
            data_identity: 11,
        };

        assert!(!cache.is_valid_for(key));
        cache.store_test_key(key);

        assert!(cache.is_valid_for(key));
        assert!(!cache.is_valid_for(GimPreviewCacheKey {
            stream_index: 8,
            pzz_revision: 3,
            data_identity: 11,
        }));
        assert!(!cache.is_valid_for(GimPreviewCacheKey {
            stream_index: 7,
            pzz_revision: 4,
            data_identity: 11,
        }));
        assert!(!cache.is_valid_for(GimPreviewCacheKey {
            stream_index: 7,
            pzz_revision: 3,
            data_identity: 12,
        }));
    }

    #[test]
    fn gim_data_identity_changes_when_same_revision_stream_bytes_change() {
        assert_ne!(
            gim_data_identity(b"first gim"),
            gim_data_identity(b"second gim")
        );
    }
}
