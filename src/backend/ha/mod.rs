use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use futures::{SinkExt, StreamExt};
use native_tls::TlsConnector;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tokio::sync::broadcast::Receiver;
use tokio::time::Instant;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::{
    Connector, MaybeTlsStream, WebSocketStream, connect_async_tls_with_config,
};

use bifrost_api::config::HomeAssistantConfig;
use hue::api::{
    BridgeHome, ColorTemperature, ColorUpdate, Device, DeviceArchetype, DeviceProductData, Dimming,
    GroupedLight, Light, LightColor, LightGradient, LightGradientMode, LightGradientPoint,
    LightMetadata, Metadata, MirekSchema, On, RType, Resource, Room, RoomArchetype, RoomMetadata,
    ZigbeeConnectivity, ZigbeeConnectivityStatus,
};
use hue::hs::HS;
use hue::xy::XY;
use svc::traits::Service;

use crate::error::{ApiError, ApiResult};
use crate::resource::Resources;

mod import;
mod registry;
mod runtime;
mod types;
mod wled;
mod ws;

use runtime::PendingLightUpdate;
use types::{
    HaArea, HaDeviceRegistryEntry, HaEntityRegistryEntry, HaState, HaStateAttributes,
    HaWsEventMessage,
};
use wled::WledDirectTarget;

pub struct HomeAssistantBackend {
    config: HomeAssistantConfig,
    state: Arc<Mutex<Resources>>,
    socket: Option<WebSocketStream<MaybeTlsStream<TcpStream>>>,
    next_cmd_id: u64,
    pending_light_updates: HashMap<String, PendingLightUpdate>,
    pending_light_batch_deadline: Option<Instant>,
    wled_segment_targets: HashMap<String, Vec<String>>,
    wled_segment_entities: HashSet<String>,
    wled_direct_targets: HashMap<String, WledDirectTarget>,
}

impl HomeAssistantBackend {
    const UPDATE_DEBOUNCE_MS: u64 = 80;
    const DEFAULT_TRANSITION_MS: u32 = 350;

    #[must_use]
    pub fn new(config: HomeAssistantConfig, state: Arc<Mutex<Resources>>) -> Self {
        Self {
            config,
            state,
            socket: None,
            next_cmd_id: 100,
            pending_light_updates: HashMap::new(),
            pending_light_batch_deadline: None,
            wled_segment_targets: HashMap::new(),
            wled_segment_entities: HashSet::new(),
            wled_direct_targets: HashMap::new(),
        }
    }

    fn next_id(&mut self) -> u64 {
        let id = self.next_cmd_id;
        self.next_cmd_id = self.next_cmd_id.saturating_add(1);
        id
    }
}

#[async_trait]
impl Service for HomeAssistantBackend {
    type Error = ApiError;

    async fn start(&mut self) -> ApiResult<()> {
        let url = self.config.get_websocket_url();
        let sanitized = self.config.get_sanitized_websocket_url();

        let connector = if self.config.disable_tls_verify.unwrap_or_default() {
            log::warn!("Home Assistant TLS verification disabled");
            Some(Connector::NativeTls(
                TlsConnector::builder()
                    .danger_accept_invalid_certs(true)
                    .build()?,
            ))
        } else {
            None
        };

        log::info!("Connecting to Home Assistant websocket at {sanitized}");
        let (mut socket, _) =
            connect_async_tls_with_config(url.as_str(), None, false, connector).await?;

        Self::authenticate(&mut socket, &self.config.token).await?;
        Self::subscribe_events(&mut socket).await?;
        match Self::list_areas(&mut socket).await {
            Ok(areas) => {
                self.sync_areas_into_hue(&areas).await?;
                log::info!(
                    "Imported {} Home Assistant area(s) into Hue rooms",
                    areas.len()
                );
            }
            Err(err) => {
                log::warn!("Failed to import Home Assistant areas: {err}");
            }
        }
        let entity_registry = match Self::list_entity_registry(&mut socket).await {
            Ok(rows) => rows,
            Err(err) => {
                log::warn!(
                    "Failed to list Home Assistant entity registry (lights will not be room-assigned): {err}"
                );
                vec![]
            }
        };
        let device_registry = match Self::list_device_registry(&mut socket).await {
            Ok(rows) => rows,
            Err(err) => {
                log::warn!(
                    "Failed to list Home Assistant device registry (lights with only device-level areas will not be room-assigned): {err}"
                );
                vec![]
            }
        };
        match Self::get_states(&mut socket).await {
            Ok(states) => {
                self.wled_segment_targets =
                    Self::compute_wled_segment_targets(&states, &entity_registry);
                self.wled_segment_entities = self
                    .wled_segment_targets
                    .values()
                    .flatten()
                    .cloned()
                    .collect();
                self.wled_direct_targets = Self::compute_wled_direct_targets(&states);
                self.sync_lights_into_hue(&states, &entity_registry, &device_registry)
                    .await?;
                if let Err(err) = self
                    .sync_wled_direct_state_into_hue(&self.wled_direct_targets)
                    .await
                {
                    log::warn!("Failed syncing WLED direct state into Hue: {}", err);
                }
                let lights_count = states
                    .iter()
                    .filter(|s| {
                        s.entity_id.starts_with("light.")
                            && matches!(s.state.as_str(), "on" | "off")
                    })
                    .count();
                log::info!("Imported {lights_count} Home Assistant on/off light(s)");
            }
            Err(err) => log::warn!("Failed to fetch Home Assistant states: {err}"),
        }

        log::info!("Home Assistant websocket authenticated and subscribed");
        self.socket = Some(socket);
        Ok(())
    }

    async fn run(&mut self) -> ApiResult<()> {
        let Some(mut socket) = self.socket.take() else {
            return Err(ApiError::service_error(
                "Home Assistant backend run called before successful start",
            ));
        };

        let mut chan = self.state.lock().await.backend_event_stream();
        self.event_loop(&mut chan, &mut socket).await
    }
}
