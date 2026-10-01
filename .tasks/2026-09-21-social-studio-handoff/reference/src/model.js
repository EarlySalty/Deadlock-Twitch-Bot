export const PLATFORMS = ['youtube', 'tiktok', 'instagram'];
export const PLATFORM_NAMES = { youtube: 'YouTube Shorts', tiktok: 'TikTok', instagram: 'Instagram Reels' };
export const STATUS = {
    new: { label: 'Neu', tone: 'neutral' }, review: { label: 'Wartet auf Freigabe', tone: 'review' },
    scheduled: { label: 'Geplant', tone: 'scheduled' }, published: { label: 'Gepostet', tone: 'success' },
    error: { label: 'Fehler', tone: 'danger' }, archived: { label: 'Archiviert', tone: 'neutral' }
};
export function validateSchedule(schedule) {
    const errors = {};
    try {
        new Intl.DateTimeFormat('de-DE', { timeZone: schedule.timezone });
    }
    catch {
        errors.timezone = 'Bitte eine gültige Zeitzone wählen.';
    }
    for (const p of schedule.platforms) {
        if (String(p.posts_per_week ?? '').trim() === '' || !Number.isInteger(Number(p.posts_per_week)) || Number(p.posts_per_week) < 0 || Number(p.posts_per_week) > 70)
            errors[p.platform + '.week'] = 'Bitte eine ganze Zahl von 0 bis 70 eingeben.';
        if (String(p.max_posts_per_day ?? '').trim() === '' || !Number.isInteger(Number(p.max_posts_per_day)) || Number(p.max_posts_per_day) < 0 || Number(p.max_posts_per_day) > 10)
            errors[p.platform + '.day'] = 'Bitte eine ganze Zahl von 0 bis 10 eingeben.';
        if (p.auto_post && Number(p.posts_per_week) > Number(p.max_posts_per_day) * 7)
            errors[p.platform + '.week'] = 'Das Wochenziel überschreitet dein Tageslimit.';
        if (p.post_times.length > 12 || (p.auto_post && !p.post_times.length) || p.post_times.some(t => !/^([01]\d|2[0-3]):[0-5]\d$/.test(t)))
            errors[p.platform + '.times'] = 'Ein bis zwölf gültige Uhrzeiten angeben.';
        if (new Set(p.post_times).size !== p.post_times.length)
            errors[p.platform + '.times'] = 'Jede Uhrzeit bitte nur einmal verwenden.';
    }
    return errors;
}
export function normalizeSchedule(s) {
    return { ...s, platforms: s.platforms.map(p => ({ ...p, posts_per_week: Number(p.posts_per_week), max_posts_per_day: Number(p.max_posts_per_day), post_times: [...new Set(p.post_times)].sort() })) };
}
export function queueMetrics(clips) {
    return { review: clips.filter(c => c.status === 'review').length, scheduled: clips.filter(c => c.status === 'scheduled').reduce((n, c) => n + c.targets.length, 0), errors: clips.filter(c => c.status === 'error').length };
}
export function formatDuration(seconds) {
    if (seconds == null || !Number.isFinite(seconds))
        return 'Dauer offen';
    const total = Math.max(0, Math.round(seconds));
    return Math.floor(total / 60) + ':' + String(total % 60).padStart(2, '0');
}
export function escapeHTML(value) { return String(value ?? '').replace(/[&<>"']/g, c => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' }[c])); }
export const DEFAULT_SCHEDULE = {
    approval_mode: 'manual', timezone: 'Europe/Berlin', subtitles_enabled: true,
    platforms: [
        { platform: 'youtube', auto_post: true, posts_per_week: 4, max_posts_per_day: 1, post_times: ['18:00'], next_slot: null },
        { platform: 'tiktok', auto_post: true, posts_per_week: 3, max_posts_per_day: 1, post_times: ['19:30'], next_slot: null },
        { platform: 'instagram', auto_post: false, posts_per_week: 3, max_posts_per_day: 1, post_times: ['18:30'], next_slot: null }
    ],
    categories: [{ category_key: 'deadlock', display_name: 'Deadlock', auto_post: true, enrichment_enabled: true }, { category_key: 'other', display_name: 'Andere Spiele', auto_post: false, enrichment_enabled: false }],
    pool: { verfuegbare_clips: 7, reicht_fuer_tage: 7, posts_pro_woche: 7, warnung: false }
};
export const DEMO_CLIPS = [
    { id: 1, title: 'Ein letzter Fight. Und plötzlich ist alles wieder offen.', status: 'review', duration: 34, views: 2840, source: 'Twitch', targets: ['youtube', 'tiktok'], scene: 0, transcript: 'Warte, wir können das noch gewinnen. Ich gehe links rein. Jetzt!' },
    { id: 2, title: 'Der Hook, mit dem wirklich niemand gerechnet hat', status: 'review', duration: 28, views: 1620, source: 'Twitch', targets: ['youtube'], scene: 1, transcript: 'Okay, das war nicht geplant. Aber ich nehme es.' },
    { id: 3, title: 'So verteidigt man den Patron mit 1 HP', status: 'review', duration: 46, views: 3910, source: 'Twitch', targets: ['youtube', 'tiktok'], scene: 2, transcript: 'Nicht aufgeben. Wir brauchen nur einen guten Fight.' },
    { id: 4, title: 'Von 0 auf Comeback in 40 Sekunden', status: 'scheduled', duration: 41, views: 1890, source: 'Twitch', targets: ['youtube', 'tiktok'], scene: 3, scheduledLabel: 'Di., 22.09. · 18:00' },
    { id: 5, title: 'Eine kleine Rotation macht den Unterschied', status: 'scheduled', duration: 37, views: 1240, source: 'Twitch', targets: ['youtube'], scene: 1, scheduledLabel: 'Mi., 23.09. · 18:00' },
    { id: 6, title: 'Dieser Push war eine sehr gute Idee. Fast.', status: 'error', duration: 25, views: 860, source: 'Twitch', targets: ['instagram'], scene: 0, error: 'Instagram-Verbindung abgelaufen. Verbinde das Konto erneut.' },
    { id: 7, title: 'Der perfekte Zeitpunkt für ein Ult', status: 'published', duration: 32, views: 4250, source: 'Twitch', targets: ['youtube', 'tiktok'], scene: 2, publishedLabel: '20.09.2026' },
    { id: 8, title: 'Noch ein Moment aus dem letzten Stream', status: 'new', duration: 53, views: 620, source: 'Twitch', targets: ['youtube'], scene: 3 }
];
