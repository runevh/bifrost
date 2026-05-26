use std::sync::Arc;

use async_trait::async_trait;
use svc::traits::{BoxDynService, Service};
use svc::{error::SvcError, template::ServiceTemplate};
use tokio::sync::broadcast::Receiver;

use bifrost_api::backend::BackendRequest;

use crate::error::ApiResult;
use crate::server::appstate::AppState;

pub struct DefaultServiceTemplate {
    state: AppState,
}

impl DefaultServiceTemplate {
    #[must_use]
    pub const fn new(state: AppState) -> Self {
        Self { state }
    }
}

impl ServiceTemplate for DefaultServiceTemplate {
    fn generate(&self, name: String) -> Result<BoxDynService, SvcError> {
        Ok(DefaultBackend::new(name, self.state.clone()).boxed())
    }
}

pub struct DefaultBackend {
    name: String,
    state: AppState,
}

impl DefaultBackend {
    #[must_use]
    pub const fn new(name: String, state: AppState) -> Self {
        Self { name, state }
    }

    async fn event_loop(&mut self, chan: &mut Receiver<Arc<BackendRequest>>) -> ApiResult<()> {
        loop {
            let req = chan.recv().await?;
            self.handle_backend_event(req).await?;
        }
    }

    async fn handle_backend_event(&mut self, req: Arc<BackendRequest>) -> ApiResult<()> {
        // This backend intentionally acts as a no-op adapter. It is useful for
        // API/UI integration testing of the Hue emulation layer without any
        // external backend.
        log::info!("[{}] default backend event: {req:?}", self.name);
        Ok(())
    }
}

#[async_trait]
impl Service for DefaultBackend {
    type Error = crate::error::ApiError;

    async fn start(&mut self) -> ApiResult<()> {
        log::info!("[{}] Default backend started", self.name);
        Ok(())
    }

    async fn run(&mut self) -> ApiResult<()> {
        let mut chan = self.state.res.lock().await.backend_event_stream();
        let res = self.event_loop(&mut chan).await;
        if let Err(err) = res {
            log::error!("[{}] Default backend event loop broke: {err}", self.name);
        }
        Ok(())
    }
}
