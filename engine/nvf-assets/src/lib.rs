//! Bevy asset plugins shared by the viewer and the game: the game's archives (VFS), NIF
//! scene graphs and prepared static models, DDS textures, KF playback (one clip, or layered
//! clips blended by priority).

pub mod actor;
pub mod anim_blend;
pub mod anim_play;
mod bc5_field;
pub mod camera_shot;
pub mod data;
pub mod guard;
pub mod material_anim;
pub mod model;
pub mod model_light;
pub mod morph;
pub mod motion_match;
pub mod particles;
pub mod read_pool;
pub mod release;
pub mod nif_material;
pub mod nif_spawn;
pub mod tangents;
pub mod terrain_sampling;
pub mod texture;

pub use data::{ArchivePlugin, GameData};
pub use particles::{ParticleSystemDef, PreparedParticles};
pub use model::{ModelCache, PreparedModel, PreparedShape};
pub use nif_material::{ambient_occlusion_opt_in, ambient_occlusion_opt_out, depth_equal, fade_prepass, grass_edges, nif_material, soft_edges, NifMaterial, NifMaterialPlugin, AMBIENT_OCCLUSION_DEF};
pub use nif_spawn::{NifHidden, NifRoot, SpawnCtx, SpawnReport};
pub use release::{AssetReleasePlugin, KeepLimits};
pub use texture::TextureCache;

pub mod nif_surface_blend;
