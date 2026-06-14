//! Go-Live-/Offline-Announcements: Template-Rendering ([`template`])
//! und Broker-Sink ([`sink`]).

pub mod sink;
pub mod template;

pub use sink::{
    AnnounceConfigStore, AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink,
    LivePingRoleProvider, NoVodPreview, VodPreviewSource,
};
pub use template::{AnnouncementConfig, RenderedAnnouncement};
