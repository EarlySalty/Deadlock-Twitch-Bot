//! Verwendet für das Archiv den bereits eingerichteten Helix-Zugang des Bots.

use std::sync::Arc;
use tb_transport_twitch::HelixClient;
use tb_vod_archive::{VodArchiveError, VodVerfuegbarkeit};

struct ArchivHelix(HelixClient);

#[async_trait::async_trait]
impl VodVerfuegbarkeit for ArchivHelix {
    async fn ist_verfuegbar(&self, twitch_id: &str) -> Result<bool, VodArchiveError> {
        self.0
            .video_verfuegbar(twitch_id)
            .await
            .map_err(|_| VodArchiveError::VerfuegbarkeitNichtBestaetigt)
    }
}

pub fn verfuegbarkeit(helix: Option<HelixClient>) -> Option<Arc<dyn VodVerfuegbarkeit>> {
    helix.map(|client| Arc::new(ArchivHelix(client)) as Arc<dyn VodVerfuegbarkeit>)
}
