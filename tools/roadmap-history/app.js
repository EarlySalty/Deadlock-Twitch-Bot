/* Git-derived strings enter this page only through textContent and checked URLs. */
'use strict';
const data = JSON.parse(document.getElementById('roadmap-data').textContent);
if (data.schemaVersion !== 3 || data.rootId !== 'genesis') throw new Error('Ungültiger Feature-Graph');
const byId = new Map(data.nodes.map(node => [node.id, node]));
const commits = new Map(data.commits.map(commit => [commit.id, commit]));
const children = new Map(data.nodes.map(node => [node.id, []]));
for (const node of data.nodes) if (node.parentId) children.get(node.parentId).push(node.id);
const $ = id => document.getElementById(id);
const categories = {core:'Kern', twitch:'Twitch', dashboard:'Dashboard', api:'API', community:'Community'};
const displayTitle = node => node.id === 'genesis' ? 'Twitch-Bot: erster belegter Stand' : node.title;
const number = value => Number(value).toLocaleString('de-DE');
const day = date => Date.parse(date + 'T00:00:00Z') / 86400000;
const fmt = date => new Intl.DateTimeFormat('de-DE', {day:'numeric',month:'short',year:'numeric',timeZone:'UTC'}).format(new Date(date + 'T00:00:00Z'));
const make = (tag, className, content) => {const node=document.createElement(tag); if(className) node.className=className; if(content!==undefined) node.textContent=content; return node;};
const button = (label, handler, className='') => {const node=make('button',className,label); node.type='button'; node.addEventListener('click',handler); return node;};
const safeCommit = id => /^[0-9a-f]{40,64}$/.test(id);
const safePr = value => /^https:\/\/github\.com\/EarlySalty\/Deadlock-(?:Twitch-)?Bots?\/pull\/[1-9]\d*$/.test(value);
const link = (label, href) => {const a=make('a','',label); a.href=href; a.target='_blank'; a.rel='noopener noreferrer'; return a;};
const params = new URLSearchParams(location.search);
let view = ['overview','features','all'].includes(params.get('view')) ? params.get('view') : 'overview';
let scale = 1, offsetX = 20, offsetY = 20, layout = null, active = null, focusReturn = null;
let focusRoot = null;
let collapse = new Set((params.get('collapsed')||'').split(',').filter(id => byId.has(id)));
let page = 0, drag = null;
const controls = ['search','category','kind','from','to','maintenance'];
for (const id of controls) {const value=params.get(id); if(id==='maintenance') $(id).checked=value==='1'; else if(value) $(id).value=value;}
if (params.has('feature')) {const raw=params.get('feature'); const id=raw==='product'?'genesis':byId.has(raw)?raw:'feature:'+raw; if(byId.has(id)) {view='features'; active=id;}}
if (params.has('focus')) {const raw=params.get('focus'); const id=raw==='product'?'genesis':byId.has(raw)?raw:'feature:'+raw; if(byId.has(id)) {view='all'; focusRoot=id;}}
$('from').min=data.coverage.first; $('from').max=data.coverage.last;
$('to').min=data.coverage.first; $('to').max=data.coverage.last;
$('snapshot-date').textContent=fmt(data.coverage.last);
$('snapshot-count').textContent=number(data.stats.commits)+' eindeutige Commits · '+number(data.stats.features)+' Funktionsfamilien';
$('snapshot-revision').textContent='Daten '+data.revision.slice(0,8)+' · Ansicht '+String(data.rendererRevision||'').slice(0,8);
$('generated').textContent='Erzeugt '+new Date(data.generatedAt).toLocaleString('de-DE',{timeZone:'Europe/Berlin'})+' · Europe/Berlin';
function ancestry(id, output) {let node=byId.get(id); while(node) {output.add(node.id); node=byId.get(node.parentId);}}
function descendsFrom(id, parent) {let current=byId.get(id); while(current) {if(current.id===parent) return true; current=byId.get(current.parentId);} return false;}
function descendantCommits(id) {const ids=new Set(); const todo=[id]; while(todo.length) {const next=todo.pop(); for(const hash of byId.get(next)?.commitIds||[]) ids.add(hash); todo.push(...(children.get(next)||[]));} return [...ids].map(hash=>commits.get(hash)).filter(Boolean).sort((a,b)=>b.date.localeCompare(a.date)||b.id.localeCompare(a.id));}
function matching(node, term) {if(!term) return true; if((displayTitle(node)+' '+node.title+' '+node.description).toLocaleLowerCase('de-DE').includes(term)) return true; return (node.commitIds||[]).some(id => {const c=commits.get(id); return c && (c.subject+' '+c.title+' '+c.id).toLocaleLowerCase('de-DE').includes(term);});}
function selected() {
  const term=$('search').value.trim().toLocaleLowerCase('de-DE');
  const category=$('category').value, kind=$('kind').value, from=$('from').value, to=$('to').value;
  if(from && to && from>to) return {ids:[],matches:0,invalidRange:true};
  const included=new Set(['genesis']), matches=[];
  if(view==='overview' && !term && !category && !kind && !from && !to) {
    for(const cat of Object.keys(categories)) {
      const latest=data.nodes.filter(n=>n.category===cat && !n.maintenance && n.role==='event').sort((a,b)=>b.date.localeCompare(a.date))[0];
      if(latest) ancestry(latest.id,included);
    }
  } else {
    for(const node of data.nodes) {
      if(node.id==='genesis') continue;
      if(view==='features' && node.role!=='feature' && !term) continue;
      if(view==='overview' && node.role==='event' && !term) continue;
      if(node.maintenance && !$('maintenance').checked) continue;
      if(category && node.category!==category) continue;
      if(kind && node.type!==kind) continue;
      if(from && (node.spanEnd||node.date)<from) continue;
      if(to && node.date>to) continue;
      if(!matching(node,term)) continue;
      matches.push(node.id); ancestry(node.id,included);
    }
  }
  // Hidden children do not reappear through a search until their branch is expanded.
  if(focusRoot && focusRoot!=='genesis') {
    const ancestors=new Set(); ancestry(focusRoot,ancestors);
    for(const id of [...included]) if(!ancestors.has(id) && !descendsFrom(id,focusRoot)) included.delete(id);
    for(const id of ancestors) included.add(id);
  }
  for(const id of [...included]) {let parent=byId.get(id)?.parentId; while(parent) {if(collapse.has(parent)) {included.delete(id); break;} parent=byId.get(parent)?.parentId;}}
  return {ids:[...included].map(id=>byId.get(id)), matches:matches.length};
}
function graphLayout(nodes) {
  const perDay=view==='all'?12:4.2, first=day(data.coverage.first), lanes=[], positions=new Map();
  const sorted=[...nodes].sort((a,b)=>a.date.localeCompare(b.date)||((a.role==='root'?0:a.role==='feature'?1:2)-(b.role==='root'?0:b.role==='feature'?1:2))||a.id.localeCompare(b.id));
  for(const node of sorted) {
    const width=node.role==='event'?174:node.role==='root'?240:218;
    const x=32+(day(node.date)-first)*perDay;
    let lane=lanes.findIndex(last=>last+14<x);
    if(lane<0) {lane=lanes.length; lanes.push(0);}
    lanes[lane]=x+width;
    positions.set(node.id,{node,x,y:45+lane*76,width,height:node.role==='event'?51:59});
  }
  const width=Math.max(460,...[...positions.values()].map(p=>p.x+p.width+24));
  const height=Math.max(180,45+lanes.length*76+24);
  return {positions,width,height,perDay};
}
function pathFor(parent,child) {
  let x1=parent.x+parent.width, y1=parent.y+parent.height/2, x2=child.x, y2=child.y+child.height/2;
  if(x2<=x1+16) {x1=parent.x+parent.width/2; y1=parent.y+parent.height; x2=child.x+child.width/2; y2=child.y; const delta=Math.max(25,Math.abs(y2-y1)/2); return `M${x1} ${y1} C${x1} ${y1+delta},${x2} ${y2-delta},${x2} ${y2}`;}
  const bend=Math.max(24,(x2-x1)/2); return `M${x1} ${y1} C${x1+bend} ${y1},${x2-bend} ${y2},${x2} ${y2}`;
}
function transform() {const stage=$('stage'); stage.style.transform=`translate(${offsetX}px,${offsetY}px) scale(${scale})`; $('zoom-label').textContent=Math.round(scale*100)+' %';}
function save() {const q=new URLSearchParams(); q.set('view',view); for(const id of controls) {const value=id==='maintenance'?($(id).checked?'1':''):$(id).value; if(value) q.set(id,value);} if(collapse.size) q.set('collapsed',[...collapse].sort().join(',')); if(focusRoot) q.set('focus',focusRoot.replace(/^feature:/,'')); if(active) q.set('feature',active.replace(/^feature:/,'')); history.replaceState(null,'',location.pathname+'?'+q);}
function draw() {
  const result=selected(); layout=graphLayout(result.ids);
  const stage=$('stage'), axis=$('axis'), edges=$('edges'), nodeLayer=$('nodes');
  stage.style.width=layout.width+'px'; stage.style.height=layout.height+'px'; axis.style.width=layout.width+'px';
  axis.replaceChildren(); for(let year=Number(data.coverage.first.slice(0,4));year<=Number(data.coverage.last.slice(0,4));year++) for(let month=1;month<=12;month++) {const date=year+'-'+String(month).padStart(2,'0')+'-01'; if(date<data.coverage.first||date>data.coverage.last) continue; const tick=make('time','',new Intl.DateTimeFormat('de-DE',{month:'short',year:'numeric',timeZone:'UTC'}).format(new Date(date+'T00:00:00Z'))); tick.style.left=(32+(day(date)-day(data.coverage.first))*layout.perDay)+'px'; axis.append(tick);}
  edges.replaceChildren(); edges.setAttribute('width',String(layout.width)); edges.setAttribute('height',String(layout.height));
  for(const p of layout.positions.values()) {const parent=layout.positions.get(p.node.parentId); if(!parent) continue; const edge=document.createElementNS('http://www.w3.org/2000/svg','path'); edge.setAttribute('d',pathFor(parent,p)); edge.dataset.parent=parent.node.id; edge.dataset.child=p.node.id; edges.append(edge);}
  nodeLayer.replaceChildren();
  for(const p of layout.positions.values()) {
    const n=p.node, wrapper=make('div','node '+(n.role==='root'?'root':n.role==='event'?'event':'feature')+(result.matches && !matching(n,$('search').value.trim().toLocaleLowerCase('de-DE'))?' context':''));
    wrapper.dataset.id=n.id; wrapper.dataset.category=n.category;
    Object.assign(wrapper.style,{left:p.x+'px',top:p.y+'px',width:p.width+'px',height:p.height+'px'});
    const open=button('',()=>openDetail(n.id),'node-open'); open.setAttribute('aria-label',displayTitle(n)+', '+fmt(n.date)+': Details öffnen'); open.append(make('span','label',displayTitle(n)),make('small','',fmt(n.date)+(n.maintenance?' · Pflege':''))); wrapper.append(open);
    if(n.role==='feature' && (children.get(n.id)||[]).length) {const toggle=button(collapse.has(n.id)?'+':'−',()=>{collapse.has(n.id)?collapse.delete(n.id):collapse.add(n.id); draw(); save();},'node-toggle'); toggle.setAttribute('aria-label',(collapse.has(n.id)?'Zweig öffnen: ':'Zweig schließen: ')+n.title); wrapper.append(toggle);}
    nodeLayer.append(wrapper);
  }
  $('status').textContent=result.invalidRange?'Der Zeitraum ist ungültig: „Von“ liegt nach „Bis“.':number(result.ids.length)+' Knoten sichtbar · '+number(data.stats.unassigned)+' Zuordnungen offen';
  for(const b of document.querySelectorAll('[data-view]')) b.setAttribute('aria-pressed',String(b.dataset.view===view));
  transform();
}
function fit() {if(!layout) return; const box=$('graph'); scale=Math.max(.08,Math.min(1.25,Math.min((box.clientWidth-36)/layout.width,(box.clientHeight-36)/layout.height))); offsetX=18; offsetY=18; transform();}
function zoom(factor,x=$('graph').clientWidth/2,y=$('graph').clientHeight/2) {const next=Math.max(.08,Math.min(2,scale*factor)); offsetX=x-(x-offsetX)*next/scale; offsetY=y-(y-offsetY)*next/scale; scale=next; transform();}
function reveal(id) {const p=layout?.positions.get(id); if(!p) return; const box=$('graph'); offsetX=box.clientWidth/2-(p.x+p.width/2)*scale; offsetY=box.clientHeight/2-(p.y+p.height/2)*scale; transform();}
function openDetail(id) {
  const n=byId.get(id); if(!n) return; active=id; page=0; focusReturn=document.activeElement;
  $('detail-title').textContent=displayTitle(n); $('detail-category').textContent=categories[n.category]+' · '+fmt(n.date);
  renderDetail(); $('detail').showModal(); document.body.style.overflow='hidden'; $('detail-close').focus(); save();
}
function renderDetail() {
  const n=byId.get(active), body=$('detail-body'); body.replaceChildren(); body.append(make('p','',n.description));
  const meta=make('div','meta'); for(const text of [n.role==='root'?'Ursprung':n.role==='feature'?'Funktionsfamilie':'Änderung',n.type,n.repository||'',n.spanEnd&&n.spanEnd!==n.date?'Bis '+fmt(n.spanEnd):''].filter(Boolean)) meta.append(make('span','',text)); body.append(meta);
  if(n.parentId) {const parent=byId.get(n.parentId); const go=button('Elternast: '+displayTitle(parent),()=>{closeDetail(); reveal(parent.id); openDetail(parent.id);},'more'); body.append(go);}
  if(n.role==='feature') body.append(button('Diesen Zweig ansehen',()=>{focusRoot=n.id; view='all'; closeDetail(); draw(); fit(); reveal(n.id); save();},'more'));
  if(n.prUrl && safePr(n.prUrl)) {const paragraph=make('p'); paragraph.append(link('Zugehörigen PR ansehen ↗',n.prUrl)); body.append(paragraph);}
  const list=descendantCommits(n.id); body.append(make('h3','',number(list.length)+' zugeordnete Commits'));
  const ul=make('ol','commit-list'); for(const c of list.slice(0,(page+1)*30)) {const li=make('li'); li.append(make('strong','',c.title||c.subject)); if(c.subject!==c.title) li.append(make('small','',c.subject)); li.append(make('small','',fmt(c.date)+' · '+c.repository)); if(safeCommit(c.id) && ['EarlySalty/Deadlock-Bots','EarlySalty/Deadlock-Twitch-Bot'].includes(c.repository)) {li.append(link(c.id.slice(0,8)+' · Code-Diff ↗','https://github.com/'+c.repository+'/commit/'+c.id));} ul.append(li);} body.append(ul);
  if(list.length>(page+1)*30) body.append(button('Weitere 30 Änderungen anzeigen',()=>{page++; renderDetail();},'more'));
}
function closeDetail() {if(!$('detail').open) return; document.body.style.overflow=''; $('detail').close(); if(focusReturn?.isConnected) focusReturn.focus();}
$('detail-close').addEventListener('click',closeDetail);
$('detail').addEventListener('cancel',event=>{event.preventDefault(); closeDetail();});
$('detail').addEventListener('close',()=>{document.body.style.overflow=''; if(focusReturn?.isConnected) focusReturn.focus();});
for(const b of document.querySelectorAll('[data-view]')) b.addEventListener('click',()=>{view=b.dataset.view; draw(); fit(); save();});
for(const id of controls) $(id).addEventListener(id==='search'?'input':'change',()=>{if(id==='search'&&$(id).value.trim()) view='all'; draw(); fit(); save();});
$('reset').addEventListener('click',()=>{for(const id of controls) {if(id==='maintenance') $(id).checked=false; else $(id).value='';} collapse.clear(); view='overview'; active=null; focusRoot=null; draw(); fit(); save();});
$('fit').addEventListener('click',fit); $('origin').addEventListener('click',()=>reveal('genesis')); $('zoom-in').addEventListener('click',()=>zoom(1.25)); $('zoom-out').addEventListener('click',()=>zoom(.8));
$('graph').addEventListener('wheel',event=>{event.preventDefault(); const rect=$('graph').getBoundingClientRect(); zoom(event.deltaY<0?1.12:.89,event.clientX-rect.left,event.clientY-rect.top);},{passive:false});
$('graph').addEventListener('pointerdown',event=>{if(event.target.closest('button')) return; drag={x:event.clientX,y:event.clientY,left:offsetX,top:offsetY}; $('graph').setPointerCapture(event.pointerId); $('graph').classList.add('dragging');});
$('graph').addEventListener('pointermove',event=>{if(!drag) return; offsetX=drag.left+event.clientX-drag.x; offsetY=drag.top+event.clientY-drag.y; transform();});
for(const type of ['pointerup','pointercancel']) $('graph').addEventListener(type,()=>{drag=null; $('graph').classList.remove('dragging');});
$('graph').addEventListener('keydown',event=>{const delta=event.shiftKey?100:35; if(event.key==='ArrowLeft') offsetX+=delta; else if(event.key==='ArrowRight') offsetX-=delta; else if(event.key==='ArrowUp') offsetY+=delta; else if(event.key==='ArrowDown') offsetY-=delta; else if(event.key==='+'||event.key==='=') zoom(1.2); else if(event.key==='-') zoom(.8); else return; event.preventDefault(); transform();});
window.addEventListener('resize',()=>transform());
draw();
if (matchMedia('(max-width:760px)').matches) {scale=.9; offsetX=12; offsetY=12; transform(); if(active) reveal(active);}
else {fit(); if(active) reveal(active);}
if(active) openDetail(active);
