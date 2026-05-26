use super::*;

#[derive(Debug, Clone)]
struct HaLightProfile {
    archetype: DeviceArchetype,
    onoff_only: bool,
    supports_dimming: bool,
    supports_color_temperature: bool,
    supports_color: bool,
    supports_gradient: bool,
}

impl HomeAssistantBackend {
    pub(super) async fn sync_areas_into_hue(&self, areas: &[HaArea]) -> ApiResult<()> {
        let mut res = self.state.lock().await;

        let bridge_home_id = res
            .get_resource_ids_by_type(RType::BridgeHome)
            .into_iter()
            .next()
            .ok_or_else(|| ApiError::service_error("Missing bridge_home resource"))?;
        let link_bridge_home = RType::BridgeHome.link_to(bridge_home_id);

        for area in areas {
            let link_room = RType::Room.deterministic(&area.area_id);
            let link_glight = RType::GroupedLight.deterministic((link_room.rid, &area.area_id));

            if let Ok(existing) = res.get::<Room>(&link_room) {
                if existing.metadata.name != area.name {
                    let new_name = area.name.clone();
                    res.update::<Room>(&link_room.rid, |room| {
                        room.metadata.name = new_name;
                    })?;
                }
            } else {
                let room = Room {
                    children: BTreeSet::new(),
                    metadata: RoomMetadata::new(RoomArchetype::Home, &area.name),
                    services: BTreeSet::from([link_glight]),
                };
                res.add(&link_room, Resource::Room(room))?;

                let glight = GroupedLight::new(link_room);
                res.add(&link_glight, Resource::GroupedLight(glight))?;

                res.update::<BridgeHome>(&link_bridge_home.rid, |bh| {
                    bh.children.insert(link_room);
                })?;
            }
        }

        Ok(())
    }

    fn classify_light_profile(
        state: &HaState,
        entry: Option<&HaEntityRegistryEntry>,
    ) -> HaLightProfile {
        let attrs = &state.attributes;
        let is_wled = attrs.is_wled_like(
            &state.entity_id,
            entry.map(|e| e.platform.as_deref()).flatten(),
        );

        let supports_dimming = attrs.supports_dimming();
        let supports_color_temperature = attrs.supports_color_temperature();
        let supports_color = attrs.supports_color();
        let onoff_only = attrs.is_onoff_only();

        if is_wled {
            return HaLightProfile {
                archetype: DeviceArchetype::HueLightstrip,
                onoff_only: false,
                supports_dimming: true,
                supports_color_temperature,
                supports_color: true,
                supports_gradient: true,
            };
        }

        HaLightProfile {
            archetype: if supports_color {
                DeviceArchetype::ClassicBulb
            } else {
                DeviceArchetype::UnknownArchetype
            },
            onoff_only,
            supports_dimming,
            supports_color_temperature,
            supports_color,
            supports_gradient: false,
        }
    }

    fn apply_capability_profile(
        light: &mut Light,
        attrs: &HaStateAttributes,
        profile: &HaLightProfile,
        is_on: bool,
    ) {
        light.metadata.archetype = profile.archetype.clone();

        if profile.onoff_only {
            light.dimming = None;
            light.color = None;
            light.gradient = None;
            light.color_temperature = None;
            light.color_temperature_delta = None;
            light.dimming_delta = None;
            return;
        }

        if profile.supports_dimming {
            let bri = attrs
                .brightness
                .map(|b| f64::from(b) / 255.0 * 100.0)
                .unwrap_or(if is_on { 100.0 } else { 0.0 });
            light.dimming = Some(Dimming {
                brightness: bri,
                min_dim_level: None,
            });
        } else {
            light.dimming = None;
            light.dimming_delta = None;
        }

        if profile.supports_color_temperature {
            let mirek = attrs.current_mirek();
            light.color_temperature = Some(ColorTemperature {
                mirek,
                mirek_schema: MirekSchema::DEFAULT,
                mirek_valid: mirek.is_some(),
            });
        } else {
            light.color_temperature = None;
            light.color_temperature_delta = None;
        }

        if profile.supports_color {
            let xy = attrs.current_xy().unwrap_or(XY::D65_WHITE_POINT);
            light.color = Some(LightColor::new(xy));
        } else {
            light.color = None;
        }

        if profile.supports_gradient {
            light.gradient = Some(LightGradient {
                mode: LightGradientMode::InterpolatedPalette,
                mode_values: BTreeSet::from([
                    LightGradientMode::InterpolatedPalette,
                    LightGradientMode::InterpolatedPaletteMirrored,
                    LightGradientMode::RandomPixelated,
                ]),
                points_capable: 10,
                points: vec![LightGradientPoint {
                    color: ColorUpdate::new(XY::D65_WHITE_POINT),
                }],
                pixel_count: 60,
            });
        } else {
            light.gradient = None;
        }
    }
    pub(super) async fn sync_lights_into_hue(
        &self,
        states: &[HaState],
        entity_registry: &[HaEntityRegistryEntry],
        device_registry: &[HaDeviceRegistryEntry],
    ) -> ApiResult<()> {
        self.purge_ha_segment_lights().await?;
        let wled_primary = Self::compute_primary_wled_entity_by_device(states, entity_registry);
        self.purge_non_primary_wled_lights(&wled_primary, entity_registry)
            .await?;
        let mut res = self.state.lock().await;

        for state in states {
            if !state.entity_id.starts_with("light.") {
                continue;
            }
            // Do not import per-segment entities as independent Hue lights.
            if Self::parse_segment_index(&state.entity_id).is_some() {
                continue;
            }

            let is_on = match state.state.as_str() {
                "on" => Some(true),
                "off" => Some(false),
                _ => None,
            };
            let Some(is_on) = is_on else {
                continue;
            };

            let friendly_name = state
                .attributes
                .friendly_name
                .as_deref()
                .unwrap_or(&state.entity_id);
            let entity_entry = entity_registry
                .iter()
                .find(|e| e.entity_id == state.entity_id);
            if let Some(entry) = entity_entry
                && let Some(device_id) = &entry.device_id
                && let Some(primary_entity_id) = wled_primary.get(device_id)
                && primary_entity_id != &state.entity_id
            {
                continue;
            }
            let profile = Self::classify_light_profile(state, entity_entry);

            let link_device = RType::Device.deterministic(("ha-device", &state.entity_id));
            let link_light = RType::Light.deterministic(("ha-light", &state.entity_id));
            let link_zbc = RType::ZigbeeConnectivity.deterministic(("ha-zbc", &state.entity_id));

            if let Ok(light) = res.get::<Light>(&link_light) {
                if light.on.on != is_on || light.metadata.name != friendly_name {
                    let new_name = friendly_name.to_string();
                    res.update::<Light>(&link_light.rid, |l| {
                        l.on = On { on: is_on };
                        l.metadata.name = new_name;
                        Self::apply_capability_profile(l, &state.attributes, &profile, is_on);
                    })?;
                } else {
                    // Re-apply capability profile at startup/import time in case
                    // earlier runs imported this light with a less accurate profile.
                    res.update::<Light>(&link_light.rid, |l| {
                        Self::apply_capability_profile(l, &state.attributes, &profile, is_on);
                    })?;
                }
                res.update::<Device>(&link_device.rid, |d| {
                    d.metadata.archetype = profile.archetype.clone();
                    d.product_data.product_archetype = profile.archetype.clone();
                })?;
            } else {
                let dev = Device {
                    product_data: DeviceProductData {
                        model_id: state
                            .attributes
                            .model
                            .clone()
                            .unwrap_or_else(|| "HA-LIGHT".to_string()),
                        manufacturer_name: state
                            .attributes
                            .manufacturer
                            .clone()
                            .unwrap_or_else(|| "Home Assistant".to_string()),
                        product_name: state
                            .attributes
                            .friendly_name
                            .clone()
                            .unwrap_or_else(|| state.entity_id.clone()),
                        product_archetype: profile.archetype.clone(),
                        certified: false,
                        software_version: "1.0".to_string(),
                        hardware_platform_type: None,
                    },
                    metadata: Metadata::new(profile.archetype.clone(), friendly_name),
                    services: BTreeSet::from([link_light, link_zbc]),
                    usertest: None,
                    identify: None,
                };

                let mut light = Light::new(
                    link_device,
                    LightMetadata::new(profile.archetype.clone(), friendly_name),
                );
                light.on = On { on: is_on };
                Self::apply_capability_profile(&mut light, &state.attributes, &profile, is_on);

                let zbc = ZigbeeConnectivity {
                    channel: None,
                    extended_pan_id: None,
                    mac_address: state.entity_id.clone(),
                    owner: link_device,
                    status: ZigbeeConnectivityStatus::Connected,
                };

                res.add(&link_device, Resource::Device(dev))?;
                res.add(&link_light, Resource::Light(light))?;
                res.add(&link_zbc, Resource::ZigbeeConnectivity(zbc))?;
            }
            res.aux_set(
                &link_light,
                crate::model::state::AuxData::new().with_topic(&state.entity_id),
            );

            let area_id = entity_entry.and_then(|e| {
                e.area_id.as_deref().or_else(|| {
                    e.device_id.as_deref().and_then(|dev_id| {
                        device_registry
                            .iter()
                            .find(|d| d.id == dev_id)
                            .and_then(|d| d.area_id.as_deref())
                    })
                })
            });

            if let Some(area_id) = area_id {
                let link_room = RType::Room.deterministic(area_id);
                if res.get::<Room>(&link_room).is_ok() {
                    res.update::<Room>(&link_room.rid, |room| {
                        room.children.insert(link_device);
                    })?;
                }
            }
        }

        Ok(())
    }

    pub(super) async fn purge_non_primary_wled_lights(
        &self,
        wled_primary: &HashMap<String, String>,
        entity_registry: &[HaEntityRegistryEntry],
    ) -> ApiResult<()> {
        let entity_by_id: HashMap<&str, &HaEntityRegistryEntry> = entity_registry
            .iter()
            .map(|entry| (entry.entity_id.as_str(), entry))
            .collect();

        let mut res = self.state.lock().await;
        let light_ids = res.get_resource_ids_by_type(RType::Light);
        let mut owners_to_delete = Vec::new();

        for rid in light_ids {
            let link = RType::Light.link_to(rid);
            let Some(topic) = res.aux_get(&link).ok().and_then(|aux| aux.topic.clone()) else {
                continue;
            };
            let Some(entry) = entity_by_id.get(topic.as_str()) else {
                continue;
            };
            let Some(device_id) = &entry.device_id else {
                continue;
            };
            let Some(primary_entity) = wled_primary.get(device_id) else {
                continue;
            };
            if &topic == primary_entity {
                continue;
            }
            if let Ok(light) = res.get::<Light>(&link) {
                owners_to_delete.push(light.owner);
            }
        }

        for owner in owners_to_delete {
            let _ = res.delete(&owner);
        }
        Ok(())
    }

    pub(super) fn compute_primary_wled_entity_by_device(
        states: &[HaState],
        entity_registry: &[HaEntityRegistryEntry],
    ) -> HashMap<String, String> {
        let entity_by_id: HashMap<&str, &HaEntityRegistryEntry> = entity_registry
            .iter()
            .map(|entry| (entry.entity_id.as_str(), entry))
            .collect();
        let mut by_device: HashMap<String, Vec<String>> = HashMap::new();

        for st in states {
            if !st.entity_id.starts_with("light.") {
                continue;
            }
            let Some(entry) = entity_by_id.get(st.entity_id.as_str()) else {
                continue;
            };
            let Some(device_id) = &entry.device_id else {
                continue;
            };
            if st
                .attributes
                .is_wled_like(&st.entity_id, entry.platform.as_deref())
            {
                by_device
                    .entry(device_id.clone())
                    .or_default()
                    .push(st.entity_id.clone());
            }
        }

        let mut primary = HashMap::new();
        for (device_id, mut entities) in by_device {
            entities.sort_by_key(|eid| {
                let preferred = eid == "light.wled"
                    || eid
                        .strip_prefix("light.wled_")
                        .is_some_and(|s| s.chars().all(|c| c.is_ascii_digit()));
                (
                    if preferred { 0 } else { 1 },
                    if eid.contains("_segment_") { 1 } else { 0 },
                    eid.len(),
                    eid.clone(),
                )
            });
            if let Some(chosen) = entities.first() {
                primary.insert(device_id, chosen.clone());
            }
        }
        primary
    }

    pub(super) async fn purge_ha_segment_lights(&self) -> ApiResult<()> {
        let mut res = self.state.lock().await;
        let light_ids = res.get_resource_ids_by_type(RType::Light);
        let mut owners_to_delete = Vec::new();

        for rid in light_ids {
            let link = RType::Light.link_to(rid);
            let is_segment = res
                .aux_get(&link)
                .ok()
                .and_then(|aux| aux.topic.as_deref())
                .is_some_and(|topic| {
                    topic.starts_with("light.") && Self::parse_segment_index(topic).is_some()
                });
            if !is_segment {
                continue;
            }
            if let Ok(light) = res.get::<Light>(&link) {
                owners_to_delete.push(light.owner);
            }
        }

        for owner in owners_to_delete {
            let _ = res.delete(&owner);
        }
        Ok(())
    }
}
