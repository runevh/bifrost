use super::*;

#[derive(Debug, Clone, Default)]
pub(super) struct PendingLightUpdate {
    pub(super) on: Option<bool>,
    pub(super) brightness_pct: Option<f64>,
    pub(super) xy_color: Option<[f64; 2]>,
    pub(super) color_temp_kelvin: Option<u32>,
    pub(super) gradient_points: Option<Vec<[f64; 2]>>,
    pub(super) gradient_mode: Option<LightGradientMode>,
    pub(super) deadline: Option<Instant>,
}

impl HomeAssistantBackend {
    pub(super) async fn enqueue_light_update(
        &mut self,
        link: &hue::api::ResourceLink,
        upd: &hue::api::LightUpdate,
    ) -> ApiResult<()> {
        let entity_id = {
            let lock = self.state.lock().await;
            let Ok(aux) = lock.aux_get(link) else {
                log::warn!(
                    "No aux mapping found for light {:?}; skipping HA update",
                    link.rid
                );
                return Ok(());
            };
            let Some(topic) = aux.topic.clone() else {
                log::warn!(
                    "No HA entity_id topic for light {:?}; skipping HA update",
                    link.rid
                );
                return Ok(());
            };
            topic
        };

        if let Some(grad) = &upd.gradient {
            if let Some(segment_entities) = self.wled_segment_targets.get(&entity_id).cloned() {
                let has_real_segments = !segment_entities.is_empty()
                    && segment_entities
                        .iter()
                        .all(|eid| Self::parse_segment_index(eid).is_some());
                if has_real_segments && !grad.points.is_empty() {
                    let max_idx = grad.points.len().saturating_sub(1);
                    for (idx, seg_entity_id) in segment_entities.iter().enumerate() {
                        let point = &grad.points[idx.min(max_idx)];
                        let pending = self
                            .pending_light_updates
                            .entry(seg_entity_id.clone())
                            .or_default();
                        pending.deadline =
                            Some(Instant::now() + Duration::from_millis(Self::UPDATE_DEBOUNCE_MS));
                        if let Some(on) = upd.on {
                            pending.on = Some(on.on);
                        }
                        if let Some(d) = upd.dimming {
                            pending.brightness_pct = Some(d.brightness.clamp(0.0, 100.0));
                        }
                        pending.xy_color = Some([point.color.xy.x, point.color.xy.y]);
                    }
                    return Ok(());
                }
            }
            if !grad.points.is_empty() && self.wled_direct_targets.contains_key(&entity_id) {
                // Keep Hue-side gradient state authoritative for WLED direct lights.
                {
                    let mut res = self.state.lock().await;
                    let grad_points: Vec<LightGradientPoint> = grad.points.clone();
                    let grad_mode = grad.mode.unwrap_or(LightGradientMode::InterpolatedPalette);
                    let first_xy = grad_points
                        .first()
                        .map(|p| p.color.xy)
                        .unwrap_or(XY::D65_WHITE_POINT);
                    let on_value = upd.on.map(|v| v.on);
                    let bri_value = upd.dimming.map(|v| v.brightness.clamp(0.0, 100.0));
                    let _ = res.update::<Light>(&link.rid, |light| {
                        if let Some(on) = on_value {
                            light.on = On { on };
                        }
                        if let Some(bri) = bri_value
                            && let Some(dim) = &mut light.dimming
                        {
                            dim.brightness = bri;
                        }
                        if let Some(col) = &mut light.color {
                            col.xy = first_xy;
                        }
                        if let Some(g) = &mut light.gradient {
                            g.mode = grad_mode;
                            g.points = grad_points.clone();
                        }
                    });
                }

                let pending = self.pending_light_updates.entry(entity_id).or_default();
                pending.deadline =
                    Some(Instant::now() + Duration::from_millis(Self::UPDATE_DEBOUNCE_MS));
                pending.gradient_points = Some(
                    grad.points
                        .iter()
                        .map(|p| [p.color.xy.x, p.color.xy.y])
                        .collect::<Vec<_>>(),
                );
                pending.gradient_mode = grad.mode;
                if let Some(on) = upd.on {
                    pending.on = Some(on.on);
                }
                if let Some(d) = upd.dimming {
                    pending.brightness_pct = Some(d.brightness.clamp(0.0, 100.0));
                }
                return Ok(());
            }
        }

        let pending = self.pending_light_updates.entry(entity_id).or_default();
        pending.deadline = Some(Instant::now() + Duration::from_millis(Self::UPDATE_DEBOUNCE_MS));
        pending.gradient_points = None;
        pending.gradient_mode = None;

        if let Some(on) = upd.on {
            pending.on = Some(on.on);
            if !on.on {
                // Turning off wins; clear value fields to avoid pointless payload.
                pending.brightness_pct = None;
                pending.xy_color = None;
                pending.color_temp_kelvin = None;
            }
        }
        if let Some(d) = upd.dimming {
            pending.brightness_pct = Some(d.brightness.clamp(0.0, 100.0));
        }
        if let Some(color) = upd.color {
            pending.xy_color = Some([color.xy.x, color.xy.y]);
        }
        if let Some(ct) = upd.color_temperature.and_then(|v| v.mirek) {
            let kelvin = (1_000_000_u32 / u32::from(ct)).clamp(2000, 6535);
            pending.color_temp_kelvin = Some(kelvin);
        }
        Ok(())
    }

    pub(super) async fn send_light_command(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        entity_id: String,
        cmd: PendingLightUpdate,
    ) -> ApiResult<()> {
        if let (Some(points), Some(target)) = (
            cmd.gradient_points.clone(),
            self.wled_direct_targets.get(&entity_id).cloned(),
        ) {
            self.send_wled_direct_gradient(
                target,
                cmd.on,
                cmd.brightness_pct,
                cmd.gradient_mode,
                &points,
            )
            .await?;
            return Ok(());
        }
        if let Some(target) = self.wled_direct_targets.get(&entity_id).cloned() {
            if cmd.brightness_pct.is_some()
                && cmd.xy_color.is_none()
                && cmd.color_temp_kelvin.is_none()
            {
                self.send_wled_direct_brightness(target, cmd.on, cmd.brightness_pct)
                    .await?;
                return Ok(());
            }
        }

        // Hue can send partial updates (only brightness/color/ct without explicit "on").
        // In HA those are represented as light.turn_on with additional fields.
        let service = if matches!(cmd.on, Some(false)) {
            "turn_off"
        } else {
            "turn_on"
        };

        let mut service_data = serde_json::Map::new();
        if let Some(v) = cmd.brightness_pct {
            service_data.insert("brightness_pct".to_string(), json!(v));
        }
        if let Some([x, y]) = cmd.xy_color {
            service_data.insert("xy_color".to_string(), json!([x, y]));
        }
        if let Some(kelvin) = cmd.color_temp_kelvin {
            service_data.insert("color_temp_kelvin".to_string(), json!(kelvin));
        }
        if self.wled_segment_entities.contains(&entity_id) && cmd.xy_color.is_some() {
            service_data.insert("effect".to_string(), json!("Solid"));
        }
        let transition_ms = self
            .config
            .transition_ms
            .unwrap_or(Self::DEFAULT_TRANSITION_MS)
            .clamp(0, 30_000);
        let transition = f64::from(transition_ms) / 1000.0;
        service_data.insert("transition".to_string(), json!(transition));

        let command_id = self.next_id();
        let mut payload = serde_json::Map::new();
        payload.insert("id".to_string(), json!(command_id));
        payload.insert("type".to_string(), json!("call_service"));
        payload.insert("domain".to_string(), json!("light"));
        payload.insert("service".to_string(), json!(service));
        payload.insert(
            "target".to_string(),
            json!({
                "entity_id": entity_id,
            }),
        );
        if !service_data.is_empty() {
            payload.insert("service_data".to_string(), Value::Object(service_data));
        }

        Self::send_json(ws, &Value::Object(payload)).await?;
        Ok(())
    }

    pub(super) async fn handle_backend_event(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        req: Arc<bifrost_api::backend::BackendRequest>,
    ) -> ApiResult<()> {
        match &*req {
            bifrost_api::backend::BackendRequest::LightUpdate(link, upd) => {
                let _ = ws; // Keep signature uniform for future event handlers.
                self.enqueue_light_update(link, upd).await
            }
            _ => Ok(()),
        }
    }

    pub(super) async fn flush_pending_light_updates(
        &mut self,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<()> {
        let now = Instant::now();
        let ready: Vec<String> = self
            .pending_light_updates
            .iter()
            .filter_map(|(entity_id, cmd)| {
                cmd.deadline
                    .filter(|deadline| *deadline <= now)
                    .map(|_| entity_id.clone())
            })
            .collect();

        for entity_id in ready {
            if let Some(cmd) = self.pending_light_updates.remove(&entity_id) {
                self.send_light_command(ws, entity_id, cmd).await?;
            }
        }
        Ok(())
    }

    pub(super) async fn event_loop(
        &mut self,
        chan: &mut Receiver<Arc<bifrost_api::backend::BackendRequest>>,
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<()> {
        loop {
            tokio::select! {
                req = chan.recv() => {
                    match req {
                        Ok(req) => {
                            if let Err(err) = self.handle_backend_event(ws, req).await {
                                log::warn!("Failed to handle backend event: {err}");
                            }
                        }
                        Err(err) => {
                            log::warn!("Backend event stream receive error: {err}");
                        }
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(20)) => {
                    if let Err(err) = self.flush_pending_light_updates(ws).await {
                        log::warn!("Failed to flush pending light updates: {err}");
                    }
                }
                msg = ws.next() => {
                    let Some(msg) = msg else {
                        return Err(ApiError::service_error("Home Assistant websocket disconnected"));
                    };
                    match msg? {
                        Message::Text(txt) => {
                            let parsed: Result<Value, _> = serde_json::from_str(&txt);
                            if let Ok(msg) = parsed {
                                let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("");
                                if msg_type == "result" {
                                    let ok = msg.get("success").and_then(Value::as_bool).unwrap_or(true);
                                    if !ok {
                                        log::warn!("Home Assistant command failed: {}", txt);
                                    } else {
                                        log::trace!("Home Assistant command result: {}", txt);
                                    }
                                } else if msg_type == "event" {
                                    if let Err(err) = self.handle_ha_event_message(msg).await {
                                        log::warn!("Failed to process Home Assistant event message: {err}");
                                    }
                                } else {
                                    log::trace!("Home Assistant event: {}", txt);
                                }
                            } else {
                                log::trace!("Home Assistant event: {}", txt);
                            }
                        }
                        Message::Ping(payload) => {
                            ws.send(Message::Pong(payload)).await?;
                        }
                        Message::Close(frame) => {
                            return Err(ApiError::service_error(format!(
                                "Home Assistant websocket closed: {frame:?}"
                            )));
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    pub(super) async fn handle_ha_event_message(&self, msg: Value) -> ApiResult<()> {
        let Ok(evt) = serde_json::from_value::<HaWsEventMessage>(msg) else {
            return Ok(());
        };
        if evt.event.event_type != "state_changed" {
            return Ok(());
        }

        let Some(new_state) = evt.event.data.new_state else {
            return Ok(());
        };
        if !new_state.entity_id.starts_with("light.") {
            return Ok(());
        }

        let is_on = match new_state.state.as_str() {
            "on" => Some(true),
            "off" => Some(false),
            _ => None,
        };
        let Some(is_on) = is_on else {
            return Ok(());
        };

        let mut res = self.state.lock().await;
        let light_ids = res.get_resource_ids_by_type(RType::Light);
        let link_light = light_ids.into_iter().find_map(|rid| {
            let link = RType::Light.link_to(rid);
            let aux = res.aux_get(&link).ok()?;
            (aux.topic.as_deref() == Some(new_state.entity_id.as_str())).then_some(link)
        });
        let Some(link_light) = link_light else {
            return Ok(());
        };

        res.update::<Light>(&link_light.rid, |light| {
            light.on = On { on: is_on };

            if let Some(dim) = &mut light.dimming {
                if let Some(brightness) = new_state.attributes.brightness {
                    dim.brightness = f64::from(brightness) / 255.0 * 100.0;
                } else if !is_on {
                    dim.brightness = 0.0;
                }
            }

            // For WLED direct gradient lights, keep Hue gradient/colors as last requested
            // from Hue app instead of forcing a flattened single-color HA readback.
            let is_wled_direct = self.wled_direct_targets.contains_key(&new_state.entity_id);
            if !is_wled_direct && let Some(col) = &mut light.color {
                if let Some(xy) = new_state.attributes.current_xy() {
                    col.xy = xy;
                }
            }

            if let Some(ct) = &mut light.color_temperature {
                ct.mirek = new_state.attributes.current_mirek();
                ct.mirek_valid = ct.mirek.is_some();
            }
        })?;

        Ok(())
    }
}
