//! Go-Live-/Offline-Announcements: Template-Rendering ([`template`])
//! und Broker-Sink ([`sink`]).

pub mod sink;
pub mod template;

pub use sink::{
    AnnouncementEditOutcome, AnnouncementSettings, AnnouncementTransport, BrokerAnnouncementSink,
    ChannelProfileSource, LivePingRoleProvider, NoChannelProfile, NoVodPreview, VodPreviewSource,
};
pub use template::{AnnouncementConfig, RenderedAnnouncement};
