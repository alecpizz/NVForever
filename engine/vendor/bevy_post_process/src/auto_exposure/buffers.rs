use bevy_ecs::{entity::EntityHashMap, prelude::*};
use bevy_platform::collections::hash_map::Entry;
use bevy_render::{
    render_resource::{StorageBuffer, UniformBuffer},
    renderer::{RenderDevice, RenderQueue},
    sync_world::RenderEntity,
    Extract,
};

use super::{pipeline::AutoExposureUniform, AutoExposure};

#[derive(Resource, Default)]
pub(super) struct AutoExposureBuffers {
    pub(super) buffers: EntityHashMap<AutoExposureBuffer>,
}

pub(super) struct AutoExposureBuffer {
    pub(super) state: StorageBuffer<f32>,
    pub(super) settings: UniformBuffer<AutoExposureUniform>,
}

#[derive(Resource)]
pub(super) struct ExtractedStateBuffers {
    changed: Vec<(Entity, AutoExposure)>,
    removed: Vec<Entity>,
}

pub(super) fn extract_buffers(
    mut commands: Commands,
    changed: Extract<Query<(RenderEntity, &AutoExposure), Changed<AutoExposure>>>,
    render_entities: Extract<Query<RenderEntity>>,
    mut removed: Extract<RemovedComponents<AutoExposure>>,
) {
    commands.insert_resource(ExtractedStateBuffers {
        changed: changed
            .iter()
            .map(|(entity, settings)| (entity, settings.clone()))
            .collect(),
        // NVForever (#3917): map main-world Entities to RenderEntities so removal matches
        // `AutoExposureBuffers.buffers` (keyed by RenderEntity).
        removed: removed
            .read()
            .filter_map(|entity| render_entities.get(entity).ok())
            .collect(),
    });
}

pub(super) fn prepare_buffers(
    device: Res<RenderDevice>,
    queue: Res<RenderQueue>,
    mut extracted: ResMut<ExtractedStateBuffers>,
    mut buffers: ResMut<AutoExposureBuffers>,
    active: Query<(), With<AutoExposure>>,
) {
    // NVForever (#3917): remove before inserting `changed` so a same-frame remove+re-add
    // starts with fresh initial state rather than dropping the re-added buffer.
    for entity in extracted.removed.drain(..) {
        buffers.buffers.remove(&entity);
    }

    for (entity, settings) in extracted.changed.drain(..) {
        let (min_log_lum, max_log_lum) = settings.range.into_inner();
        let (low_percent, high_percent) = settings.filter.into_inner();
        let initial_state = 0.0f32.clamp(min_log_lum, max_log_lum);

        let settings = AutoExposureUniform {
            min_log_lum,
            inv_log_lum_range: 1.0 / (max_log_lum - min_log_lum),
            log_lum_range: max_log_lum - min_log_lum,
            low_percent,
            high_percent,
            speed_up: settings.speed_brighten,
            speed_down: settings.speed_darken,
            exponential_transition_distance: settings.exponential_transition_distance,
        };

        match buffers.buffers.entry(entity) {
            Entry::Occupied(mut entry) => {
                // Update the settings buffer, but skip updating the state buffer.
                // The state buffer is skipped so that the animation stays continuous.
                let value = entry.get_mut();
                value.settings.set(settings);
                value.settings.write_buffer(&device, &queue);
            }
            Entry::Vacant(entry) => {
                let value = entry.insert(AutoExposureBuffer {
                    state: StorageBuffer::from(initial_state),
                    settings: UniformBuffer::from(settings),
                });

                value.state.write_buffer(&device, &queue);
                value.settings.write_buffer(&device, &queue);
            }
        }
    }

    // NVForever (#3917): also prune views whose AutoExposure was removed or despawned.
    buffers.buffers.retain(|entity, _| active.contains(*entity));
}
