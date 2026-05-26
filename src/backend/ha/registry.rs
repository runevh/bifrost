use super::*;

impl HomeAssistantBackend {
    pub(super) async fn list_areas(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<Vec<HaArea>> {
        const CMD_ID: u64 = 2;
        let list_cmd = json!({
            "id": CMD_ID,
            "type": "config/area_registry/list",
        });
        Self::send_command_result(ws, CMD_ID, &list_cmd, "area_registry/list").await
    }

    pub(super) async fn list_entity_registry(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<Vec<HaEntityRegistryEntry>> {
        const CMD_ID: u64 = 3;
        let list_cmd = json!({
            "id": CMD_ID,
            "type": "config/entity_registry/list",
        });
        Self::send_command_result(ws, CMD_ID, &list_cmd, "entity_registry/list").await
    }

    pub(super) async fn list_device_registry(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<Vec<HaDeviceRegistryEntry>> {
        const CMD_ID: u64 = 5;
        let list_cmd = json!({
            "id": CMD_ID,
            "type": "config/device_registry/list",
        });
        Self::send_command_result(ws, CMD_ID, &list_cmd, "device_registry/list").await
    }

    pub(super) async fn get_states(
        ws: &mut WebSocketStream<MaybeTlsStream<TcpStream>>,
    ) -> ApiResult<Vec<HaState>> {
        const CMD_ID: u64 = 6;
        let cmd = json!({
            "id": CMD_ID,
            "type": "get_states",
        });
        Self::send_command_result(ws, CMD_ID, &cmd, "get_states").await
    }
}
