use super::*;

#[derive(Debug, Clone)]
pub(super) struct WledDirectTarget {
    pub(super) ip: String,
    pub(super) led_count: u32,
}

#[derive(Debug, Deserialize)]
struct WledStateResponse {
    on: bool,
    bri: Option<u32>,
}

impl HomeAssistantBackend {
    pub(super) fn parse_segment_index(entity_id: &str) -> Option<u32> {
        let (_, suffix) = entity_id.rsplit_once("_segment_")?;
        suffix.parse::<u32>().ok()
    }

    pub(super) fn compute_wled_segment_targets(
        states: &[HaState],
        entity_registry: &[HaEntityRegistryEntry],
    ) -> HashMap<String, Vec<String>> {
        let entity_by_id: HashMap<&str, &HaEntityRegistryEntry> = entity_registry
            .iter()
            .map(|entry| (entry.entity_id.as_str(), entry))
            .collect();

        let mut by_device: HashMap<String, Vec<String>> = HashMap::new();
        for state in states {
            if !state.entity_id.starts_with("light.") {
                continue;
            }
            let Some(entry) = entity_by_id.get(state.entity_id.as_str()) else {
                continue;
            };
            let Some(device_id) = &entry.device_id else {
                continue;
            };
            let is_wled = state
                .attributes
                .is_wled_like(&state.entity_id, entry.platform.as_deref());
            if !is_wled {
                continue;
            }
            by_device
                .entry(device_id.clone())
                .or_default()
                .push(state.entity_id.clone());
        }

        let mut map = HashMap::new();
        for entities in by_device.values_mut() {
            entities.sort_by(|a, b| {
                let ka = (
                    Self::parse_segment_index(a).is_some(),
                    Self::parse_segment_index(a).unwrap_or(0),
                    a,
                );
                let kb = (
                    Self::parse_segment_index(b).is_some(),
                    Self::parse_segment_index(b).unwrap_or(0),
                    b,
                );
                ka.cmp(&kb)
            });
            let targets = entities.iter().take(3).cloned().collect::<Vec<_>>();
            for entity_id in entities.iter() {
                map.insert(entity_id.clone(), targets.clone());
            }
        }
        map
    }

    pub(super) fn parse_wled_suffix(entity_id: &str) -> Option<String> {
        let eid = entity_id.strip_prefix("light.")?;
        if eid == "wled" {
            return Some(String::new());
        }
        eid.strip_prefix("wled_").map(|s| format!("_{s}"))
    }

    pub(super) fn compute_wled_direct_targets(
        states: &[HaState],
    ) -> HashMap<String, WledDirectTarget> {
        let mut ip_by_suffix: HashMap<String, String> = HashMap::new();
        let mut leds_by_suffix: HashMap<String, u32> = HashMap::new();

        for st in states {
            if let Some(suffix) = st.entity_id.strip_prefix("sensor.wled_ip") {
                let ip = st.state.trim();
                if ip.parse::<IpAddr>().is_ok() {
                    ip_by_suffix.insert(suffix.to_string(), ip.to_string());
                }
            }
            if let Some(suffix) = st.entity_id.strip_prefix("sensor.wled_led_count")
                && let Ok(v) = st.state.parse::<u32>()
            {
                leds_by_suffix.insert(suffix.to_string(), v);
            }
        }

        let mut targets = HashMap::new();
        for st in states {
            if !st.entity_id.starts_with("light.wled") {
                continue;
            }
            if st.state == "unavailable" {
                continue;
            }
            let Some(suffix) = Self::parse_wled_suffix(&st.entity_id) else {
                continue;
            };
            let Some(ip) = ip_by_suffix.get(&suffix) else {
                continue;
            };
            let led_count = leds_by_suffix.get(&suffix).copied().unwrap_or(90).max(3);
            targets.insert(
                st.entity_id.clone(),
                WledDirectTarget {
                    ip: ip.clone(),
                    led_count,
                },
            );
        }
        targets
    }

    pub(super) async fn sync_wled_direct_state_into_hue(
        &self,
        targets: &HashMap<String, WledDirectTarget>,
    ) -> ApiResult<()> {
        for (entity_id, target) in targets {
            let url = format!("http://{}/json/state", target.ip);
            let resp = match reqwest::Client::new().get(url).send().await {
                Ok(r) => r,
                Err(err) => {
                    log::warn!("Failed reading WLED state for {}: {}", entity_id, err);
                    continue;
                }
            };
            let state = match resp.json::<WledStateResponse>().await {
                Ok(s) => s,
                Err(err) => {
                    log::warn!("Failed parsing WLED state for {}: {}", entity_id, err);
                    continue;
                }
            };

            let mut res = self.state.lock().await;
            let light_ids = res.get_resource_ids_by_type(RType::Light);
            let link_light = light_ids.into_iter().find_map(|rid| {
                let link = RType::Light.link_to(rid);
                let aux = res.aux_get(&link).ok()?;
                (aux.topic.as_deref() == Some(entity_id.as_str())).then_some(link)
            });
            let Some(link_light) = link_light else {
                continue;
            };

            res.update::<Light>(&link_light.rid, |light| {
                light.on = On { on: state.on };
                if let Some(dim) = &mut light.dimming {
                    if let Some(bri) = state.bri {
                        dim.brightness = (f64::from(bri) / 255.0) * 100.0;
                    }
                }
            })?;
        }
        Ok(())
    }

    pub(super) async fn send_wled_direct_brightness(
        &self,
        target: WledDirectTarget,
        on: Option<bool>,
        brightness_pct: Option<f64>,
    ) -> ApiResult<()> {
        let bri = brightness_pct
            .map(|p| ((p.clamp(0.0, 100.0) / 100.0) * 255.0).round() as u32)
            .unwrap_or(255)
            .clamp(0, 255);
        let payload = json!({
            "on": on.unwrap_or(true),
            "bri": bri
        });
        let url = format!("http://{}/json/state", target.ip);
        let resp = reqwest::Client::new()
            .post(url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ApiError::service_error(format!(
                "WLED direct brightness API failed with status {}",
                resp.status()
            )));
        }
        Ok(())
    }

    pub(super) async fn send_wled_direct_gradient(
        &self,
        target: WledDirectTarget,
        on: Option<bool>,
        brightness_pct: Option<f64>,
        gradient_mode: Option<LightGradientMode>,
        points: &[[f64; 2]],
    ) -> ApiResult<()> {
        let bri_255 = brightness_pct
            .map(|p| (p.clamp(1.0, 100.0) / 100.0) * 255.0)
            .unwrap_or(255.0);
        let n = target.led_count.max(3);

        let to_rgb = |xy: [f64; 2]| XY::new(xy[0], xy[1]).to_rgb(bri_255);
        let sampled = if points.is_empty() {
            vec![[XY::D65_WHITE_POINT.x, XY::D65_WHITE_POINT.y]]
        } else {
            points.to_vec()
        };

        let _mode = gradient_mode.unwrap_or(LightGradientMode::InterpolatedPalette);
        // Always drive a SINGLE WLED segment and only change its palette/colors.
        // Hue can send 3 or 5 points; WLED accepts 3 color anchors in `col`.
        // For 5-point gradients, sample left/middle/right (0,2,4).
        let (p0, p1, p2) = match sampled.len() {
            0 => (
                [XY::D65_WHITE_POINT.x, XY::D65_WHITE_POINT.y],
                [XY::D65_WHITE_POINT.x, XY::D65_WHITE_POINT.y],
                [XY::D65_WHITE_POINT.x, XY::D65_WHITE_POINT.y],
            ),
            1 => (sampled[0], sampled[0], sampled[0]),
            2 => (sampled[0], sampled[1], sampled[1]),
            3 => (sampled[0], sampled[1], sampled[2]),
            4 => (sampled[0], sampled[1], sampled[3]),
            _ => (sampled[0], sampled[2], sampled[4]),
        };
        let c0 = to_rgb(p0);
        let c1 = to_rgb(p1);
        let c2 = to_rgb(p2);
        let payload = json!({
            "on": on.unwrap_or(true),
            "ps": -1,
            "pl": -1,
            "seg": [
                {
                    "id": 0,
                    "start": 0,
                    "stop": n,
                    "fx": 46,
                    "sx": 0,
                    "ix": 0,
                    "pal": 4,
                    "col": [[c0[0], c0[1], c0[2]], [c1[0], c1[1], c1[2]], [c2[0], c2[1], c2[2]]]
                }
            ]
        });

        let url = format!("http://{}/json/state", target.ip);
        let resp = reqwest::Client::new()
            .post(url)
            .json(&payload)
            .send()
            .await?;
        if !resp.status().is_success() {
            return Err(ApiError::service_error(format!(
                "WLED direct JSON API failed with status {}",
                resp.status()
            )));
        }
        Ok(())
    }
}
