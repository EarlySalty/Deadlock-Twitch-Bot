import { PLATFORMS, PLATFORM_NAMES, STATUS, DEFAULT_SCHEDULE, DEMO_CLIPS, validateSchedule, normalizeSchedule, queueMetrics, formatDuration, escapeHTML as esc } from './model.js';
import { icon } from './icons.js';
const clone = value => structuredClone(value);
let clips = clone(DEMO_CLIPS), saved = clone(DEFAULT_SCHEDULE), draft = clone(saved);
let view = 'queue', filter = 'review', search = '', platform = 'all', layout = 'list', busy = new Set(), errors = {}, menuId = null, scheduleErrors = {}, failNext = false, savedAt = null, activeModal = null;
let schedulePending = false;
let defaultDemoLayout = { template: 'pip', position: 50, cam: 32 };
let accountConnected = { youtube: true, tiktok: true, instagram: false };
const $ = s => document.querySelector(s);
const tabs = [['queue', 'Pipeline', 'layers'], ['autopilot', 'Auto-Pilot & Zeitplan', 'bolt'], ['templates', 'Templates & Layouts', 'crop'], ['accounts', 'Konten & Einstellungen', 'sliders']];
const meta = { queue: ['Deine Clip-Pipeline', 'Aus guten Momenten werden deine nächsten Posts.'], autopilot: ['Auto-Pilot & Zeitplan', 'Du bestimmst, was wann veröffentlicht wird.'], templates: ['Templates & Layouts', 'Ein Format für deine Clips. Ein eigener Editor für den Feinschliff.'], accounts: ['Konten & Einstellungen', 'Deine Quellen, Zielplattformen und Verbindungen.'] };
const formatViews = n => n == null ? 'Eigener Upload' : new Intl.NumberFormat('de-DE', { notation: 'compact', maximumFractionDigits: 1 }).format(n) + ' Aufrufe';
function platformIcon(p) { return `<span class="mini-platform ${p}" aria-hidden="true">${p === 'youtube' ? '▶' : p === 'tiktok' ? '♪' : '◎'}</span>`; }
function statusBadge(status) { return `<span class="badge badge-${STATUS[status].tone}">${STATUS[status].label}</span>`; }
function thumbnail(c) {
    const palette = [['#233f45', '#af9975'], ['#403536', '#dcba7c'], ['#273644', '#8291b0'], ['#294039', '#9eafa0']][c.scene % 4];
    return `<div class="thumb shrink-0 w-full md:w-[196px]" aria-label="Abstraktes Demo-Vorschaubild"><svg viewBox="0 0 320 180" preserveAspectRatio="xMidYMid slice"><defs><linearGradient id="sky-${c.id}" x2="0" y2="1"><stop stop-color="${palette[0]}"/><stop offset="1" stop-color="#131519"/></linearGradient><radialGradient id="light-${c.id}"><stop stop-color="${palette[1]}" stop-opacity=".45"/><stop offset="1" stop-color="${palette[1]}" stop-opacity="0"/></radialGradient></defs><rect width="320" height="180" fill="url(#sky-${c.id})"/><circle cx="218" cy="65" r="100" fill="url(#light-${c.id})"/><path d="M0 44 50 30 67 42 67 124 82 124 82 11 118 11 126 22 126 127 147 127 147 62 176 62 184 74 184 137 206 137 206 38 245 38 258 50 258 119 286 119 286 59 320 51V180H0Z" fill="#131819" opacity=".8"/><path d="m0 180 133-51h56l131 51" fill="#545853" opacity=".5"/><path d="m80 180 77-51m87 51-76-51" stroke="${palette[1]}" stroke-opacity=".4"/><g fill="${palette[1]}" opacity=".5"><rect x="92" y="28" width="4" height="10"/><rect x="105" y="28" width="4" height="10"/><rect x="92" y="54" width="4" height="10"/><rect x="227" y="59" width="4" height="13"/><rect x="240" y="59" width="4" height="13"/></g><path d="m152 126 4-17h8l5 18-6 4 6 17h-7l-4-11-6 11h-5l5-19Z" fill="#a9b2a5"/><rect x="20" y="160" width="55" height="3" rx="1.5" fill="#9bb7b4" opacity=".6"/><circle cx="288" cy="157" r="9" stroke="#bfb49b" fill="none" opacity=".5"/></svg><span class="sample-note">DEMO</span><span class="duration">${formatDuration(c.duration)}</span></div>`;
}
function metric(label, value, detail, name, tone = 'text-text-primary') { return `<div class="px-5 py-4"><div class="flex items-center justify-between gap-2 text-[13px] text-text-secondary"><span>${label}</span>${icon(name, 'h-4 w-4 text-text-secondary')}</div><div class="metric-number mt-3 ${tone}">${value}</div><p class="mt-1.5 text-xs text-text-secondary">${detail}</p></div>`; }
function toast(text) { const el = $('#toast'); el.textContent = text; el.hidden = false; clearTimeout(toast.timer); toast.timer = setTimeout(() => el.hidden = true, 5500); }
async function simulate() { await new Promise(r => setTimeout(r, 260)); if (failNext) {
    failNext = false;
    throw Error('Simulierter Serverfehler. Deine Änderung wurde nicht gespeichert.');
} }
function renderHeader() {
    $('#page-title').textContent = meta[view][0];
    $('#page-description').textContent = meta[view][1];
    $('#page-actions').innerHTML = view === 'queue' ? `<button class="btn btn-ghost" data-action="analytics">${icon('chart')}<span class="hidden sm:inline">Auswertung</span></button><button class="btn btn-primary" data-action="upload">${icon('plus')} Clip hinzufügen</button>` : '';
    $('#tabs').innerHTML = tabs.map(([id, label, ico]) => `<button id="tab-${id}" role="tab" aria-selected="${view === id}" aria-controls="panel-${id}" tabindex="${view === id ? '0' : '-1'}" data-tab="${id}" class="tab-button">${icon(ico)}${label}</button>`).join('');
    $('#content').id = 'content';
    $('#content').setAttribute('aria-labelledby', 'tab-' + view);
    $('#content').setAttribute('data-active-panel', view);
    $('#content').innerHTML = `<section id="panel-${view}" role="tabpanel" aria-labelledby="tab-${view}"></section>`;
}
function render() { const focusId = document.activeElement?.id; renderHeader(); if (view === 'queue')
    renderQueue();
else if (view === 'autopilot')
    renderAutopilot();
else if (view === 'templates')
    renderTemplates();
else
    renderAccounts(); if (focusId)
    document.getElementById(focusId)?.focus({ preventScroll: true }); }
function card(c) {
    const review = c.status === 'review', pending = busy.has(c.id);
    return `<article class="queue-card card flex flex-col md:flex-row gap-4 p-3.5 md:items-center" data-status="${c.status}" data-clip="${c.id}" aria-label="${esc(c.title)}" aria-busy="${pending}">
 ${thumbnail(c)}<div class="clip-content flex-1 min-w-0 py-1"><div class="flex items-center gap-2.5 mb-2">${statusBadge(c.status)}${c.customLayout ? '<span class="text-xs text-text-secondary">· Eigenes Layout</span>' : ''}</div><h3 class="text-[15px] leading-6 font-medium text-text-primary">${esc(c.title)}</h3><p class="mt-1 text-[13px] text-text-secondary">${esc(c.source)} <span class="text-text-secondary px-1">/</span> earlysalty <span class="text-text-secondary px-1">·</span> ${formatViews(c.views)}</p>
 <div class="flex items-center flex-wrap gap-2 mt-3">${c.targets.map(platformIcon).join('')}<span class="text-xs text-text-secondary ml-0.5">${c.targets.length ? c.targets.map(p => PLATFORM_NAMES[p].replace(' Shorts', '').replace('Instagram Reels', 'Reels')).join(' · ') : 'Zielplattform im Menü wählen'}</span>${c.scheduledLabel && c.status === 'scheduled' ? `<span class="text-xs text-text-secondary md:ml-2">${icon('clock', 'inline h-3 w-3 mr-1')}${esc(c.scheduledLabel)}</span>` : ''}${c.status === 'published' ? `<span class="text-xs text-text-secondary">· ${esc(c.publishedLabel)}</span>` : ''}</div>
 ${c.error && c.status === 'error' ? `<p class="error-text mt-2">${esc(c.error)}</p>` : ''}${errors[c.id] ? `<p class="error-text mt-2" role="alert">${esc(errors[c.id])}</p>` : ''}</div>
 <div class="clip-actions flex items-center gap-2 self-stretch md:self-center shrink-0">${review ? `<button class="btn btn-primary" data-action="approve" data-id="${c.id}" ${pending || !c.targets.length ? 'disabled' : ''}>${icon('check')} ${pending ? 'Speichert…' : 'Freigeben'}</button><button class="btn icon-btn" data-action="archive" data-id="${c.id}" aria-label="Clip ablehnen: ${esc(c.title)}" title="Ablehnen" ${pending ? 'disabled' : ''}>${icon('x')}</button>` : c.status === 'new' ? '<span class="text-xs text-text-secondary mr-1">Wird aufbereitet</span>' : c.status === 'error' ? `<button class="btn" data-tab="accounts">Konto prüfen ${icon('arrow')}</button>` : ''}<button class="btn btn-ghost icon-btn" id="clip-more-${c.id}" data-action="menu" data-id="${c.id}" aria-label="Weitere Aktionen für ${esc(c.title)}" aria-expanded="${menuId === c.id}" aria-controls="clip-menu" ${pending ? 'disabled' : ''}>${icon('more')}</button></div></article>`;
}
function renderQueue() {
    if (!$('#panel-queue'))
        return;
    const m = queueMetrics(clips);
    const p = $('#panel-queue');
    p.innerHTML = `<div class="card grid grid-cols-2 xl:grid-cols-4 divide-x divide-white/[0.06] mb-7">${metric('Wartet auf Freigabe', m.review, 'Bereit für deine Entscheidung', 'check', 'text-accent')}${metric('Geplante Posts', m.scheduled, 'Ein Post je Zielplattform', 'calendar')}${metric('Clip-Vorrat', saved.pool.reicht_fuer_tage + ' <span class="text-base font-normal tracking-normal text-text-secondary">Tage</span>', saved.pool.posts_pro_woche + ' Posts pro Woche', 'layers')}${metric('Fehler', m.errors, m.errors ? 'Benötigt deine Aufmerksamkeit' : 'Keine offenen Fehler', 'warning', m.errors ? 'text-danger' : 'text-text-primary')}</div>
 <div class="flex flex-col xl:flex-row xl:items-center justify-between gap-4 mb-4"><div class="queue-filters flex items-center gap-0.5 overflow-scroll" aria-label="Clip-Status">${[['all', 'Alle'], ['new', 'Neu'], ['review', 'Freigabe'], ['scheduled', 'Geplant'], ['published', 'Gepostet'], ['error', 'Fehler'], ['archived', 'Archiv']].map(([id, label]) => `<button class="filter" data-filter="${id}" aria-pressed="${filter === id}">${label}${id === 'review' ? ` <span class="ml-1 text-xs text-text-secondary">${m.review}</span>` : ''}</button>`).join('')}</div><div class="flex items-center gap-2 min-w-0"><label class="relative flex-1"><span class="sr-only">Clips durchsuchen</span>${icon('search', 'absolute left-3 top-3 h-4 w-4 text-text-secondary')}<input id="search" class="field !pl-9 !min-h-10 xl:!w-52" type="search" placeholder="Clips durchsuchen…" value="${esc(search)}"></label><select id="platform-filter" class="field !w-32 !min-h-10" aria-label="Zielplattform filtern"><option value="all">Plattform</option>${PLATFORMS.map(p => `<option value="${p}" ${platform === p ? 'selected' : ''}>${p === 'youtube' ? 'YouTube' : p === 'tiktok' ? 'TikTok' : 'Reels'}</option>`).join('')}</select><div class="hidden sm:flex border border-border rounded-lg p-0.5"><button class="btn btn-ghost icon-btn !min-h-8 !w-8 !p-1.5 ${layout === 'list' ? '!bg-card-hover !text-text-primary' : ''}" data-layout="list" aria-label="Listenansicht" aria-pressed="${layout === 'list'}">${icon('list')}</button><button class="btn btn-ghost icon-btn !min-h-8 !w-8 !p-1.5 ${layout === 'grid' ? '!bg-card-hover !text-text-primary' : ''}" data-layout="grid" aria-label="Kartenansicht" aria-pressed="${layout === 'grid'}">${icon('grid')}</button></div></div></div>
 <div id="queue-list" class="${layout === 'grid' ? 'grid-view' : 'space-y-3'}"></div><div class="flex flex-wrap gap-3 items-center justify-between mt-5 text-xs text-text-secondary"><span id="result-count" role="status"></span><span>Zeiten in Europe/Berlin</span></div><div class="mt-8 flex flex-wrap gap-3 items-center justify-between rounded-lg border border-dashed border-border px-4 py-3"><div class="flex items-center gap-3 text-text-secondary">${icon('upload')}<span class="text-[13px]">Noch einen eigenen Clip auf Lager?</span></div><button class="btn btn-ghost !text-[13px] !min-h-8" data-action="upload">MP4 hinzufügen ${icon('arrow')}</button></div>`;
    renderCards();
}
function renderCards() { if (!$('#queue-list'))
    return; const selected = clips.filter(c => (filter === 'all' || c.status === filter) && (platform === 'all' || c.targets.includes(platform)) && c.title.toLocaleLowerCase('de').includes(search.toLocaleLowerCase('de'))); $('#queue-list').innerHTML = selected.length ? selected.map(card).join('') : `<div class="card py-16 px-6 text-center"><div class="text-text-secondary flex justify-center mb-4">${icon('check', 'h-8 w-8')}</div><h3 class="text-base font-medium">${filter === 'review' && !search ? 'Alles geprüft. Gut gemacht.' : 'Keine Clips gefunden'}</h3><p class="hint mt-2">${filter === 'review' && !search ? 'Deine freigegebenen Clips findest du unter „Geplant“.' : 'Wähle einen anderen Filter oder passe die Suche an.'}</p><button class="btn mt-5" data-filter="all">Alle Clips anzeigen</button></div>`; $('#result-count').textContent = selected.length + ' von ' + clips.length + ' Clips'; }
function fieldError(key) { return scheduleErrors[key] ? `<p class="error-text mt-2" id="error-${key.replace('.', '-')}" role="alert">${esc(scheduleErrors[key])}</p>` : ''; }
function switchButton(checked, action, label, extra = '') { return `<button type="button" class="switch" role="switch" aria-checked="${checked}" aria-label="${esc(label)}" data-action="${action}" ${extra}></button>`; }
function renderAutopilot() {
    if (!$('#panel-autopilot'))
        return;
    const dirty = JSON.stringify(draft) !== JSON.stringify(saved);
    $('#panel-autopilot').innerHTML = `<div class="grid gap-6 xl:grid-cols-[minmax(0,1fr)_280px]"><form id="schedule-form" class="min-w-0 space-y-5" novalidate>
 <section class="card p-5 sm:p-6"><div class="flex items-start justify-between gap-4"><div><h2 class="text-base font-medium">Freigabe & Automatisierung</h2><p class="hint mt-1">Wie viel Kontrolle möchtest du vor jedem Post?</p></div><span class="badge badge-violet">${draft.approval_mode === 'manual' ? 'Mit Freigabe' : 'Automatisiert'}</span></div><div class="grid sm:grid-cols-3 gap-3 mt-5">${[['manual', 'Mit Freigabe', 'Jeder Clip wartet auf dein Okay.'], ['veto_window', 'Mit Einspruch', 'Geplant, bis du widersprichst.'], ['full_auto', 'Vollautomatisch', 'Ohne manuelle Sichtung.']].map(([id, title, desc]) => `<label class="rounded-lg border ${draft.approval_mode === id ? 'border-primary/50 bg-primary/[0.06]' : 'border-border'} p-3.5 cursor-pointer"><span class="flex items-center gap-2.5 text-[13px] font-medium"><input type="radio" name="approval_mode" value="${id}" ${draft.approval_mode === id ? 'checked' : ''} class="accent-primary">${title}</span><span class="block text-xs leading-5 text-text-secondary mt-2">${desc}</span></label>`).join('')}</div>${draft.approval_mode === 'full_auto' ? '<p class="mt-4 rounded-lg bg-warning/[0.06] border border-warning/15 px-3 py-2.5 text-[13px] text-warning">Clips werden ohne Freigabe veröffentlicht. Die Änderung wird erst beim Speichern übernommen.</p>' : ''}</section>
 <section class="card"><div class="p-5 sm:p-6 border-b border-border"><div class="flex items-center justify-between gap-3"><div><h2 class="text-base font-medium">Plattformen & Posting-Zeiten</h2><p class="hint mt-1">Eigene Intervalle, ohne einen gemeinsamen Kalender zu überladen.</p></div>${icon('calendar', 'h-5 w-5 text-text-secondary')}</div><label class="flex flex-wrap gap-3 items-center mt-5"><span class="text-[13px] text-text-secondary">Zeitzone</span><select name="timezone" class="field !w-auto !min-h-9 !py-1.5">${['Europe/Berlin', 'Europe/London', 'America/New_York', 'UTC'].map(z => `<option ${draft.timezone === z ? 'selected' : ''}>${z}</option>`).join('')}</select><span class="text-xs text-text-secondary">Sommerzeit wird berücksichtigt</span></label></div>
 <div class="divide-y divide-white/[0.07]">${draft.platforms.map(p => `<fieldset class="p-5 sm:p-6" data-platform="${p.platform}"><legend class="sr-only">${PLATFORM_NAMES[p.platform]}</legend><div class="flex items-center gap-3 mb-5">${platformIcon(p.platform)}<div class="flex-1"><p class="text-sm font-medium">${PLATFORM_NAMES[p.platform]}</p><p class="text-xs text-text-secondary mt-0.5">${p.auto_post ? 'Posting-Zeitplan aktiv' : 'Pausiert. Werte bleiben erhalten.'}</p></div>${switchButton(p.auto_post, 'schedule-toggle', PLATFORM_NAMES[p.platform] + ' automatisch posten', `data-platform="${p.platform}"`)}</div>
 <div class="grid grid-cols-2 sm:grid-cols-[1fr_1fr_1.5fr] gap-4"><label class="text-[13px] text-text-secondary">Posts pro Woche<input class="field mt-2" inputmode="numeric" type="number" min="0" max="70" step="1" name="${p.platform}.posts_per_week" value="${esc(p.posts_per_week)}" aria-invalid="${!!scheduleErrors[p.platform + '.week']}" ${scheduleErrors[p.platform + '.week'] ? `aria-describedby="error-${p.platform}-week"` : ''}>${fieldError(p.platform + '.week')}</label><label class="text-[13px] text-text-secondary">Höchstens pro Tag<input class="field mt-2" inputmode="numeric" type="number" min="0" max="10" step="1" name="${p.platform}.max_posts_per_day" value="${esc(p.max_posts_per_day)}" aria-invalid="${!!scheduleErrors[p.platform + '.day']}" ${scheduleErrors[p.platform + '.day'] ? `aria-describedby="error-${p.platform}-day"` : ''}>${fieldError(p.platform + '.day')}</label><div class="col-span-2 sm:col-span-1"><span class="text-[13px] text-text-secondary">Uhrzeiten</span><div class="flex flex-wrap items-center gap-2 mt-2">${p.post_times.map((time, i) => `<div class="flex items-center gap-1"><input type="time" class="field !w-[128px]" name="${p.platform}.time.${i}" value="${time}" aria-label="${PLATFORM_NAMES[p.platform]} Uhrzeit ${i + 1}" aria-invalid="${!!scheduleErrors[p.platform + '.times']}">${p.post_times.length > 1 ? `<button type="button" class="btn btn-ghost icon-btn !w-7 !p-1" data-action="remove-time" data-platform="${p.platform}" data-index="${i}" aria-label="Uhrzeit ${i + 1} entfernen">${icon('x', 'h-3 w-3')}</button>` : ''}</div>`).join('')}<button class="btn btn-ghost icon-btn !w-8" type="button" data-action="add-time" data-platform="${p.platform}" aria-label="Uhrzeit für ${PLATFORM_NAMES[p.platform]} hinzufügen" ${p.post_times.length >= 12 ? 'disabled' : ''}>${icon('plus')}</button></div>${fieldError(p.platform + '.times')}</div></div></fieldset>`).join('')}</div></section>
 <section class="card p-5 sm:p-6"><h2 class="text-base font-medium mb-4">Filter & Aufbereitung</h2><div class="space-y-5">${draft.categories.map(c => `<div class="flex items-center justify-between gap-5"><div><p class="text-sm">${c.display_name}</p><p class="hint mt-0.5">${c.enrichment_enabled ? 'Mit Aufbereitung von Titel, Transkript und Untertiteln.' : 'Nur posten, ohne spielbezogene KI-Aufbereitung.'}</p></div>${switchButton(c.auto_post, 'category-toggle', c.display_name + ' automatisch posten', `data-category="${c.category_key}"`)}</div>`).join('')}<div class="flex items-center justify-between gap-5 border-t border-border-soft pt-5"><div><p class="text-sm">Untertitel einbrennen</p><p class="hint mt-0.5">Bessere Verständlichkeit auch ohne Ton.</p></div>${switchButton(draft.subtitles_enabled, 'subtitles', 'Untertitel einbrennen')}</div></div></section>
 <div class="save-bar flex flex-wrap items-center justify-between gap-3"><span id="save-status" class="text-[13px] text-text-secondary" role="status">${dirty ? 'Ungespeicherte Änderungen' : savedAt ? 'Gespeichert um ' + savedAt : 'Keine ungespeicherten Änderungen'}</span><div class="flex items-center gap-2"><button class="btn btn-ghost" data-action="reset-schedule" type="button" ${dirty ? '' : 'disabled'}>Verwerfen</button><button class="btn btn-primary" type="submit" ${dirty ? '' : 'disabled'}>${icon('check')} Änderungen speichern</button></div></div></form>
 <aside class="space-y-4"><section class="card p-5"><div class="flex items-center justify-between text-sm text-text-secondary"><h2>Dein Posting-Rhythmus</h2>${icon('clock')}</div><p class="metric-number mt-5">${draft.platforms.filter(p => p.auto_post).reduce((n, p) => n + (Number(p.posts_per_week) || 0), 0)} <span class="text-sm font-normal tracking-normal text-text-secondary">Posts / Woche</span></p><p class="hint mt-3">${draft.platforms.filter(p => p.auto_post).length} aktive Plattformen. Die Uhrzeiten gelten jeweils in ${esc(draft.timezone)}.</p><div class="flex gap-1.5 mt-5" aria-hidden="true">${['M', 'D', 'M', 'D', 'F', 'S', 'S'].map((d, i) => `<div class="flex-1 text-center"><div class="h-7 rounded ${i % 2 === 0 ? 'bg-primary/30' : 'bg-card-hover'}"></div><p class="text-[10px] text-text-secondary mt-2">${d}</p></div>`).join('')}</div><p class="text-xs text-text-secondary mt-3">Schematisch, keine echten Termine</p></section><section class="rounded-xl border border-success/10 bg-success/[0.025] p-5"><p class="text-sm text-success flex items-center gap-2">${icon('layers')} Dein Vorrat reicht für ${saved.pool.reicht_fuer_tage} Tage</p><p class="hint mt-2">Beispielprognose aus dem Demo-Datensatz. Die Live-Prognose kommt vom Server.</p></section><section class="px-1"><h3 class="text-sm font-medium text-text-primary">Kontrolle bleibt bei dir</h3><p class="hint mt-2">Freigaben und geplante Posts bleiben jederzeit in der Pipeline sichtbar. Noch nicht gestartete Posts lassen sich stoppen.</p><p class="hint mt-4">Ein frei wählbarer zeitlicher Vorlauf ist in der vorhandenen API noch nicht enthalten. Deshalb wird hier kein wirkungsloser Schalter angeboten.</p></section></aside></div>`;
}
function renderTemplates() { $('#panel-templates').innerHTML = `<div class="flex items-center justify-between gap-4 mb-6"><div><h2 class="text-base font-medium">Dein Standard-Layout</h2><p class="hint mt-1">Für neue Clips. Individuelle Anpassungen überschreiben nur den jeweiligen Clip.</p></div><span class="text-xs text-text-secondary">1080 × 1920 · 9:16</span></div><div class="grid gap-5 md:grid-cols-3">${[['focus', 'Gameplay Focus', 'Volle Aufmerksamkeit auf deinen Spielmoment.'], ['pip', 'Facecam & Gameplay', 'Deine Reaktion, ohne das Spiel zu verdecken.'], ['split', 'Split Screen', 'Facecam oben, das Gameplay darunter.']].map(([id, title, desc]) => `<article class="card overflow-hidden"><div class="bg-card/40 px-8 py-7 flex justify-center"><div class="portrait ${id}"><div class="game"></div><div class="cam"></div><div class="captions"></div></div></div><div class="p-5 border-t border-border-soft"><div class="flex items-center justify-between gap-3"><h3 class="text-sm font-medium">${title}</h3>${id === 'pip' ? '<span class="badge badge-violet">Standard</span>' : ''}</div><p class="hint mt-2 min-h-10">${desc}</p><button class="btn w-full mt-5" data-action="template" data-template="${id}">${icon('crop')} Im Editor öffnen</button></div></article>`).join('')}</div><div class="card mt-6 p-5 flex flex-wrap items-start gap-4">${icon('help', 'h-5 w-5 text-text-secondary')}<div class="flex-1"><p class="text-sm font-medium">Der Editor ist ein eigener Arbeitsbereich.</p><p class="hint mt-1">Große Vorschau, klarer Geltungsbereich und Speichern ohne Ablenkung durch die Pipeline. Die tatsächliche Videoverarbeitung bleibt bei eurem bestehenden Cropper.</p></div></div>`; }
function renderAccounts() { if (!$('#panel-accounts'))
    return; $('#panel-accounts').innerHTML = `<section class="card max-w-4xl"><div class="p-6 border-b border-border"><h2 class="text-base font-medium">Verbundene Konten</h2><p class="hint mt-1">Ein eindeutiger Status für jede Quelle und jedes Ziel.</p></div>${[['twitch', 'Twitch', 'Quelle · Clips von earlysalty', true], ...PLATFORMS.map(p => [p, PLATFORM_NAMES[p], accountConnected[p] ? 'Zielkonto · @earlysalty' : 'Autorisierung muss erneuert werden', accountConnected[p]])].map(([id, title, subtitle, connected]) => `<div class="flex flex-wrap items-center gap-4 p-5 sm:p-6 border-b last:border-0 border-border-soft"><div class="w-11 h-11 rounded-xl bg-card-hover/60 border border-border-soft flex items-center justify-center text-text-primary">${icon(id === 'twitch' ? 'film' : 'link', 'h-5 w-5')}</div><div class="flex-1 min-w-[140px]"><h3 class="text-sm font-medium">${title}</h3><p class="hint mt-1">${subtitle}</p></div><span class="badge badge-${connected ? 'green' : 'red'}">${connected ? 'Verbunden' : 'Abgelaufen'}</span>${id === 'twitch' ? '<span class="text-xs text-text-secondary ml-2">Verwaltung im Dashboard</span>' : `<button class="btn ${connected ? 'btn-ghost' : ''}" data-action="account" data-platform="${id}">${connected ? 'Details' : 'Verbindung simulieren'} ${icon(connected ? 'chevron' : 'link', 'h-3.5 w-3.5')}</button>`}</div>`).join('')}</section><p class="hint mt-4 max-w-3xl">Diese Demo zeigt Beispielkonten. Kein Login wird gestartet und es werden keine Tokens gespeichert. Im echten Dashboard wird der bestehende OAuth-Redirect verwendet.</p><section class="card max-w-4xl mt-6 p-6"><div class="flex items-center justify-between gap-4"><div><h2 class="text-base font-medium">Vorschau-Einstellungen</h2><p class="hint mt-1">Einmalig einen fehlgeschlagenen Speichervorgang testen.</p></div>${switchButton(failNext, 'fail-next', 'Nächsten Serverfehler simulieren')}</div><p class="hint mt-4">Die Demo speichert ausschließlich im Arbeitsspeicher dieses Tabs. Neu laden setzt sie zurück.</p></section>`; }
function closeMenu(restore = false) { const btn = menuId == null ? null : document.querySelector(`[data-action="menu"][data-id="${menuId}"]`); $('#clip-menu').hidden = true; $('#clip-menu').innerHTML = ''; menuId = null; if (btn) {
    btn.setAttribute('aria-expanded', 'false');
    if (restore)
        btn.focus();
} }
function openMenu(id, button) { if (menuId === id) {
    closeMenu(true);
    return;
} closeMenu(); menuId = id; const c = clips.find(c => c.id === id); const el = $('#clip-menu'); el.innerHTML = `${c.status === 'review' ? `<fieldset class="p-2.5 border-b border-border mb-1"><legend class="text-xs text-text-secondary mb-1">Zielplattformen</legend>${PLATFORMS.map(p => `<label class="flex items-center gap-2.5 py-2 text-[13px] ${accountConnected[p] ? 'text-text-primary' : 'text-text-secondary'}"><input type="checkbox" data-target="${p}" data-id="${id}" ${c.targets.includes(p) ? 'checked' : ''} ${accountConnected[p] ? '' : 'disabled'}>${PLATFORM_NAMES[p]}${accountConnected[p] ? '' : ' · offline'}</label>`).join('')}</fieldset>` : ''}<button class="action" data-action="edit-layout" data-id="${id}">${icon('crop')} Layout anpassen</button><button class="action" data-action="edit-transcript" data-id="${id}">${icon('text')} Transkript bearbeiten</button>${c.status === 'scheduled' ? `<button class="action !text-warning" data-action="stop" data-id="${id}">${icon('clock')} Geplanten Post stoppen</button>` : ''}${c.status !== 'archived' ? `<button class="action" data-action="archive" data-id="${id}">${icon('archive')} ${c.status === 'review' ? 'Ablehnen' : 'Archivieren'}</button>` : ''}`; el.hidden = false; button.setAttribute('aria-expanded', 'true'); const r = button.getBoundingClientRect(); el.style.left = Math.max(8, Math.min(window.innerWidth - 260, r.right - 252)) + 'px'; const height = el.offsetHeight; el.style.top = (r.bottom + 8 + height < innerHeight ? r.bottom + 8 : Math.max(8, r.top - height - 8)) + 'px'; el.querySelector('input:not(:disabled),button')?.focus(); }
function showDialog(title, subtitle, body, small = false) { const trigger = menuId == null ? document.activeElement : document.querySelector(`[data-action="menu"][data-id="${menuId}"]`); closeMenu(); const el = $('#dialog'); if (el.open)
    return; activeModal = { focus: trigger, overflow: document.body.style.overflow }; el.className = 'modal' + (small ? ' small' : ''); el.innerHTML = `<header class="modal-header"><div class="min-w-0"><p class="text-xs text-accent mb-1">${subtitle}</p><h2 id="dialog-title" class="text-lg font-medium truncate">${esc(title)}</h2></div><button class="btn btn-ghost icon-btn" data-action="close-dialog" aria-label="Dialog schließen">${icon('x')}</button></header><div class="modal-body">${body}</div>`; document.body.style.overflow = 'hidden'; el.showModal(); }
function closeDialog(force = false) { const el = $('#dialog'); if (el.dataset.busy === 'true' && !force)
    return; if (el.querySelector('[data-dirty="true"]') && !force && !window.confirm('Ungespeicherte Änderungen verwerfen?'))
    return; el.close(); if (activeModal) {
    document.body.style.overflow = activeModal.overflow;
    if (activeModal.focus?.isConnected)
        activeModal.focus.focus({ preventScroll: true });
} activeModal = null; el.removeAttribute('data-busy'); }
function openEditor(c = null, kind = 'layout', template = null) {
    const settings = c?.demoLayout || defaultDemoLayout;
    template = template || settings.template;
    const title = c?.title || 'Standard-Layout';
    const scope = c ? 'Gilt nur für diesen Clip' : 'Gilt für neue Clips dieses Kanals';
    if (kind === 'transcript') {
        showDialog(title, scope, `<form id="transcript-form" data-id="${c.id}"><label class="text-sm font-medium" for="transcript">Transkript</label><p class="hint mt-1 mb-4">Prüfe Namen, Spielbegriffe und den Wortlaut vor dem Speichern.</p><textarea id="transcript" name="transcript" class="field min-h-48 leading-7 resize-y" maxlength="20000">${esc(c.transcript || 'Für diesen Demo-Clip ist noch kein Transkript hinterlegt.')}</textarea><div class="flex justify-end gap-2 mt-5"><button class="btn btn-ghost" type="button" data-action="close-dialog">Abbrechen</button><button class="btn btn-primary" type="submit">Transkript speichern</button></div><p id="dialog-error" class="error-text mt-3" role="alert"></p></form>`, true);
        return;
    }
    showDialog(title, scope, `<form id="layout-form" data-id="${c?.id || ''}" class="grid md:grid-cols-[1fr_280px] gap-6"><div class="rounded-xl bg-bg border border-border min-h-96 flex flex-col items-center justify-center p-6"><div id="editor-portrait" class="portrait ${template}" style="max-width:210px"><div class="game"></div><div class="cam"></div><div class="captions"></div></div><p class="text-xs text-text-secondary mt-5">Schematische Vorschau · 1080 × 1920</p></div><div class="layout-controls space-y-6"><div><h3 class="text-sm font-medium">Format & Ausschnitt</h3><p class="hint mt-1">Der Fokus bleibt auf deinem Clip.</p></div><label class="block">Layout<select name="template" id="editor-template" class="field mt-2"><option value="focus" ${template === 'focus' ? 'selected' : ''}>Gameplay Focus</option><option value="pip" ${template === 'pip' ? 'selected' : ''}>Facecam & Gameplay</option><option value="split" ${template === 'split' ? 'selected' : ''}>Split Screen</option></select></label><label class="block">Horizontale Position<input type="range" name="position" id="position-slider" min="0" max="100" value="${settings.position}"><span class="hint" id="position-value">${settings.position} %</span></label><label class="block">Facecam-Größe<input type="range" name="cam" id="cam-slider" min="20" max="50" value="${settings.cam}"><span class="hint" id="cam-value">${settings.cam} %</span></label><p class="hint">In der Produktintegration wird hier euer vorhandener 9:16-Cropper eingesetzt. Diese Vorschau verarbeitet keine Videodatei.</p><div class="flex flex-col gap-2 pt-2"><button class="btn btn-primary" type="submit">Layout speichern</button><button class="btn btn-ghost" type="button" data-action="close-dialog">Abbrechen</button></div><p id="dialog-error" class="error-text" role="alert"></p></div></form>`);
    document.getElementById("position-slider").dispatchEvent(new Event("input", { bubbles: true }));
    document.getElementById("cam-slider").dispatchEvent(new Event("input", { bubbles: true }));
    document.getElementById("layout-form").dataset.dirty = "false";
}
async function mutateClip(id, action) { if (busy.has(id))
    return; const c = clips.find(c => c.id === id); if (!c)
    return; if (action === 'approve' && (c.status !== 'review' || !c.targets.length))
    return; closeMenu(); busy.add(id); delete errors[id]; renderCards(); try {
    await simulate();
    if (action === 'approve') {
        c.status = 'scheduled';
        c.scheduledLabel = 'Nächster freier Slot · Demo';
    }
    else if (action === 'stop') {
        c.status = 'review';
        delete c.scheduledLabel;
    }
    else {
        c.status = 'archived';
        delete c.scheduledLabel;
    }
    busy.delete(id);
    renderQueue();
    toast(action === 'approve' ? 'Clip freigegeben und in der Demo eingeplant.' : action === 'stop' ? 'Geplanter Post gestoppt. Der Clip wartet wieder auf Freigabe.' : 'Clip archiviert. Es wird nichts veröffentlicht.');
}
catch (e) {
    busy.delete(id);
    errors[id] = e.message;
    renderCards();
} }
async function saveSchedule() { if (schedulePending)
    return; scheduleErrors = validateSchedule(draft); if (Object.keys(scheduleErrors).length) {
    renderAutopilot();
    $('#schedule-form [aria-invalid=true]')?.focus();
    return;
} schedulePending = true; const form = $('#schedule-form'); form.querySelectorAll('button,input,select').forEach(e => e.disabled = true); $('#save-status').textContent = 'Wird gespeichert…'; try {
    await simulate();
    saved = normalizeSchedule(clone(draft));
    draft = clone(saved);
    savedAt = new Date().toLocaleTimeString('de-DE', { hour: '2-digit', minute: '2-digit' });
    renderAutopilot();
    toast('Zeitplan in dieser Demo gespeichert. Keine echten Posts werden ausgelöst.');
}
catch (e) {
    renderAutopilot();
    if ($('#save-status')) {
        $('#save-status').textContent = e.message;
        $('#save-status').classList.add('text-danger');
    }
    else
        toast(e.message);
}
finally {
    schedulePending = false;
} }
function updateDirty() { const dirty = JSON.stringify(draft) !== JSON.stringify(saved); $('#save-status').textContent = dirty ? 'Ungespeicherte Änderungen' : 'Keine ungespeicherten Änderungen'; $('#schedule-form button[type=submit]').disabled = !dirty; $('#schedule-form [data-action=reset-schedule]').disabled = !dirty; }
function changeView(id) { if (schedulePending) {
    toast('Zeitplan wird gespeichert…');
    return;
} if (!tabs.some(t => t[0] === id))
    return; closeMenu(); view = id; render(); history.replaceState(null, '', '#' + id); }
document.addEventListener('click', async (e) => {
    const target = e.target.closest('[data-action],[data-tab],[data-filter],[data-layout]');
    if (!target) {
        if (!e.target.closest('#clip-menu'))
            closeMenu();
        return;
    }
    if (target.disabled || schedulePending)
        return;
    if (target.dataset.tab) {
        changeView(target.dataset.tab);
        return;
    }
    if (target.dataset.filter) {
        filter = target.dataset.filter;
        search = '';
        renderQueue();
        return;
    }
    if (target.dataset.layout) {
        layout = target.dataset.layout;
        renderQueue();
        return;
    }
    const { action, id } = target.dataset;
    const c = clips.find(c => c.id === Number(id));
    if (action === 'menu') {
        openMenu(Number(id), target);
        return;
    }
    if (['approve', 'archive', 'stop'].includes(action)) {
        await mutateClip(Number(id), action);
        return;
    }
    if (action === 'edit-layout' || action === 'edit-transcript') {
        openEditor(c, action === 'edit-layout' ? 'layout' : 'transcript');
        return;
    }
    if (action === 'template') {
        openEditor(null, 'layout', target.dataset.template);
        return;
    }
    if (action === 'close-dialog') {
        closeDialog();
        return;
    }
    if (action === 'schedule-toggle') {
        draft.platforms.find(p => p.platform === target.dataset.platform).auto_post = target.getAttribute('aria-checked') !== 'true';
        renderAutopilot();
        return;
    }
    if (action === 'category-toggle') {
        const item = draft.categories.find(c => c.category_key === target.dataset.category);
        item.auto_post = !item.auto_post;
        renderAutopilot();
        return;
    }
    if (action === 'subtitles') {
        draft.subtitles_enabled = !draft.subtitles_enabled;
        renderAutopilot();
        return;
    }
    if (action === 'add-time') {
        const p = draft.platforms.find(p => p.platform === target.dataset.platform);
        const unused = Array.from({ length: 24 }, (_, i) => String(i).padStart(2, '0') + ':00').find(t => !p.post_times.includes(t));
        p.post_times.push(unused || '12:00');
        renderAutopilot();
        return;
    }
    if (action === 'remove-time') {
        draft.platforms.find(p => p.platform === target.dataset.platform).post_times.splice(Number(target.dataset.index), 1);
        renderAutopilot();
        return;
    }
    if (action === 'reset-schedule') {
        draft = clone(saved);
        scheduleErrors = {};
        renderAutopilot();
        return;
    }
    if (action === 'fail-next') {
        failNext = !failNext;
        target.setAttribute('aria-checked', String(failNext));
        return;
    }
    if (action === 'upload') {
        showDialog('Eigenen Clip hinzufügen', 'Lokaler Demo-Import', `<form id="upload-form"><label for="clip-file" class="block text-sm font-medium mb-3">MP4-Datei auswählen</label><input id="clip-file" name="clip-file" type="file" accept="video/mp4,.mp4" class="field" required><p class="hint mt-3">Maximal 200 MB. Die Datei bleibt auf deinem Gerät. Die Demo übernimmt nur den Dateinamen in die Pipeline.</p><div class="flex justify-end mt-5"><button class="btn btn-primary" type="submit">Zur Demo hinzufügen</button></div><p id="dialog-error" class="error-text mt-3" role="alert"></p></form>`, true);
        return;
    }
    if (action === 'analytics') {
        const m = queueMetrics(clips);
        showDialog('Dein Pipeline-Stand', 'Auswertung der Demodaten', `<p class="hint">Hier werden keine erfundenen Plattform-Analytics angezeigt. Die drei Zahlen stammen direkt aus den Beispielclips.</p><div class="grid grid-cols-3 gap-2 mt-5">${metric('Freigabe', m.review, 'Clips', 'check')}${metric('Geplant', m.scheduled, 'Plattform-Posts', 'calendar')}${metric('Fehler', m.errors, 'Clips', 'warning')}</div>`, true);
        return;
    }
    if (action === 'account') {
        const p = target.dataset.platform;
        if (accountConnected[p])
            showDialog(PLATFORM_NAMES[p], 'Verbindungsdetails · Demo', `<p class="text-sm">Beispielkonto: @earlysalty</p><p class="hint mt-3">Im Live-System werden Ablaufzeit und Auth-Status vom bestehenden Plattform-Endpunkt geladen. „Verbunden“ ist kein Versprechen über eine Upload-Freigabe des Anbieters.</p>`, true);
        else {
            target.disabled = true;
            await new Promise(r => setTimeout(r, 250));
            accountConnected[p] = true;
            renderAccounts();
            toast('Demo-Verbindung hergestellt. Es wurde kein OAuth-Login ausgeführt.');
        }
        return;
    }
    if (action === 'help') {
        showDialog('So funktioniert diese Vorschau', 'Social Studio · UI-Referenz', `<div class="space-y-4 hint"><p>Alle Clips, Bilder, Konten und Kennzahlen sind Beispieldaten. Es gibt keine Verbindung zu eurem Produktionssystem.</p><p>Freigeben, Archivieren, Filtern, Zeitplan speichern und die Editor-Dialoge lassen sich testen. Die Daten werden nur im geöffneten Tab gehalten.</p><p>Im Code-Paket liegen die wiederverwendbaren React-Komponenten und eine genaue Zuordnung zu euren bestehenden API-Funktionen.</p></div>`, true);
        return;
    }
    if (action === 'reset-demo') {
        if (busy.size)
            return;
        defaultDemoLayout = { template: 'pip', position: 50, cam: 32 };
        clips = clone(DEMO_CLIPS);
        saved = clone(DEFAULT_SCHEDULE);
        draft = clone(saved);
        errors = {};
        scheduleErrors = {};
        search = '';
        filter = 'review';
        platform = 'all';
        failNext = false;
        busy.clear();
        accountConnected = { youtube: true, tiktok: true, instagram: false };
        changeView('queue');
        toast('Demodaten zurückgesetzt.');
    }
});
document.addEventListener('input', e => { const el = e.target; if (el.id === 'search') {
    search = el.value;
    renderCards();
    return;
} if (el.closest('#schedule-form')) {
    const [p, key, i] = el.name.split('.');
    if (p && key) {
        const item = draft.platforms.find(v => v.platform === p);
        if (item) {
            if (key === 'time')
                item.post_times[Number(i)] = el.value;
            else
                item[key] = el.value;
        }
        updateDirty();
    }
    return;
} if (el.closest('#dialog form'))
    el.closest('form').dataset.dirty = 'true'; if (el.id === 'position-slider') {
    const value = Number(el.value);
    $('#position-value').textContent = value + ' %';
    $('#editor-portrait .game').style.backgroundPosition = value + '% center';
    $('#editor-portrait .game').style.transform = `translateX(${(value - 50) / 4}%) scale(1.25)`;
} if (el.id === 'cam-slider') {
    $('#cam-value').textContent = el.value + ' %';
    $('#editor-portrait .cam').style.width = el.value + '%';
} });
document.addEventListener('change', e => { const el = e.target; if (el.id === 'platform-filter') {
    platform = el.value;
    renderCards();
    return;
} if (el.dataset.target) {
    const c = clips.find(c => c.id === Number(el.dataset.id));
    c.targets = el.checked ? [...new Set([...c.targets, el.dataset.target])] : c.targets.filter(p => p !== el.dataset.target);
    renderCards();
    return;
} if (el.name === 'approval_mode') {
    draft.approval_mode = el.value;
    renderAutopilot();
    return;
} if (el.name === 'timezone') {
    draft.timezone = el.value;
    renderAutopilot();
    return;
} if (el.id === 'editor-template')
    $('#editor-portrait').className = 'portrait ' + el.value; });
document.addEventListener('submit', async (e) => { e.preventDefault(); if (e.target.id === 'schedule-form') {
    await saveSchedule();
    return;
} const form = e.target; if (!['layout-form', 'transcript-form', 'upload-form'].includes(form.id))
    return; const submit = form.querySelector('button[type=submit]'); submit.disabled = true; $('#dialog').dataset.busy = 'true'; try {
    if (form.id === 'upload-form') {
        const file = form.querySelector('input[type=file]').files[0];
        if (!file || !file.name.toLowerCase().endsWith('.mp4'))
            throw Error('Bitte eine MP4-Datei auswählen.');
        if (file.size > 200 * 1024 * 1024)
            throw Error('Die Datei ist größer als 200 MB.');
        await simulate();
        clips.unshift({ id: Math.max(...clips.map(c => c.id)) + 1, title: file.name.replace(/\.mp4$/i, ''), source: 'Eigener Upload', duration: null, views: null, targets: [], scene: 0, status: 'new' });
        filter = 'new';
        view = 'queue';
    }
    else {
        await simulate();
        const c = clips.find(c => c.id === Number(form.dataset.id));
        if (form.id === 'transcript-form' && c)
            c.transcript = form.querySelector('textarea').value;
        if (form.id === 'layout-form') {
            const settings = { template: form.querySelector('#editor-template').value, position: Number(form.querySelector('#position-slider').value), cam: Number(form.querySelector('#cam-slider').value) };
            if (c) {
                c.customLayout = true;
                c.demoLayout = settings;
            }
            else
                defaultDemoLayout = settings;
        }
    }
    closeDialog(true);
    render();
    toast(form.id === 'upload-form' ? 'Dateiname zur Demo hinzugefügt. Es wurde keine Datei hochgeladen.' : 'Änderung in dieser Demo gespeichert.');
}
catch (err) {
    $('#dialog-error').textContent = err.message;
    submit.disabled = false;
    $('#dialog').dataset.busy = 'false';
} });
document.addEventListener('keydown', e => { if (e.key === 'Tab' && $('#dialog').open) {
    const items = [...$('#dialog').querySelectorAll('button:not(:disabled),input:not(:disabled),select:not(:disabled),textarea:not(:disabled),a[href],[tabindex="0"]')].filter(el => el.getClientRects().length);
    const first = items[0], last = items.at(-1);
    if ((e.shiftKey && document.activeElement === first) || (!e.shiftKey && document.activeElement === last)) {
        e.preventDefault();
        (e.shiftKey ? last : first)?.focus();
    }
} if (e.key === 'Escape' && menuId != null) {
    e.preventDefault();
    closeMenu(true);
} if (e.target.matches('[role=tab]') && ['ArrowRight', 'ArrowLeft', 'Home', 'End'].includes(e.key)) {
    e.preventDefault();
    const index = tabs.findIndex(t => t[0] === e.target.dataset.tab);
    const next = e.key === 'Home' ? 0 : e.key === 'End' ? 3 : (index + (e.key === 'ArrowRight' ? 1 : 3)) % 4;
    changeView(tabs[next][0]);
    $('#tab-' + view).focus();
} });
$('#dialog').addEventListener('cancel', e => { e.preventDefault(); closeDialog(); });
window.addEventListener('resize', () => closeMenu());
window.addEventListener('scroll', () => closeMenu(), true);
window.addEventListener('beforeunload', e => { if (JSON.stringify(draft) !== JSON.stringify(saved)) {
    e.preventDefault();
    e.returnValue = '';
} });
const initial = location.hash.slice(1);
if (tabs.some(t => t[0] === initial))
    view = initial;
render();
