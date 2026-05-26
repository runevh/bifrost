use super::*;

#[derive(Debug, Deserialize)]
pub(super) struct HaArea {
    pub(super) area_id: String,
    pub(super) name: String,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaEntityRegistryEntry {
    pub(super) entity_id: String,
    pub(super) area_id: Option<String>,
    pub(super) device_id: Option<String>,
    pub(super) platform: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaDeviceRegistryEntry {
    pub(super) id: String,
    pub(super) area_id: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaState {
    pub(super) entity_id: String,
    pub(super) state: String,
    pub(super) attributes: HaStateAttributes,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaWsEventMessage {
    pub(super) event: HaWsEvent,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaWsEvent {
    pub(super) event_type: String,
    pub(super) data: HaWsEventData,
}

#[derive(Debug, Deserialize)]
pub(super) struct HaWsEventData {
    pub(super) new_state: Option<HaState>,
}

#[derive(Debug, Deserialize, Default)]
pub(super) struct HaStateAttributes {
    pub(super) friendly_name: Option<String>,
    pub(super) manufacturer: Option<String>,
    pub(super) model: Option<String>,
    pub(super) brightness: Option<u16>,
    pub(super) color_mode: Option<String>,
    pub(super) supported_color_modes: Option<Vec<String>>,
    pub(super) supported_features: Option<u64>,
    pub(super) min_color_temp_kelvin: Option<u32>,
    pub(super) max_color_temp_kelvin: Option<u32>,
    pub(super) color_temp_kelvin: Option<u32>,
    pub(super) color_temp: Option<u16>,
    pub(super) hs_color: Option<[f64; 2]>,
    pub(super) rgb_color: Option<[u8; 3]>,
    pub(super) xy_color: Option<[f64; 2]>,
}

impl HaStateAttributes {
    pub(super) fn current_xy(&self) -> Option<XY> {
        if let Some([x, y]) = self.xy_color {
            return Some(XY::new(x, y));
        }

        if let Some([h, s]) = self.hs_color {
            let hs = HS {
                hue: (h / 360.0).clamp(0.0, 1.0),
                sat: (s / 100.0).clamp(0.0, 1.0),
            };
            return Some(XY::from_hs(hs).0);
        }

        if let Some([r, g, b]) = self.rgb_color {
            return Some(XY::from_rgb(r, g, b).0);
        }

        None
    }

    pub(super) fn current_mirek(&self) -> Option<u16> {
        let mirek_from_kelvin = self.color_temp_kelvin.and_then(|k| {
            if k == 0 {
                return None;
            }
            Some((1_000_000_u32 / k).clamp(
                MirekSchema::DEFAULT.mirek_minimum,
                MirekSchema::DEFAULT.mirek_maximum,
            ) as u16)
        });

        mirek_from_kelvin.or_else(|| {
            self.color_temp.map(|m| {
                u32::from(m).clamp(
                    MirekSchema::DEFAULT.mirek_minimum,
                    MirekSchema::DEFAULT.mirek_maximum,
                ) as u16
            })
        })
    }

    pub(super) fn is_onoff_only(&self) -> bool {
        // Be conservative: classify as on/off-only only when Home Assistant
        // explicitly reports a single onoff mode.
        self.supported_color_modes
            .as_ref()
            .is_some_and(|modes| modes.len() == 1 && modes[0] == "onoff")
    }

    pub(super) fn supports_color_temperature(&self) -> bool {
        self.supported_color_modes
            .as_ref()
            .is_some_and(|modes| modes.iter().any(|m| m == "color_temp"))
            || self.min_color_temp_kelvin.is_some()
            || self.max_color_temp_kelvin.is_some()
            || self.supported_features.is_some_and(|flags| flags & 2 != 0)
    }

    pub(super) fn supports_color(&self) -> bool {
        self.supported_color_modes.as_ref().is_some_and(|modes| {
            modes
                .iter()
                .any(|m| matches!(m.as_str(), "xy" | "hs" | "rgb" | "rgbw" | "rgbww"))
        }) || self.hs_color.is_some()
            || self.rgb_color.is_some()
            || self.xy_color.is_some()
            || self.supported_features.is_some_and(|flags| flags & 16 != 0)
    }

    pub(super) fn supports_dimming(&self) -> bool {
        self.brightness.is_some()
            || self.color_mode.as_ref().is_some_and(|mode| mode != "onoff")
            || self.supported_features.is_some_and(|flags| flags & 1 != 0)
            || self
                .supported_color_modes
                .as_ref()
                .is_some_and(|modes| modes.iter().any(|m| !matches!(m.as_str(), "onoff")))
    }

    pub(super) fn is_wled_like(&self, entity_id: &str, platform: Option<&str>) -> bool {
        let platform_is_wled = platform.is_some_and(|p| p.eq_ignore_ascii_case("wled"));
        let id_wled_like = entity_id.to_ascii_lowercase().contains("wled");
        let model_wled_like = self
            .model
            .as_deref()
            .is_some_and(|m| m.to_ascii_lowercase().contains("wled"));
        let manufacturer_wled_like = self.manufacturer.as_deref().is_some_and(|m| {
            let lower = m.to_ascii_lowercase();
            lower.contains("wled") || lower.contains("aircookie")
        });

        // Keep this strict to avoid classifying regular bulbs as strips.
        // Generic effect names such as "solid"/"rainbow" are not reliable.
        platform_is_wled || id_wled_like || model_wled_like || manufacturer_wled_like
    }
}
