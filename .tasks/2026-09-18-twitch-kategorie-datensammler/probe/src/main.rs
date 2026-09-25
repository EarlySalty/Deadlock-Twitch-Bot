//! One-shot operational probe. Reuses the NEW shared transport, never implements IRC.
//! No credentials, no Helix client, no chat/moderation API and no raw message output.
use std::sync::{atomic::{AtomicU64, Ordering}, Arc};
use std::time::Duration;
use tb_monitoring::anon_chat::{AnonChatConfig, AnonChatHandle, PrivmsgSink, TokioTaskSpawner};

const TAG_KEYS: [&str; 7] = ["room-id", "user-id", "id", "tmi-sent-ts", "emotes", "badges", "display-name"];

#[derive(Default)]
struct CountOnly {
    messages: AtomicU64,
    seen_tag_keys: AtomicU64,
}
impl PrivmsgSink for CountOnly {
    fn handle_privmsg(&self, line: String) {
        if let Some(parsed) = tb_engagement::irc_message::parse_privmsg(&line) {
            self.messages.fetch_add(1, Ordering::Relaxed);
            let mut mask = 0;
            for (index, key) in TAG_KEYS.iter().enumerate() {
                if parsed.tags.contains_key(*key) { mask |= 1 << index; }
            }
            self.seen_tag_keys.fetch_or(mask, Ordering::Relaxed);
        }
        // All raw line/text/identity values are dropped without logs or persistence.
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut arguments = std::env::args().skip(1);
    let channel = arguments.next().ok_or("Usage: category-anonymous-probe <live-channel> [seconds<=60]")?;
    let channel = tb_monitoring::anon_chat::validiere_login(&channel).ok_or("Invalid Twitch login")?;
    let seconds: u64 = arguments.next().map(|value| value.parse()).transpose()?.unwrap_or(20);
    if !(1..=60).contains(&seconds) || arguments.next().is_some() { return Err("Invalid probe duration/arguments".into()); }
    let sink = Arc::new(CountOnly::default());
    let config = AnonChatConfig { nick_base: 94_718_200, ..AnonChatConfig::default() };
    let handle = AnonChatHandle::start(sink.clone(), config, Arc::new(TokioTaskSpawner));
    handle.set_channels(vec![channel]).await;
    tokio::time::sleep(Duration::from_secs(seconds)).await;
    let stats = handle.stats();
    handle.stop().await;
    let mask = sink.seen_tag_keys.load(Ordering::Relaxed);
    let tags: Vec<_> = TAG_KEYS.iter().enumerate().filter_map(|(index, key)| (mask & (1 << index) != 0).then_some(*key)).collect();
    println!("duration_seconds={seconds}");
    println!("protocol_join_writes={}", stats.joins_written);
    println!("received_privmsgs={}", sink.messages.load(Ordering::Relaxed));
    println!("dropped_privmsgs={}", stats.privmsgs_dropped);
    println!("reconnects={}", stats.reconnects);
    println!("observed_tag_names={tags:?}");
    println!("raw_message_storage=false; authenticated_chat=false; outgoing_chat_api=false");
    if stats.joins_written == 0 { return Err("No JOIN completed; connection not verified".into()); }
    Ok(())
}
