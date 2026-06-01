from __future__ import annotations

HIGHLIGHT_DISCORD_CHANNEL_ID = 1511060958460776458
STATE_PATH = "data/highlight_clipper/state.json"
CLIPS_DIR = "data/highlight_clipper/clips"
POLL_INTERVAL_SECONDS = 600
# Knappes, action-zentriertes Framing: kurzer Anlauf vor der ersten Action,
# kurzer Nachlauf nach der letzten. Frühere Logik (combo_score*3+15) erzeugte
# 21-24s Leerlauf-Vorlauf und hat die Clips ertränkt.
CLIP_PRE_ROLL_SECONDS = 6
CLIP_POST_ROLL_SECONDS = 4
MAX_CLIP_SECONDS = 40
CLIP_PADDING_SECONDS = 10
MULTIKILL_MIN_KILLS = 2
MULTIKILL_THRESHOLD_SECONDS = 15
TEAMFIGHT_MIN_KILLS = 4
TEAMFIGHT_THRESHOLD_SECONDS = 15
FFMPEG_PATH = "/usr/bin/ffmpeg"
MAX_DISCORD_FILE_MB = 24
