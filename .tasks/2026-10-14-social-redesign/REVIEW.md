# Independent review

## Frontend review
The independent read-only reviewer confirmed six defects and then verified the corrective batch without remaining confirmed findings:
1. Completed production jobs disappear from the schedule response, so primary scheduled videos require the existing publication snapshot.
2. Failed refreshes must not display cached green status-strip values as current.
3. The schedule review count includes production jobs, not only finished videos.
4. An empty-pool production hold needs an actionable warning.
5. Legacy publication times use the legacy timezone, not the Auto-Pilot timezone.
6. Review shortcuts must reset incompatible search and platform filters.

The implementation now combines schedule and publication snapshots, distinguishes unavailable sources, labels reviews as productions and videos, exposes production holds, formats timezone-specific legacy dates, and resets shortcut filters. A regression test additionally caught planned events with no destinations before execution; these remain visible.

## Rust review
The independent Rust reviewer resumed after an initially blocked compiler attempt. Static review found no confirmed code defects in the nullable view_count response extension. PostgreSQL integer matches Option<i32>; missing source clips preserve pool history with a null count; ownership, pagination and reservation behavior remain unchanged. This verdict is distinct from compiler/test verification.

## Static design detector
The final detector reported two side-tab warnings: the existing selected-sidebar navigation marker and the new two-pixel overview-row status marker. These mark selection/state rather than decorating cards; the pinned sidebar identity is preserved. No ignore configuration or suppression was added. The palette suite found unapproved schematic colors; these were replaced with existing Industrial Gold colors without weakening the palette test.
