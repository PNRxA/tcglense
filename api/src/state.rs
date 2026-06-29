use std::sync::Arc;

use sea_orm::DatabaseConnection;

use crate::config::Config;

/// Shared, cheaply-clonable application state passed to every handler.
#[derive(Clone)]
pub struct AppState {
    pub db: DatabaseConnection,
    pub config: Arc<Config>,
}
