use async_trait::async_trait;
use uuid::Uuid;

use hue::api::{GroupedLightUpdate, LightUpdate, ResourceLink, RoomUpdate, Scene, SceneUpdate};
use hue::stream::HueStreamLightsV2;

use crate::error::ApiResult;

/// Backend adapter capabilities.
///
/// This allows backends to opt into optional behavior (e.g. low-latency
/// entertainment streaming), while keeping a single orchestration flow.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct BackendCapabilities {
    pub entertainment_streaming: bool,
    pub scene_management: bool,
    pub room_membership_management: bool,
    pub pairing: bool,
}

/// Backend abstraction layer for non-Hue transports.
///
/// The intent is that Hue/API orchestration can target this trait instead of
/// concrete backend protocol details (e.g. Z2M topics).
#[async_trait]
pub trait BackendAdapter {
    fn capabilities(&self) -> BackendCapabilities;

    async fn light_update(&mut self, link: &ResourceLink, upd: &LightUpdate) -> ApiResult<()>;

    async fn grouped_light_update(
        &mut self,
        link: &ResourceLink,
        upd: &GroupedLightUpdate,
    ) -> ApiResult<()>;

    async fn scene_create(
        &mut self,
        link_scene: &ResourceLink,
        sid: u32,
        scene: &Scene,
    ) -> ApiResult<()>;

    async fn scene_update(&mut self, link: &ResourceLink, upd: &SceneUpdate) -> ApiResult<()>;

    async fn room_update(&mut self, link: &ResourceLink, upd: &RoomUpdate) -> ApiResult<()>;

    async fn delete(&mut self, link: &ResourceLink) -> ApiResult<()>;

    async fn entertainment_start(&mut self, ent_id: &Uuid) -> ApiResult<()>;

    async fn entertainment_frame(&mut self, frame: &HueStreamLightsV2) -> ApiResult<()>;

    async fn entertainment_stop(&mut self) -> ApiResult<()>;

    async fn pairing_start(&mut self, link: &ResourceLink) -> ApiResult<()>;
}
