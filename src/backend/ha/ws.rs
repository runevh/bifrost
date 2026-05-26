use super::*;

impl HomeAssistantBackend {
    pub(super) async fn recv_text(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        context: &str,
    ) -> ApiResult<String> {
        let Some(msg) = ws.next().await else {
            return Err(ApiError::service_error(format!(
                "Home Assistant websocket closed while waiting for {context}"
            )));
        };

        match msg? {
            Message::Text(txt) => Ok(txt.to_string()),
            other => Err(ApiError::service_error(format!(
                "Unexpected Home Assistant websocket message while waiting for {context}: {other:?}"
            ))),
        }
    }

    pub(super) async fn send_json(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        payload: &Value,
    ) -> ApiResult<()> {
        let text = serde_json::to_string(payload)?;
        ws.send(Message::Text(text.into())).await?;
        Ok(())
    }

    pub(super) async fn authenticate(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        token: &str,
    ) -> ApiResult<()> {
        let greeting_raw = Self::recv_text(ws, "auth_required").await?;
        let greeting: Value = serde_json::from_str(&greeting_raw)?;
        let msg_type = greeting.get("type").and_then(Value::as_str).unwrap_or("");
        if msg_type != "auth_required" {
            return Err(ApiError::service_error(format!(
                "Expected Home Assistant auth_required, got {greeting_raw}"
            )));
        }

        let auth_msg = json!({
            "type": "auth",
            "access_token": token,
        });
        Self::send_json(ws, &auth_msg).await?;

        let auth_reply_raw = Self::recv_text(ws, "auth reply").await?;
        let auth_reply: Value = serde_json::from_str(&auth_reply_raw)?;
        let auth_type = auth_reply.get("type").and_then(Value::as_str).unwrap_or("");
        if auth_type != "auth_ok" {
            return Err(ApiError::service_error(format!(
                "Home Assistant authentication failed: {auth_reply_raw}"
            )));
        }

        Ok(())
    }

    pub(super) async fn subscribe_events(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<()> {
        let subscribe = json!({
            "id": 1,
            "type": "subscribe_events",
        });
        Self::send_json(ws, &subscribe).await?;

        let reply_raw = Self::recv_text(ws, "subscribe_events reply").await?;
        let reply: Value = serde_json::from_str(&reply_raw)?;
        let ok = reply
            .get("success")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        if !ok {
            return Err(ApiError::service_error(format!(
                "Home Assistant subscribe_events failed: {reply_raw}"
            )));
        }

        Ok(())
    }

    pub(super) async fn send_command_result<T: serde::de::DeserializeOwned>(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
        cmd_id: u64,
        payload: &Value,
        context: &str,
    ) -> ApiResult<T> {
        Self::send_json(ws, payload).await?;

        loop {
            let raw = Self::recv_text(ws, context).await?;
            let msg: Value = serde_json::from_str(&raw)?;
            let msg_type = msg.get("type").and_then(Value::as_str).unwrap_or("");
            if msg_type == "event" {
                continue;
            }
            if msg.get("id").and_then(Value::as_u64) != Some(cmd_id) {
                continue;
            }

            let ok = msg.get("success").and_then(Value::as_bool).unwrap_or(false);
            if !ok {
                return Err(ApiError::service_error(format!(
                    "Home Assistant {context} failed: {raw}"
                )));
            }

            let Some(result) = msg.get("result") else {
                return Err(ApiError::service_error(format!(
                    "Home Assistant {context} returned no result field: {raw}"
                )));
            };
            return serde_json::from_value::<T>(result.clone()).map_err(ApiError::from);
        }
    }
}
