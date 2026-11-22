#!/usr/bin/env python3
"""Génère un tableau de bord HTML autonome à partir du CSV de benchmark_peer_report."""
import argparse
import csv
import json
import math
from pathlib import Path


IDENTITY = ('peer', 'arch', 'id', 'rank', 'family', 'source', 'output', 'status', 'reason', 'same_cpu_runs')
METRICS = ('cpu', 'p50', 'p95', 'memory', 'bytes')
MODES = ('default', 'native')
VALUES = tuple('peer_' + metric for metric in METRICS) + tuple(
    f'{mode}_{metric}{suffix}' for mode in MODES for metric in METRICS
    for suffix in ('', '_ratio', '_min_ratio')
)
STATUSES = {'measured', 'quality', 'error', 'unmapped'}


def dataset(path):
    rows = []
    with path.open(encoding='utf-8-sig', newline='') as stream:
        reader = csv.DictReader(stream)
        missing = set(IDENTITY) - set(reader.fieldnames or ())
        if missing:
            raise ValueError('Colonnes manquantes : ' + ', '.join(sorted(missing)))
        for row in reader:
            if row['status'] not in STATUSES:
                raise ValueError(f"État inconnu à la ligne {reader.line_num}")
            identity = [int(row[k]) if k in ('rank', 'same_cpu_runs') else row[k] for k in IDENTITY]
            if row['status'] == 'measured':
                try:
                    values = [float(row[key]) for key in VALUES]
                except (KeyError, ValueError, TypeError) as error:
                    raise ValueError(f"Mesure manquante à la ligne {reader.line_num}") from error
                if not all(math.isfinite(n) and n > 0 for n in values):
                    raise ValueError(f"Mesure invalide à la ligne {reader.line_num}")
                identity.extend(values)
            rows.append(identity)
    return {'columns': IDENTITY + VALUES, 'rows': rows}


HTML = r"""<!doctype html>
<html lang="fr"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>Comparaison des serveurs d’images · Pixstride</title>
<style>
:root{color-scheme:light;--ink:#172c36;--muted:#475f68;--line:#cbd7d6;--wash:#eff5f3;--accent:#075e52;--slow:#97390d}
*{box-sizing:border-box}body{margin:0;color:var(--ink);background:#fbfcfa;font:15px/1.45 system-ui,sans-serif}
main{max-width:1440px;margin:auto;padding:clamp(16px,3vw,44px)}h1,h2,h3,p{margin:0}
h1{font-size:clamp(1.6rem,4vw,2.6rem);letter-spacing:-.04em}h2{font-size:1.3rem;margin:1.8rem 0 .5rem}
p{max-width:92ch;color:var(--muted)}.filters{display:flex;gap:12px;flex-wrap:wrap;margin:1.25rem 0}
label{display:flex;flex-direction:column;gap:3px;font-size:.86rem;font-weight:600}
select,input,button{font:inherit;color:var(--ink);background:white;border:1px solid #8ca49f;border-radius:5px;padding:6px 9px}
input[type=search]{width:210px}button{cursor:pointer}button:disabled{opacity:.45;cursor:default}
:focus-visible{outline:3px solid #2178af;outline-offset:2px}.summary{display:grid;grid-template-columns:repeat(auto-fit,minmax(155px,1fr));background:var(--wash);border-block:1px solid var(--line);padding:14px 10px;gap:12px}
.metric{padding:0 8px}.metric strong{font-size:1.65rem;display:block;letter-spacing:-.03em}.metric small,td small{color:var(--muted);display:block;font-size:.79rem}
.metric:first-child strong,.slow{color:var(--slow)}.details{margin:.5rem 0}summary{cursor:pointer;color:var(--accent);font-weight:600}
.wrap{overflow:auto;border:1px solid var(--line);border-radius:5px;background:white;margin:.6rem 0}
table{width:100%;border-collapse:collapse;font-variant-numeric:tabular-nums;font-size:.88rem}
th,td{text-align:left;padding:8px 10px;border-bottom:1px solid #e2e8e6;vertical-align:top}thead{background:var(--wash)}th{font-weight:650}
td.num,th.num{text-align:right;white-space:nowrap}tbody tr:last-child td{border:0}.case{min-width:175px;overflow-wrap:anywhere}.reason{overflow-wrap:anywhere}
.tools,.pager{display:flex;align-items:center;gap:14px;flex-wrap:wrap}.tools .check{flex-direction:row;align-items:center;gap:5px}
.pager{justify-content:flex-end;font-size:.85rem}.note{font-size:.87rem;margin:.4rem 0}.families{max-width:780px}
progress{accent-color:var(--slow);max-width:100px;width:8vw;vertical-align:middle;margin-right:8px}
@media(max-width:600px){.filters>label{flex:1 1 45%}select,input[type=search]{width:100%}.pager{justify-content:flex-start}}
</style></head><body><main>
<h1>Pixstride face aux autres serveurs d’images</h1>
<p>Un cas est en retard lorsque Pixstride 0.5.0 utilise au moins 10 % de temps CPU de plus par requête et que sa latence médiane dépasse aussi celle du serveur comparé d’au moins 10 %. Chaque métrique utilise le meilleur résultat des trois exécutions historiques de Pixstride.</p>
<div class="filters">
<label>Serveur comparé<select id="peer"><option value="">Tous les serveurs</option></select></label>
<label>Architecture<select id="arch"><option value="">Toutes</option></select></label>
<label>Mode Pixstride<select id="mode"><option value="default">Par défaut</option><option value="native">Natif</option></select></label>
<label>Modèle de CPU<select id="cpu"><option value="">Tous</option><option value="same">Au moins une référence identique</option><option value="other">Aucune référence identique</option></select></label>
<label>Qualité et disponibilité<select id="status"><option value="">Tous les résultats</option>
<option value="measured">Qualité validée</option><option value="quality">Écart de pixels</option>
<option value="unmapped">Opération incompatible</option><option value="error">Erreur d’essai</option></select></label>
<label>Famille ou cas<input id="query" type="search" placeholder="Ex. blur, v2-0137"></label>
</div>
<section aria-label="Synthèse de la sélection"><div class="summary" id="summary"></div>
<p class="note">Une comparaison associe un cas, un serveur et une architecture. Le taux de retard porte sur les comparaisons de qualité validée.</p></section>
<section><h2>Retards par famille</h2><div class="families wrap" tabindex="0" role="region" aria-label="Retards par famille">
<table><thead><tr><th>Famille</th><th class="num">Comparaisons en retard / validées</th><th class="num">Part</th></tr></thead><tbody id="families"></tbody></table></div></section>
<section><h2>Mesures de qualité validée</h2>
<div class="tools"><label>Trier par<select id="sort"><option value="both">Retard commun au CPU et à la p50</option>
<option value="cpu">Écart minimal de CPU</option><option value="p50">Écart minimal de p50</option>
<option value="p95">Écart relatif de p95</option><option value="memory">Écart de mémoire en octets</option>
<option value="bytes">Écart de taille en octets</option><option value="rank">Rang dans le catalogue</option></select></label>
<label>Ordre<select id="direction"><option value="desc">Décroissant</option><option value="asc">Croissant</option></select></label>
<label>Afficher les cas<select id="scope"><option value="">Tous les cas validés</option>
<option value="slow">Retard CPU et p50 sur cette architecture</option>
<option value="both">Retard CPU et p50 sur les deux architectures</option></select></label></div>
<p class="note">CPU et p50 affichent l’écart minimal sur les trois références. Pour la p95, le pic mémoire du cgroup et la taille de réponse, l’écart utilise la médiane historique sur CPU identique quand elle est disponible, sinon celle des trois références. Sous chaque écart figurent la médiane historique de Pixstride ainsi sélectionnée, puis la mesure du serveur comparé.</p>
<div class="wrap" tabindex="0" role="region" aria-label="Mesures des cas"><table><thead><tr>
<th>Cas</th><th>Serveur</th><th class="num">CPU minimal</th><th class="num">p50 minimale</th><th class="num">p95</th><th class="num">Écart mémoire</th><th class="num">Écart réponse</th>
</tr></thead><tbody id="measured"></tbody></table></div><div class="pager" id="pages-measured"></div></section>
<section><h2>Écarts de qualité, incompatibilités et erreurs</h2>
<div class="wrap" tabindex="0" role="region" aria-label="Résultats hors classement"><table><thead><tr><th>Cas</th><th>Serveur</th><th>État</th><th>Motif relevé</th></tr></thead><tbody id="excluded"></tbody></table></div>
<div class="pager" id="pages-excluded"></div></section>
<noscript>Activez JavaScript dans votre navigateur pour explorer les mesures embarquées dans ce fichier.</noscript>
</main><script type="application/json" id="source">__DATA__</script><script>
'use strict';
const $ = id => document.getElementById(id);
const source = JSON.parse($('source').textContent);
const rows = source.rows.map(values => Object.fromEntries(values.map((v,i) => [source.columns[i],v])));
const states = {measured:'Qualité validée', quality:'Écart de pixels', unmapped:'Opération incompatible', error:'Erreur d’essai'};
const number = new Intl.NumberFormat('fr-FR',{maximumFractionDigits:2});
const oneDecimal = new Intl.NumberFormat('fr-FR',{maximumFractionDigits:1});
const serviceNames = {thumbor:'Thumbor',imaginary:'Imaginary',weserv:'weserv',ipx:'IPX',imageflow:'Imageflow'};
for (const id of ['arch','peer']) {
  const choices = [...new Set(rows.map(row=>row[id]))].sort();
  for(const value of choices) $(id).add(new Option(id==='peer' ? serviceNames[value] || value : value,value));
}
const limit = 40, pages = {measured:0,excluded:0};
function element(tag, value, className='') {
  const node = document.createElement(tag);
  if (value!==undefined) node.textContent=value;
  if(className) node.className=className;
  return node;
}
function percentage(a,b) {return b ? oneDecimal.format(100*a/b)+' %' : '—';}
function deltaRatio(ratio) {
  const n=(ratio-1)*100;
  return (n>0?'+':'')+oneDecimal.format(n)+' %';
}
function bytes(n) {
  let value=Math.abs(n), unit=0;
  const units=['o','Kio','Mio','Gio'];
  while(value>=1024 && unit<units.length-1){value/=1024;unit++;}
  return number.format(value)+' '+units[unit];
}
function byteDelta(n) {return (n>0?'+':n<0?'−':'')+bytes(n);}
function metricCard(label, a, b) {
  const card=element('div',undefined,'metric');
  card.append(element('div',label),element('strong',percentage(a,b)),element('small',number.format(a)+' / '+number.format(b)));
  return card;
}
function renderTable(id, data, cols, renderRow) {
  const body=$(id), page=pages[id], pageCount=Math.ceil(data.length/limit);
  const visible=data.slice(page*limit,(page+1)*limit);
  body.replaceChildren(...visible.map(renderRow));
  if(!visible.length) {
    const cell=element('td','Aucun résultat pour cette sélection.');cell.colSpan=cols;
    const tr=element('tr');tr.append(cell);body.append(tr);
  }
  const nav=$('pages-'+id);
  const message=visible.length ? (page*limit+1)+'–'+(page*limit+visible.length)+' sur '+number.format(data.length) : '0 résultat';
  const info=element('span',message);info.setAttribute('role','status');
  const previous=element('button','Précédent'),next=element('button','Suivant');
  previous.disabled=page===0;next.disabled=page+1>=pageCount;
  previous.onclick=()=>{pages[id]--;draw();};next.onclick=()=>{pages[id]++;draw();};
  nav.replaceChildren(info,previous,next);
}
function cell(main, sub, css='') {
  const td=element('td',main,css);
  if(sub!==undefined) td.append(element('small',sub));
  return td;
}
function caseCells(r) {
  return [cell(r.id,r.family+' · '+r.source+' → '+r.output,'case'),
    cell(serviceNames[r.peer]||r.peer,r.arch+' · '+r.same_cpu_runs+'/3 CPU identiques')];
}
function draw() {
  const mode=$('mode').value+'_', term=$('query').value.trim().toLowerCase();
  const selected=rows.filter(r=>(!$('peer').value || r.peer===$('peer').value) && (!$('arch').value || r.arch===$('arch').value)
    && (!$('status').value || r.status===$('status').value) && (!$('cpu').value || (r.same_cpu_runs>0)===($('cpu').value==='same'))
    && (!term || [r.id,r.family,r.source,r.output].some(v=>v.toLowerCase().includes(term))));
  const measured=selected.filter(r=>r.status==='measured'), excluded=selected.filter(r=>r.status!=='measured');
  const min=r=>Math.min(r[mode+'cpu_min_ratio'],r[mode+'p50_min_ratio']);
  const slow=r=>min(r)>=1.10, count=state=>selected.filter(r=>r.status===state).length;
  $('summary').replaceChildren(metricCard('Retards CPU et p50',measured.filter(slow).length,measured.length),
    metricCard('Qualité validée',measured.length,selected.length),metricCard('Écarts de pixels',count('quality'),selected.length),
    metricCard('Incompatibilités',count('unmapped'),selected.length),metricCard('Erreurs d’essai',count('error'),selected.length));
  const families=new Map();
  for(const r of measured){
    const tally=families.get(r.family)||{name:r.family,total:0,slow:0};
    tally.total++;if(slow(r)) tally.slow++;families.set(r.family,tally);
  }
  const groups=[...families.values()].sort((a,b)=>b.slow-a.slow || b.slow/b.total-a.slow/a.total || a.name.localeCompare(b.name));
  $('families').replaceChildren(...groups.map(g=>{
    const tr=element('tr'), share=cell(undefined,undefined,'num'), bar=element('progress');
    bar.max=g.total;bar.value=g.slow;bar.setAttribute('aria-hidden','true');
    share.append(bar,document.createTextNode(percentage(g.slow,g.total)));
    tr.append(cell(g.name),cell(g.slow+' / '+g.total,undefined,'num'),share);return tr;
  }));
  if(!groups.length){const tr=element('tr'),td=cell('Aucune mesure validée.');td.colSpan=3;tr.append(td);$('families').append(tr);}
  const sort=$('sort').value, direction=$('direction').value==='asc' ? 1 : -1;
  function sortValue(r) {
    if(sort==='both')return min(r);
    if(sort==='rank')return r.rank;
    if(sort==='memory'||sort==='bytes')return r[mode+sort]-r['peer_'+sort];
    return r[mode+sort+(sort==='p95'?'_ratio':'_min_ratio')];
  }
  const scope=$('scope').value;
  const key=r=>r.peer+'|'+r.id;
  const slowArchitectures=new Map();
  if(scope==='both') {
    for(const r of rows.filter(r=>r.status==='measured' && slow(r))) {
      const k=key(r);
      if(!slowArchitectures.has(k))slowArchitectures.set(k,new Set());
      slowArchitectures.get(k).add(r.arch);
    }
  }
  const ranked=measured.filter(r=>!scope || (scope==='slow' ? slow(r) : slowArchitectures.get(key(r))?.size===2));
  ranked.sort((a,b)=>direction*(sortValue(a)-sortValue(b)) || a.rank-b.rank || a.peer.localeCompare(b.peer) || a.arch.localeCompare(b.arch));
  renderTable('measured',ranked,7,r=>{
    const tr=element('tr');tr.append(...caseCells(r));
    for(const m of ['cpu','p50','p95','memory','bytes']){
      const pix=r[mode+m],peer=r['peer_'+m],size=(m==='memory'||m==='bytes');
      const ratio=r[mode+m+(m==='cpu'||m==='p50'?'_min_ratio':'_ratio')];
      const value=size ? byteDelta(pix-peer) : deltaRatio(ratio);
      const pair=size ? bytes(pix)+' / '+bytes(peer) : number.format(pix)+' / '+number.format(peer)+' ms';
      tr.append(cell(value,pair,'num'+(!size && ratio>=1.1?' slow':'')));
    }
    return tr;
  });
  renderTable('excluded',excluded,4,r=>{
    const tr=element('tr');tr.append(...caseCells(r),cell(states[r.status]),cell(r.reason||'—',undefined,'reason'));return tr;
  });
}
for(const id of ['peer','arch','mode','query','cpu','status','sort','direction','scope']){
  $(id).addEventListener(id==='query'?'input':'change',()=>{pages.measured=0;pages.excluded=0;draw();});
}
draw();
</script></body></html>"""


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('csv', type=Path)
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    payload = json.dumps(dataset(args.csv), ensure_ascii=False, separators=(',', ':'))
    # Escape HTML raw-text delimiters even if a server returns markup in its error.
    document = HTML.replace('__DATA__', payload.replace('<', r'\u003c'))
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(document, encoding='utf-8')


if __name__ == '__main__':
    main()
