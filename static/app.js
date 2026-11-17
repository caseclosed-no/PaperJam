const state={catalog:null,inventory:null,game:null,document:null,plan:null,runs:[]};
const $=s=>document.querySelector(s);const $$=s=>[...document.querySelectorAll(s)];
async function api(path,options={}){const r=await fetch(path,{headers:{"Content-Type":"application/json"},...options});const body=await r.json().catch(()=>({error:r.statusText}));if(!r.ok)throw new Error(body.error||r.statusText);return body}
function toast(message,error=false){const el=$("#toast");el.textContent=message;el.className=error?"show error":"show";clearTimeout(toast.timer);toast.timer=setTimeout(()=>el.className="",3200)}
function profileName(id){return state.catalog.profiles[id]?.name||id}function stock(game,id){return state.inventory.counts[game]?.[id]||0}
function profileOptions(selected=""){return `<option value="">Use page settings</option>`+Object.entries(state.catalog.profiles).map(([id,p])=>`<option value="${esc(id)}"${id===selected?' selected':''}>${esc(p.name)}</option>`).join("")}
function replacementValues(root){return Object.fromEntries($$(`${root} [data-replace-profile]`).filter(select=>select.value).map(select=>[select.dataset.replaceProfile,select.value]))}
function renderReplacements(root,profiles,replacements,wholeProfile,onchange){
 $(root).innerHTML=`<div class="replacement-heading"><strong>Replace individual profiles</strong><button type="button" class="mini" data-reset-replacements${!Object.keys(replacements).length?' disabled':''}>Reset</button></div><p class="settings-note">${wholeProfile?'The whole-run override takes priority.':'Choose a replacement for this run, including its paper and borders.'}</p>`+[...new Set(profiles)].map(id=>`<label><span>${esc(profileName(id))}</span><select data-replace-profile="${esc(id)}"${wholeProfile?' disabled':''}><option value="">Keep original</option>${Object.entries(state.catalog.profiles).filter(([target])=>target!==id).map(([target,p])=>`<option value="${esc(target)}"${replacements[id]===target?' selected':''}>${esc(p.name)}</option>`).join("")}</select></label>`).join("");
 $$(`${root} [data-replace-profile]`).forEach(select=>select.onchange=onchange);
 $(`${root} [data-reset-replacements]`).onclick=()=>{$$(`${root} [data-replace-profile]`).forEach(select=>select.value="");onchange()};
}
function renderProductionReplacements(){
 renderReplacements('#production-replacements',state.document.elements.map(e=>e.profile||state.document.default_profile),state.productionReplacements||{},$('#production-profile').value,()=>{state.productionReplacements=replacementValues('#production-replacements');renderProductionReplacements()});
 $$('#element-table [data-original-profile]').forEach(tag=>{const original=tag.dataset.originalProfile;const effective=$('#production-profile').value||state.productionReplacements?.[original]||original;tag.textContent=profileName(effective);tag.title=effective===original?'':`Replaces ${profileName(original)} for this run`});
}
function renderPreviewReplacements(plan){
 const game=state.catalog.games.find(g=>g.id===plan.game_id);const doc=game.documents.find(d=>d.id===plan.document_id);
 const selected=new Set(plan.requirements.map(r=>r.element_id));
 renderReplacements('#preview-replacements',doc.elements.filter(e=>selected.has(e.id)).map(e=>e.profile||doc.default_profile),plan.profile_replacements||{},plan.profile_override,updatePreviewSettings);
}
function jobPages(job){if(!job.source_pages?.length)return `${job.page_start}${job.page_end!==job.page_start?`–${job.page_end}`:""}`;const counts=new Map();for(const page of job.source_pages)counts.set(page,(counts.get(page)||0)+1);return [...counts].map(([page,count])=>`${page}${count>1?` × ${count}`:""}`).join(", ")}
function effectiveProfile(el){return el.profile||state.document.default_profile}
async function boot(){try{[state.catalog,state.inventory]=await Promise.all([api("/api/catalog"),api("/api/inventory")]);renderWarnings();$("#production-profile").innerHTML=profileOptions();fillGames();renderInventory();await refreshRuns();checkPrinter();setInterval(refreshRuns,1800)}catch(e){toast(e.message,true)}}
function renderWarnings(){const root=$("#warnings");root.innerHTML=state.catalog.warnings.map(w=>`<div class="warning">${esc(w)}</div>`).join("")}
function fillGames(){const select=$("#game-select");select.innerHTML=state.catalog.games.map(g=>`<option value="${g.id}">${esc(g.name)}</option>`).join("");state.game=state.catalog.games[0];fillDocuments()}
function fillDocuments(){const select=$("#document-select");select.innerHTML=state.game.documents.map(d=>`<option value="${d.id}">${esc(d.name)}${d.copies_per_page>1?` · ${d.copies_per_page}-up`:""}</option>`).join("");state.document=state.game.documents[0];state.productionReplacements={};renderElements()}
function renderElements(){const episodes=new Map(state.game.episodes.map(e=>[e.id,e]));const groups=new Map();for(const el of state.document.elements){if(!groups.has(el.episode))groups.set(el.episode,[]);groups.get(el.episode).push(el)}
 $("#element-table").innerHTML=[...groups].map(([episode,elements])=>`<section class="episode-block"><div class="episode-head"><label><input class="episode-check" data-episode="${episode}" type="checkbox" checked>${esc(episodes.get(episode)?.name||episode)}</label><span class="result-actions"><span>${elements.length} element${elements.length===1?"":"s"}</span><button class="mini" data-print-episode="${esc(episode)}" title="Preview this whole episode in the selected PDF stack using the target sets and inventory settings">Print whole episode</button></span></div><table class="element-table"><thead><tr><th></th><th>Page / element</th><th>In stock</th><th>Profile</th><th>Quick print</th></tr></thead><tbody>${elements.map(el=>`<tr><td><input class="element-check" data-id="${el.id}" data-episode="${episode}" type="checkbox" checked></td><td><div class="element-name">${esc(el.name)}</div><div class="element-id">Page ${el.page} · ${esc(el.id)}</div></td><td class="stock">${stock(state.game.id,el.id)}</td><td><span class="profile-tag" data-original-profile="${esc(effectiveProfile(el))}">${esc(profileName(effectiveProfile(el)))}</span></td><td><span class="result-actions"><input class="qty" data-qty="${el.id}" type="number" min="1" max="500" value="1"><button class="mini" data-print="${el.id}">Print</button></span></td></tr>`).join("")}</tbody></table></section>`).join("");bindElementEvents();updateSelection();renderProductionReplacements()}
function bindElementEvents(){$$('[data-print-episode]').forEach(b=>b.onclick=()=>productionPlan(state.document.elements.filter(el=>el.episode===b.dataset.printEpisode).map(el=>el.id)));$$(".element-check").forEach(c=>c.onchange=updateSelection);$$(".episode-check").forEach(c=>c.onchange=()=>{$$(`.element-check[data-episode="${c.dataset.episode}"]`).forEach(x=>x.checked=c.checked);updateSelection()});$$('[data-print]').forEach(b=>b.onclick=()=>manualPlan(state.game.id,state.document.id,b.dataset.print,Number($(`[data-qty="${CSS.escape(b.dataset.print)}"]`).value)))}
function updateSelection(){const all=$$(".element-check"),checked=all.filter(x=>x.checked);$("#selection-count").textContent=`${checked.length} of ${all.length} selected`;$("#select-all").checked=checked.length===all.length;$("#select-all").indeterminate=checked.length>0&&checked.length<all.length;$$('.episode-check').forEach(e=>{const children=$$(`.element-check[data-episode="${e.dataset.episode}"]`);e.checked=children.every(x=>x.checked);e.indeterminate=!e.checked&&children.some(x=>x.checked)})}
function groupingValues(mode,perSet){return{group_by_profile:mode!=="none",group_by_episode:mode==="episode_profile",group_per_set:mode!=="none"&&perSet}}
function groupingMode(plan){return !plan.group_by_profile?"none":plan.group_by_episode?"episode_profile":"profile"}
function syncPerSet(mode,box,hasSets=true){box.disabled=mode.value==="none"||!hasSets;if(box.disabled)box.checked=false}
async function productionPlan(selectedElementIds=$$(".element-check:checked").map(x=>x.dataset.id)){try{const payload={game_id:state.game.id,document_id:state.document.id,target_games:Number($("#target-games").value),selected_element_ids:selectedElementIds,inventory_mode:$("input[name=stock-mode]:checked").value,...groupingValues($("#group-mode").value,$("#group-per-set").checked),profile_override:$("#production-profile").value||null,profile_replacements:state.productionReplacements||{}};await requestPlan("production",payload)}catch(e){toast(e.message,true)}}
async function manualPlan(gameId,documentId,elementId,quantity){await requestPlan("manual",{game_id:gameId,document_id:documentId,selections:[{element_id:elementId,quantity}],profile_override:$("#production-profile").value||null,profile_replacements:state.productionReplacements||{},group_by_profile:false})}
async function requestPlan(kind,payload){state.planRequest={kind,payload};const version=(state.planVersion||0)+1;state.planVersion=version;$("#start-run").disabled=true;try{const plan=await api(`/api/plans/${kind}`,{method:"POST",body:JSON.stringify(payload)});if(version!==state.planVersion)return;showPlan(plan);$("#start-run").disabled=false}catch(e){if(version===state.planVersion)toast(e.message,true)}}
function updatePreviewSettings(){if(!state.planRequest)return;const {kind,payload}=state.planRequest;requestPlan(kind,{...payload,profile_override:$("#preview-profile").value||null,...groupingValues($("#preview-group-mode").value,$("#preview-group-per-set").checked),profile_replacements:replacementValues("#preview-replacements")})}
function showPlan(plan){state.plan=plan;renderPreviewReplacements(plan);$("#preview-profile").innerHTML=profileOptions(plan.profile_override||"");$("#preview-group-mode").value=groupingMode(plan);$("#preview-group-per-set").checked=!!plan.group_per_set;syncPerSet($("#preview-group-mode"),$("#preview-group-per-set"),plan.sets.length>0);$("#preview-title").textContent=`${plan.game_name} / ${plan.document_name}`;const total=plan.requirements.reduce((n,r)=>n+r.to_produce,0),stockUsed=plan.requirements.reduce((n,r)=>n+r.consume_from_stock,0),surplus=plan.requirements.reduce((n,r)=>n+r.surplus,0);let html=`<div class="summary-cards"><div class="summary-card"><small>Elements to produce</small><strong>${total}</strong></div><div class="summary-card"><small>From stock</small><strong>${stockUsed}</strong></div><div class="summary-card"><small>Printer jobs</small><strong>${plan.jobs.length}</strong></div><div class="summary-card"><small>Unavoidable surplus</small><strong>${surplus}</strong></div></div><h3 class="preview-section">Requirement</h3><table class="preview-table"><thead><tr><th>Element</th><th>Have</th><th>Need</th><th>Produce</th><th>Sheets</th></tr></thead><tbody>${plan.requirements.map(r=>`<tr><td>p.${r.page} ${esc(r.element_name)}</td><td>${r.have}</td><td>${r.need}</td><td>${r.to_produce}${r.surplus?` <small>(+${r.surplus} surplus)</small>`:""}</td><td>${r.sheets}</td></tr>`).join("")}</tbody></table>`;
 const scope=plan.group_by_episode?" per episode":"";
 if(plan.group_by_profile&&plan.group_per_set)html+=`<p class="settings-note">Grouped by print profile${scope}, separately for each target set${plan.group_by_episode?" (episode by episode, then set by set)":""}. The output comes out in set order; no sorting needed.</p>`;
 else if(plan.group_by_profile)html+=`<p class="settings-note">Grouped by print profile${scope} across the entire run${plan.target_games?` (${plan.target_games} target sets)`:""}. Each job includes all required copies for its profile.</p>`;
 if(plan.jobs.length>50)html+=`<p class="settings-note"><strong>${plan.jobs.length} printer jobs.</strong> Many small jobs mean many profile changes and a slower run.</p>`;
 if(plan.sets.length)html+=`<h3 class="preview-section">${plan.group_by_profile&&!plan.group_per_set?"Packing guide — grouped output needs sorting":"Ordered sets"}</h3><div class="set-list">${plan.sets.map(s=>`<div class="set-line"><strong>Set ${s.number}</strong> · ${s.elements.map(e=>e.source==="inventory"?`<span class="from-stock">${esc(e.element_name)} from stock</span>`:`p.${e.page}`).join(" · ")}</div>`).join("")}</div>`;
 html+=`<h3 class="preview-section">Printer jobs and profile changes</h3><div class="set-list">${plan.jobs.map((j,i)=>`<div class="set-line"><strong>${i+1}.</strong> pages ${jobPages(j)} · ${esc(profileName(j.profile_id))}${j.source_pages?.length?` · ${j.source_pages.length} sheets in this job`:""}${j.copies>1?` · ${j.copies} copies`:""}</div>`).join("")}</div>`;$("#preview-content").innerHTML=html;if(!$("#preview-dialog").open)$("#preview-dialog").showModal()}
async function startRun(){try{const run=await api("/api/runs",{method:"POST",body:JSON.stringify({plan_id:state.plan.id})});$("#preview-dialog").close();toast("Print run queued");await Promise.all([refreshRuns(),reloadInventory()])}catch(e){toast(e.message,true)}}
function inventoryEdits(){return state.inventoryEdits??={}}
function inventoryCount(game,id){const edits=inventoryEdits()[game];return edits&&Object.hasOwn(edits,id)?(edits[id]??""):stock(game,id)}
function updateInventoryStatus(){const count=Object.values(inventoryEdits()).reduce((n,items)=>n+Object.keys(items).length,0);$("#inventory-status").textContent=count?`${count} unsaved change${count===1?"":"s"} across all projects`:"All changes saved";$("#save-inventory").disabled=!!state.inventorySaving||!count}
function renderInventory(){
 const games=state.catalog.games;
 const game=games.find(g=>g.id===state.inventoryGame)||games[0];
 if(!game)return;
 state.inventoryGame=game.id;
 $("#inventory-game").innerHTML=games.map(g=>`<option value="${esc(g.id)}"${g.id===game.id?' selected':''}>${esc(g.name)}</option>`).join("");
 if(!game.documents.some(d=>d.id===state.inventoryDocument))state.inventoryDocument="";
 $("#inventory-document").innerHTML='<option value="">All paper stacks</option>'+game.documents.map(d=>`<option value="${esc(d.id)}"${d.id===state.inventoryDocument?' selected':''}>${esc(d.name)}</option>`).join("");
 const rows=game.documents.filter(d=>!state.inventoryDocument||d.id===state.inventoryDocument).flatMap(d=>d.elements.map(e=>({d,e})));
 $("#inventory-content").innerHTML=game.episodes.map(episode=>{
  const elements=rows.filter(({e})=>e.episode===episode.id);
  if(!elements.length)return "";
  return `<section class="episode-block"><div class="episode-head"><span>${esc(episode.name)}</span><span>${elements.length} element${elements.length===1?"":"s"}</span></div><table class="element-table inventory-table"><thead><tr><th>Page / element</th><th>Paper stack</th><th>In stock</th></tr></thead><tbody>${elements.map(({d,e})=>`<tr><td><div class="element-name">${esc(e.name)}</div><div class="element-id">Page ${e.page} · ${esc(e.id)}</div></td><td>${esc(d.name)}${d.copies_per_page>1?` · ${d.copies_per_page}-up`:""}</td><td><input class="inventory-count" data-inventory-element="${esc(e.id)}" aria-label="In stock: ${esc(e.name)}" type="number" min="0" max="99999" step="1" value="${inventoryCount(game.id,e.id)}"></td></tr>`).join("")}</tbody></table></section>`;
 }).join("")||'<p class="inventory-empty">No elements in this paper stack.</p>';
 $$('[data-inventory-element]').forEach(input=>input.oninput=()=>{
  const edits=inventoryEdits();const id=input.dataset.inventoryElement;const value=input.valueAsNumber;
  (edits[game.id]??={})[id]=Number.isFinite(value)?value:null;
  if(value===stock(game.id,id)&&!state.inventorySaving)delete edits[game.id][id];
  updateInventoryStatus();
 });
 updateInventoryStatus();
}
async function saveInventory(){
 const edits=inventoryEdits();
 if(Object.values(edits).some(items=>Object.values(items).some(value=>!Number.isInteger(value)||value<0||value>99999))){toast("Enter whole stock counts between 0 and 99999, including changes in other projects.",true);return}
 const counts=structuredClone(state.inventory.counts);
 for(const [game,items] of Object.entries(edits))Object.assign(counts[game]??={},items);
 state.inventorySaving=true;$("#save-inventory").disabled=true;
 try{
  state.inventory=await api("/api/inventory",{method:"PUT",body:JSON.stringify({expected_revision:state.inventory.revision,counts})});
  // Keep any newer edits made while the save request was in flight.
  for(const [game,items] of Object.entries(edits))for(const [id,value] of Object.entries(items))if(value===counts[game][id])delete items[id];
  toast("Inventory saved");renderInventory();renderElements();
 }catch(e){toast(e.message,true);try{await reloadInventory()}catch{}}finally{state.inventorySaving=false;updateInventoryStatus()}
}
async function reloadInventory(){state.inventory=await api("/api/inventory");renderInventory();if(state.document)renderElements()}
async function refreshRuns(){try{state.runs=await api("/api/runs");renderRuns()}catch(e){toast(e.message,true)}}
function renderRuns(){const active=state.runs.filter(r=>["queued","running","cancel_requested"].includes(r.status));$("#active-count").hidden=!active.length;$("#active-count").textContent=active.length;$("#runs-content").innerHTML=state.runs.map(r=>{const done=r.jobs.filter(j=>j.status==="completed").length,pct=r.jobs.length?Math.round(done/r.jobs.length*100):0;return `<article class="panel run-card"><div class="run-head"><div><h3>${esc(r.game_name)} / ${esc(r.document_name)}</h3><p>${new Date(r.created_at).toLocaleString()} · ${done} of ${r.jobs.length} jobs complete</p></div><span class="run-status ${r.status}">${r.status.replaceAll('_',' ')}</span></div><div class="progress"><span style="width:${pct}%"></span></div>${r.current_job!=null&&r.jobs[r.current_job]?`<p>Current: ${esc(r.jobs[r.current_job].job.title)}</p>`:""}${r.error?`<p class="run-error">${esc(r.error)}</p>`:""}<div class="run-actions">${["queued","running"].includes(r.status)?`<button class="secondary" data-cancel="${r.id}">Cancel</button>`:""}${r.status==="running"&&r.jobs[r.current_job]?.status==="submitted"&&r.jobs[r.current_job]?.cups_request_id?`<button class="secondary" data-recover="${r.id}">Clear stall and retry</button>`:""}${Object.keys(r.consumed_inventory).length&&!r.inventory_restored?`<button class="secondary" data-restore="${r.id}">Restore consumed stock</button>`:""}</div></article>`}).join("")||'<div class="panel run-card">No print runs yet.</div>';$$('[data-cancel]').forEach(b=>b.onclick=()=>runAction(b.dataset.cancel,'cancel'));$$('[data-recover]').forEach(b=>b.onclick=()=>recoverRun(b.dataset.recover));$$('[data-restore]').forEach(b=>b.onclick=()=>runAction(b.dataset.restore,'restore-stock'))}
async function runAction(id,action){try{await api(`/api/runs/${id}/${action}`,{method:"POST",body:"{}"});toast(action==="cancel"?"Cancellation requested":"Inventory restored");await Promise.all([refreshRuns(),reloadInventory()])}catch(e){toast(e.message,true)}}
async function checkPrinter(){try{const p=await api("/api/printer/status");$("#printer-pill").textContent=`${p.destination} · ${p.backend}`;$("#printer-pill").title=p.status}catch(e){$("#printer-pill").textContent="Printer unavailable"}}
function showView(name){if(!["production","inventory","runs"].includes(name))name="production";$$('.view').forEach(v=>v.classList.toggle('active',v.id===name));$$('[data-view]').forEach(b=>b.classList.toggle('nav-active',b.dataset.view===name));location.hash=name;if(name==='runs')refreshRuns()}
function esc(v){return String(v).replace(/[&<>"]/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;'}[c]))}
$("#game-select").onchange=e=>{state.game=state.catalog.games.find(g=>g.id===e.target.value);fillDocuments()};$("#document-select").onchange=e=>{state.document=state.game.documents.find(d=>d.id===e.target.value);state.productionReplacements={};renderElements()};$("#select-all").onchange=e=>{$$('.element-check,.episode-check').forEach(x=>x.checked=e.target.checked);updateSelection()};$("#preview-production").onclick=()=>productionPlan();$("#start-run").onclick=startRun;$("#save-inventory").onclick=saveInventory;$("#refresh-runs").onclick=refreshRuns;$$('[data-view]').forEach(b=>b.onclick=()=>showView(b.dataset.view));$$('[data-close]').forEach(b=>b.onclick=()=>$("#preview-dialog").close());showView(location.hash.slice(1)||"production");boot();

$("#preview-profile").onchange=updatePreviewSettings;$("#preview-group-mode").onchange=()=>{syncPerSet($("#preview-group-mode"),$("#preview-group-per-set"),!!state.plan?.sets.length);updatePreviewSettings()};$("#preview-group-per-set").onchange=updatePreviewSettings;$("#group-mode").onchange=()=>syncPerSet($("#group-mode"),$("#group-per-set"));

$("#inventory-game").onchange=e=>{state.inventoryGame=e.target.value;state.inventoryDocument="";renderInventory()};
$("#inventory-document").onchange=e=>{state.inventoryDocument=e.target.value;renderInventory()};

$("#production-profile").onchange=renderProductionReplacements;

async function recoverRun(id){
 if(state.recoveringRun)return;
 const run=state.runs.find(r=>r.id===id),requestId=run?.jobs[run.current_job]?.cups_request_id;
 if(!requestId){toast("Refresh the run before retrying",true);return;}
 if(!confirm("Cancel the current printer job and retry its entire batch? Pages already printed from this batch may print again. Completed batches will be kept and the remaining work will continue."))return;
 state.recoveringRun=id;
 const button=$$('[data-recover]').find(b=>b.dataset.recover===id);
 if(button)button.disabled=true;
 try{await api(`/api/runs/${id}/recover`,{method:"POST",body:JSON.stringify({cups_request_id:requestId})});toast("Stall recovery requested");await refreshRuns()}
 catch(e){toast(e.message,true)}
 finally{state.recoveringRun=null;if(button)button.disabled=false}
}
