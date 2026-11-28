//! Bevy asset plugins shared by the viewer and the game: the game's archives (VFS), NIF
//! scene graphs and prepared static models, DDS textures, KF playback (one clip, or layered
//! clips blended by priority).

pub mod actor;
pub mod anim_blend;
pub mod anim_play;
pub mod authored;
mod bc5_field;
pub mod camera_shot;
pub mod data;
pub mod env_map;
pub mod guard;
pub mod load_times;
pub mod material_anim;
pub mod model;
pub mod model_glow;
pub mod model_light;
pub mod morph;
pub mod particles;
mod pbr_field;
pub mod read_pool;
pub mod reflection_cube;
pub mod reflection_room;
pub mod release;
pub mod room_rays;
pub mod nif_material;
pub mod nif_spawn;
pub mod tangents;
pub mod terrain_sampling;
pub mod texture;
pub mod tone_input;

pub use data::{ArchivePlugin, GameData};
pub use particles::{ParticleSystemDef, PreparedParticles};
pub use model::{ModelCache, PreparedModel, PreparedShape};
pub use nif_material::{ambient_occlusion_opt_in, ambient_occlusion_opt_out, depth_equal, enable_foliage_canopy, fade_prepass, grass_edges, nif_material, soft_edges, NifMaterial, NifMaterialPlugin, AMBIENT_OCCLUSION_DEF};
pub use nif_spawn::{NifHidden, NifRoot, SpawnCtx, SpawnReport};
pub use release::{AssetReleasePlugin, KeepLimits};
/// The reflections' mount-pack test aid (pbr_field/reflect.rs).
pub use pbr_field::reflect::{mount_pack_requested, seed_mount_pack, MOUNT_PACK_ENV};
/// The terrain parallax's ground-pack test aid (pbr_field/ground.rs).
pub use pbr_field::ground::{ground_pack_requested, seed_ground_pack, GroundPack, GROUND_DEPTH, GROUND_PACK_ENV};
pub use texture::{TextureCache, ALPHA_HEIGHT_DEPTH};

pub mod nif_surface_blend;
