//! Whether a view may have a volume, asked before anything is made [design] (rtgi-volume.md
//! "Admission").

use crate::sun_visibility::{fullest_view_key, view_counts, world_material_counts, StageCounts};
use bevy::pbr::{MeshPipelineViewLayoutKey, MeshPipelineViewLayouts};
use bevy::render::render_resource::{TextureFormat, TextureFormatFeatureFlags, TextureUsages};
use bevy::render::renderer::{RenderAdapter, RenderDevice};

pub(super) const FORMAT: TextureFormat = TextureFormat::Rgba16Float;
pub(super) const USAGE: TextureUsages = TextureUsages::TEXTURE_BINDING.union(TextureUsages::COPY_DST);

/// The most bytes of a texture. Of one replacement the old one, the new one, its staging copy and the
/// main world's are four times that at the most; that counts the payload, and replacements still in
/// the queue add to it.
pub(super) const MOST_BYTES: u64 = 32 << 20;

/// The mesh bind group's, at the most: a lightmap, the morph targets, the mesh's and the skin's
/// uniforms where they are no storage buffers.
pub(super) const MESH: StageCounts = StageCounts { textures: 2, samplers: 2, uniforms: 4 };

/// The device's, as far as they concern this.
#[derive(Clone, Copy, Debug)]
pub(super) struct Limits {
    pub(super) stage: StageCounts,
    pub(super) side: u32,
    /// [`FORMAT`] can be filtered and has [`USAGE`].
    pub(super) format: bool,
}

impl Limits {
    fn of(device: &RenderDevice, adapter: &RenderAdapter) -> Limits {
        let l = device.limits();
        let features = adapter.get_texture_format_features(FORMAT);
        Limits {
            stage: StageCounts { textures: l.max_sampled_textures_per_shader_stage, samplers: l.max_samplers_per_shader_stage, uniforms: l.max_uniform_buffers_per_shader_stage },
            side: l.max_texture_dimension_3d,
            format: features.flags.contains(TextureFormatFeatureFlags::FILTERABLE) && features.allowed_usages.contains(USAGE),
        }
    }
}

/// The key of the fullest view with a volume: the cloud shadows' ([`fullest_view_key`]), without
/// the light volume's arrays where they are not bound, and with what that one leaves out, the
/// tonemapping's table and the contact shadows' uniform (light_shadow_detail.rs).
pub(super) fn view_key(clouds: bool) -> MeshPipelineViewLayoutKey {
    let key = fullest_view_key() | MeshPipelineViewLayoutKey::TONEMAP_IN_SHADER | MeshPipelineViewLayoutKey::CONTACT_SHADOWS | MeshPipelineViewLayoutKey::NVF_RTGI_VOLUME;
    if clouds {
        key
    } else {
        key.difference(MeshPipelineViewLayoutKey::NVF_CLOUD_VOLUME)
    }
}

/// The fullest pipeline's: the view's ([`view_key`]), the mesh's and the material's with the most, of
/// all that draw in the view, whether they read or not (the layout is theirs too).
pub(super) fn counts(device: &RenderDevice, layouts: &MeshPipelineViewLayouts, clouds: bool) -> StageCounts {
    view_counts(layouts, view_key(clouds)).plus(MESH).plus(world_material_counts(device).0)
}

/// Whether the device has room for a pipeline of `counts` and for a texture of these sides.
pub(super) fn admit(limits: &Limits, counts: StageCounts, atlas: [u32; 3]) -> Result<(), String> {
    for (what, has, limit) in [
        ("sampled textures", counts.textures, limits.stage.textures),
        ("samplers", counts.samplers, limits.stage.samplers),
        ("uniform buffers", counts.uniforms, limits.stage.uniforms),
    ] {
        if has > limit {
            return Err(format!("the fullest pipeline has {has} {what} in its fragment stage, the device allows {limit}"));
        }
    }
    if !limits.format {
        return Err(format!("{FORMAT:?} cannot be filtered or written here"));
    }
    if let Some(side) = atlas.into_iter().find(|s| *s > limits.side) {
        return Err(format!("a texture of {atlas:?} has a side of {side}, the device allows {}", limits.side));
    }
    let bytes = atlas.iter().fold(8u64, |b, s| b.saturating_mul(u64::from(*s)));
    if bytes > MOST_BYTES {
        return Err(format!("a texture of {atlas:?} has {bytes} bytes, {MOST_BYTES} are allowed"));
    }
    Ok(())
}

/// What was asked last, and the answer.
pub(super) struct Asked {
    pub(super) clouds: bool,
    pub(super) atlas: [u32; 3],
    pub(super) granted: Result<(), String>,
}

impl Asked {
    pub(super) fn of(device: &RenderDevice, adapter: &RenderAdapter, layouts: &MeshPipelineViewLayouts, clouds: bool, atlas: [u32; 3]) -> Asked {
        Asked { clouds, atlas, granted: admit(&Limits::of(device, adapter), counts(device, layouts, clouds), atlas) }
    }

    /// Whether the answer still is.
    pub(super) fn is_of(&self, clouds: bool, atlas: [u32; 3]) -> bool {
        (self.clouds, self.atlas) == (clouds, atlas)
    }
}
