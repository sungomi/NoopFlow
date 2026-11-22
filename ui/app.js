'use strict';

const T = window.__TAURI__;
const $ = (s, el = document) => el.querySelector(s);
const $$ = (s, el = document) => [...el.querySelectorAll(s)];
const esc = (s) => String(s ?? '').replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]);

// ------------------------------------------------------------------ icons
const ICON = {
  windows: '<svg viewBox="0 0 24 24"><rect x="3.5" y="3.5" width="7.3" height="7.3" rx="1.2"/><rect x="13.2" y="3.5" width="7.3" height="7.3" rx="1.2"/><rect x="3.5" y="13.2" width="7.3" height="7.3" rx="1.2"/><rect x="13.2" y="13.2" width="7.3" height="7.3" rx="1.2"/></svg>',
  mac: '<svg viewBox="0 0 24 24"><path d="M9 9V6.5A2.5 2.5 0 1 0 6.5 9H9Zm0 0h6m-6 0v6m6-6V6.5A2.5 2.5 0 1 1 17.5 9H15Zm0 0v6m0 0H9m6 0v2.5a2.5 2.5 0 1 0 2.5-2.5H15Zm-6 0v2.5A2.5 2.5 0 1 1 6.5 15H9Z"/></svg>',
  linux: '<svg viewBox="0 0 24 24"><rect x="3" y="4.5" width="18" height="12" rx="2"/><path d="M9 20h6M12 16.5V20"/></svg>',
  cursor: '<svg viewBox="0 0 24 24"><path d="M5 3.5 19 10l-6.2 1.8L10 18.5z"/></svg>',
  up: '<svg viewBox="0 0 24 24"><path d="M12 19V5M6 11l6-6 6 6"/></svg>',
  down: '<svg viewBox="0 0 24 24"><path d="M12 5v14M6 13l6 6 6-6"/></svg>',
  check: '<svg viewBox="0 0 24 24"><path d="M5 12.5 10 17l9-10"/></svg>',
  x: '<svg viewBox="0 0 24 24"><path d="M6 6l12 12M18 6 6 18"/></svg>',
  link: '<svg viewBox="0 0 24 24"><path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1"/><path d="M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/></svg>',
  shield: '<svg viewBox="0 0 24 24"><path d="M12 3 5 6v5.5c0 4.2 2.9 8 7 9.5 4.1-1.5 7-5.3 7-9.5V6z"/><path d="M9 12l2 2 4-4"/></svg>',
  plus: '<svg viewBox="0 0 24 24"><path d="M12 5v14M5 12h14"/></svg>',
  upload: '<svg viewBox="0 0 24 24"><path d="M12 15V4M7.5 8.5 12 4l4.5 4.5M5 15v3a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2v-3"/></svg>',
};
const OS_NAME = { windows: 'Windows', mac: 'Mac', linux: 'Linux' };

// --------------------------------------------------------------- backend
const MOCK = !T;
const invoke = (cmd, args) => (MOCK ? mockInvoke(cmd, args) : T.core.invoke(cmd, args));

let S = null;

function device(id) {
  if (!S) return null;
  if (S.me.id === id) return S.me;
  return S.devices.find((d) => d.id === id) || null;
}
const nameOf = (id) => device(id)?.name || '다른 기기';

// ----------------------------------------------------------------- toasts
function toast(text, level = 'info', action) {
  const el = document.createElement('div');
  el.className = `toast ${level}`;
  el.innerHTML = `<span class="t-dot"></span><span>${esc(text)}</span>`;
  if (action) {
    const b = document.createElement('button');
    b.textContent = action.label;
    b.onclick = () => { action.fn(); remove(); };
    el.appendChild(b);
  }
  $('#toasts').appendChild(el);
  const remove = () => { el.classList.add('leaving'); setTimeout(() => el.remove(), 250); };
  setTimeout(remove, action ? 6000 : 3500);
}

// ------------------------------------------------------------------- tabs
function showTab(name) {
  $$('.nav-item').forEach((b) => b.classList.toggle('active', b.dataset.tab === name));
  $$('.tab').forEach((t) => t.classList.toggle('active', t.id === `tab-${name}`));
  if (name === 'layout') requestAnimationFrame(renderStage);
}

// ------------------------------------------------------------------ stage
const TILE_W = 210;
const CW = 262; // cell pitch
const CH = 230;
let geo = { ox: 0, oy: 0, s: 1 };
let drag = null;

function layoutCells() {
  return S.layout.cells.filter((c) => device(c.id) && (c.id === S.me.id || device(c.id).trusted));
}

function renderStage() {
  if (!S || drag) return;
  const stage = $('#stage');
  const inner = $('#stageInner');
  const W = stage.clientWidth;
  const H = stage.clientHeight;
  if (!W || !H) return;

  const cells = layoutCells();
  const me = cells.find((c) => c.id === S.me.id) || { id: S.me.id, x: 0, y: 0 };
  const hasPeers = cells.length > 1;
  const addCell = hasPeers ? null : { x: me.x + 1, y: me.y };

  const all = addCell ? [...cells, addCell] : cells;
  const minX = Math.min(...all.map((c) => c.x));
  const maxX = Math.max(...all.map((c) => c.x));
  const minY = Math.min(...all.map((c) => c.y));
  const maxY = Math.max(...all.map((c) => c.y));
  const cols = maxX - minX + 1;
  const rows = maxY - minY + 1;
  const contentW = cols * CW;
  const contentH = rows * CH;
  const s = Math.min(1, (W - 40) / contentW, (H - 40) / contentH);
  geo = {
    s,
    ox: (W / s - contentW) / 2 - minX * CW + (CW - TILE_W) / 2,
    oy: (H / s - contentH) / 2 - minY * CH + 14,
  };
  inner.style.transform = `scale(${s})`;
  inner.style.transformOrigin = '0 0';
  inner.style.width = `${W / s}px`;
  inner.style.height = `${H / s}px`;

  const parts = [`<div class="slot" id="slot" style="width:${TILE_W + 20}px;height:${CH - 20}px"></div>`];
  for (const c of cells) {
    const d = device(c.id);
    const isMe = c.id === S.me.id;
    const online = isMe || d.online;
    const active = S.active === c.id && S.devices.some((x) => x.trusted && x.online);
    const scr = d.screen ? `${d.screen.w}×${d.screen.h}` : '';
    const sub = isMe
      ? `이 컴퓨터 · ${OS_NAME[d.os]}`
      : online
        ? `연결됨 · ${OS_NAME[d.os]}`
        : d.nearby ? '연결 중…' : '꺼져 있음';
    parts.push(`
      <div class="tile ${isMe ? 'me' : ''} ${online ? 'online' : 'offline'} ${active ? 'active' : ''}"
           data-id="${esc(c.id)}" style="left:${geo.ox + c.x * CW}px;top:${geo.oy + c.y * CH}px;width:${TILE_W}px">
        <div class="screen">
          <div class="screen-inner"><span class="os-icon">${ICON[d.os] || ICON.linux}</span><span class="screen-res">${scr}</span></div>
          <div class="drop-label">${ICON.upload}여기로 보내기</div>
          ${active ? `<div class="cursor-badge">${ICON.cursor}커서</div>` : ''}
        </div>
        <div class="stand"></div>
        <div class="tile-name">${esc(d.name)}</div>
        <div class="tile-sub"><span class="dot"></span>${esc(sub)}</div>
      </div>`);
  }
  if (addCell) {
    parts.push(`
      <button class="add-tile" id="addTile" style="left:${geo.ox + addCell.x * CW}px;top:${geo.oy + addCell.y * CH}px;width:${TILE_W}px;height:${(TILE_W * 10) / 16}px">
        ${ICON.plus}<span>다른 컴퓨터 연결</span>
      </button>`);
  }
  inner.innerHTML = parts.join('');
  $('#addTile')?.addEventListener('click', () => showTab('devices'));
  $$('.tile', inner).forEach((t) => t.addEventListener('pointerdown', onTileDown));
}

function cellAt(left, top) {
  return { x: Math.round((left - geo.ox) / CW), y: Math.round((top - geo.oy) / CH) };
}

function onTileDown(e) {
  if (e.button !== 0) return;
  const tile = e.currentTarget;
  drag = {
    tile,
    id: tile.dataset.id,
    startX: e.clientX,
    startY: e.clientY,
    left: parseFloat(tile.style.left),
    top: parseFloat(tile.style.top),
    moved: false,
  };
  tile.setPointerCapture(e.pointerId);
  tile.addEventListener('pointermove', onTileMove);
  tile.addEventListener('pointerup', onTileUp, { once: true });
  tile.addEventListener('pointercancel', onTileUp, { once: true });
}

function onTileMove(e) {
  if (!drag) return;
  const dx = (e.clientX - drag.startX) / geo.s;
  const dy = (e.clientY - drag.startY) / geo.s;
  if (!drag.moved && Math.hypot(dx, dy) < 4) return;
  drag.moved = true;
  drag.tile.classList.add('dragging');
  const left = drag.left + dx;
  const top = drag.top + dy;
  drag.tile.style.left = `${left}px`;
  drag.tile.style.top = `${top}px`;
  const c = cellAt(left, top);
  const slot = $('#slot');
  slot.style.left = `${geo.ox + c.x * CW - 10}px`;
  slot.style.top = `${geo.oy + c.y * CH - 10}px`;
  slot.classList.add('show');
}

function onTileUp() {
  const d = drag;
  drag = null;
  if (!d) return;
  d.tile.removeEventListener('pointermove', onTileMove);
  if (!d.moved) return renderStage();
  const c = cellAt(parseFloat(d.tile.style.left), parseFloat(d.tile.style.top));
  const cells = S.layout.cells.map((x) => ({ ...x }));
  const mine = cells.find((x) => x.id === d.id);
  const other = cells.find((x) => x.x === c.x && x.y === c.y && x.id !== d.id);
  if (other) {
    other.x = mine.x;
    other.y = mine.y;
  }
  mine.x = c.x;
  mine.y = c.y;
  S.layout = { ...S.layout, cells };
  renderStage();
  invoke('set_layout', { cells });
}

// ---------------------------------------------------------------- status
function renderStatus() {
  const card = $('#statusCard');
  const online = S.devices.filter((d) => d.trusted && d.online);
  card.classList.remove('ok', 'active');
  let title;
  let sub;
  if (S.active !== S.me.id) {
    card.classList.add('active');
    title = `${nameOf(S.active)} 조작 중`;
    sub = '키보드·마우스를 보내는 중이에요';
  } else if (S.controlled_by) {
    card.classList.add('active');
    title = `${nameOf(S.controlled_by)}에서 조작 중`;
    sub = '다른 키보드로 움직이는 중이에요';
  } else if (online.length) {
    card.classList.add('ok');
    title = `${online.length}대 연결됨`;
    sub = online.map((d) => d.name).join(', ');
  } else {
    title = '연결된 기기 없음';
    sub = '기기 탭에서 연결해 보세요';
  }
  $('#statusTitle').textContent = title;
  $('#statusSub').textContent = sub;
  $('#lockToggle').checked = S.locked;
  renderPermBanner();

  const nearby = S.devices.filter((d) => !d.trusted).length;
  const badge = $('#nearbyBadge');
  badge.textContent = nearby;
  badge.classList.toggle('hidden', nearby === 0);
}

// Mac permissions: walk the user through the one step that is actually missing.
function renderPermBanner() {
  const banner = $('#permBanner');
  const p = S.perm || { accessibility: true, input_monitoring: true, installed: true };
  const needsHelp = S.me.os === 'mac' && (!S.permission || !p.accessibility || !p.installed);
  banner.classList.toggle('hidden', !needsHelp);
  if (!needsHelp) return;

  let title;
  let text;
  let actions;
  banner.classList.remove('ready');
  if (!p.installed) {
    title = '응용 프로그램 폴더로 옮겨 주세요';
    text = 'dmg 창에서 바로 실행하면 macOS가 권한을 기억하지 못해요. NoopFlow를 Applications 폴더로 끌어다 놓고, 거기서 다시 열어 주세요.';
    actions = [];
  } else if (!p.accessibility) {
    title = '손쉬운 사용 권한이 필요해요';
    text = '시스템 설정 → 개인정보 보호 및 보안 → 손쉬운 사용에서 NoopFlow를 켜 주세요. 이미 켜져 있다면 −로 지우고 다시 추가해 주세요.';
    actions = [['permission', '권한 설정 열기', true]];
  } else if (!p.input_monitoring) {
    title = '입력 모니터링 권한도 켜 주세요';
    text = '시스템 설정 → 개인정보 보호 및 보안 → 입력 모니터링에서 NoopFlow를 켠 뒤, 다시 시작을 눌러 주세요.';
    actions = [['input', '설정 열기', false], ['restart', '다시 시작', true]];
  } else {
    banner.classList.add('ready');
    title = '거의 다 됐어요! 다시 시작해 주세요';
    text = '권한은 켜졌어요. macOS는 앱을 다시 열어야 새 권한을 적용해요.';
    actions = [['restart', '다시 시작', true]];
  }
  $('#permTitle').textContent = title;
  $('#permText').textContent = text;
  $('#permActions').innerHTML = actions
    .map(([act, label, primary]) => `<button class="btn ${primary ? 'primary' : ''}" data-perm="${act}">${label}</button>`)
    .join('');
}

// ------------------------------------------------------------- clipboard
const FILE_ICON = '<svg viewBox="0 0 24 24"><path d="M14 3H7a2 2 0 0 0-2 2v14a2 2 0 0 0 2 2h10a2 2 0 0 0 2-2V8z"/><path d="M14 3v5h5"/></svg>';
const KIND_LABEL = { text: '글자', image: '이미지', files: '파일', empty: '비어 있음' };

function timeAgo(ms) {
  if (!ms) return '';
  const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (s < 10) return '방금 복사';
  if (s < 60) return `${s}초 전 복사`;
  if (s < 3600) return `${Math.floor(s / 60)}분 전 복사`;
  if (s < 86400) return `${Math.floor(s / 3600)}시간 전 복사`;
  return `${Math.floor(s / 86400)}일 전 복사`;
}

function clipBody(c) {
  if (!c || c.kind === 'empty') return { html: '<div class="clip-empty">복사된 게 없어요</div>', fade: false };
  if (c.kind === 'text') {
    const more = c.count > (c.text || '').length;
    return { html: `<pre class="clip-text">${esc(c.text || '')}${more ? '…' : ''}</pre>`, fade: (c.text || '').split('\n').length > 8 || (c.text || '').length > 400 };
  }
  if (c.kind === 'image') {
    return { html: `<div class="clip-image">${c.thumb ? `<img src="${c.thumb}" alt="" />` : ''}</div>`, fade: false };
  }
  const files = (c.files || []).slice(0, 8).map((f) => `
    <div class="clip-file">${FILE_ICON}<span class="fname">${esc(f.path)}</span><span class="fsize">${fmtBytes(f.size)}</span></div>`).join('');
  const rest = c.count > 8 ? `<div class="clip-meta">외 ${c.count - 8}개</div>` : '';
  return { html: `<div class="clip-files">${files}${rest}</div>`, fade: false };
}

function clipMeta(c) {
  if (!c || c.kind === 'empty') return '';
  if (c.kind === 'text') return `${c.count.toLocaleString()}자 · ${timeAgo(c.at)}`;
  if (c.kind === 'image') return `${c.width}×${c.height} · ${timeAgo(c.at)}`;
  const total = (c.files || []).reduce((a, f) => a + f.size, 0);
  return `${c.count}개 · ${fmtBytes(total)} · ${timeAgo(c.at)}`;
}

function renderClipboard() {
  const grid = $('#clipGrid');
  if (!grid || !S) return;
  const clips = Object.fromEntries((S.clipboards || []).map((c) => [c.device_id, c]));
  const online = S.devices.filter((d) => d.trusted && d.online);
  const cards = [S.me, ...online].map((d) => {
    const isMe = d.id === S.me.id;
    const c = clips[d.id];
    const oldPeer = !isMe && d.version && d.version < 3;
    const kind = c?.kind || 'empty';
    const body = oldPeer
      ? { html: `<div class="clip-empty">${esc(d.name)}의 NoopFlow를<br>0.3.0 이상으로 업데이트하면 보여요</div>`, fade: false }
      : clipBody(c);
    let actions = '';
    if (isMe) {
      actions = online.length
        ? online.map((p) => `<button class="btn" data-clip-push="${esc(p.id)}" ${kind === 'empty' ? 'disabled' : ''}>${esc(p.name)}로 보내기</button>`).join('')
        : '<span class="clip-meta">연결된 기기가 없어요</span>';
    } else if (!oldPeer) {
      const label = kind === 'files' ? '이 컴퓨터로 다운로드' : '이 컴퓨터로 가져오기';
      actions = `<button class="btn primary" data-clip-pull="${esc(d.id)}" ${kind === 'empty' ? 'disabled' : ''}>${label}</button>`;
    }
    return `
      <div class="clip-card ${isMe ? 'me' : ''}">
        <div class="clip-head">
          <div class="device-icon">${ICON[d.os] || ICON.linux}</div>
          <div class="clip-title">
            <span class="clip-name">${esc(d.name)}${isMe ? ' <span class="tag">이 컴퓨터</span>' : ''}</span>
            <span class="clip-meta">${esc(oldPeer ? '' : clipMeta(c))}</span>
          </div>
          <span class="kind-badge ${kind === 'empty' ? 'empty' : ''}">${KIND_LABEL[kind]}</span>
        </div>
        <div class="clip-body ${body.fade ? 'fade' : ''}">${body.html}</div>
        <div class="clip-actions">${actions}</div>
      </div>`;
  });
  grid.innerHTML = cards.join('');
}

// --------------------------------------------------------------- devices
function renderDevices() {
  const trusted = S.devices.filter((d) => d.trusted);
  const nearby = S.devices.filter((d) => !d.trusted);

  $('#trustedList').innerHTML = trusted.length
    ? trusted.map((d) => {
        const pill = d.online ? '<span class="pill ok">연결됨</span>' : d.nearby ? '<span class="pill wait">연결 중…</span>' : '<span class="pill">꺼져 있음</span>';
        return `
          <div class="device ${d.online ? 'online' : ''}">
            <div class="device-icon">${ICON[d.os] || ICON.linux}</div>
            <div class="device-text">
              <div class="device-name">${esc(d.name)}</div>
              <div class="device-meta">${pill}${OS_NAME[d.os]}${d.addr ? ` · ${esc(d.addr)}` : ''}</div>
            </div>
            <button class="btn ghost danger" data-unpair="${esc(d.id)}">연결 해제</button>
          </div>`;
      }).join('')
    : '<div class="empty">아직 연결한 기기가 없어요.<br>아래 <b>주변에서 찾은 기기</b>에서 연결해 보세요.</div>';

  $('#nearbyList').innerHTML = nearby.length
    ? nearby.map((d) => `
        <div class="device">
          <div class="device-icon">${ICON[d.os] || ICON.linux}</div>
          <div class="device-text">
            <div class="device-name">${esc(d.name)}</div>
            <div class="device-meta">${OS_NAME[d.os]}${d.addr ? ` · ${esc(d.addr)}` : ''}</div>
          </div>
          <button class="btn primary" data-pair="${esc(d.id)}">연결</button>
        </div>`).join('')
    : '<div class="empty">주변에 NoopFlow가 켜진 기기가 없어요.<br>다른 컴퓨터에서도 NoopFlow를 실행해 주세요.</div>';
}

// -------------------------------------------------------------- settings
function renderSettings() {
  const isMac = S.me.os === 'mac';
  $$('.mac-only').forEach((el) => el.classList.toggle('hidden', !isMac));
  $$('.win-only').forEach((el) => el.classList.toggle('hidden', isMac));
  $('#hangulCard .tag').textContent = isMac ? 'Mac' : 'Windows';

  for (const el of $$('[data-setting]')) {
    const key = el.dataset.setting;
    const v = S.settings[key];
    if (el.classList.contains('segmented')) {
      $$('button', el).forEach((b) => b.classList.toggle('on', b.dataset.value === v));
    } else if (el.type === 'checkbox') {
      el.checked = !!v;
    } else if (el.type === 'range' && document.activeElement !== el) {
      el.value = v;
      $('#speedValue').textContent = `${Number(v).toFixed(1)}×`;
    }
  }
  renderDiag();
  if (document.activeElement !== $('#nameInput')) $('#nameInput').value = S.me.name;
  $$('.win-only-row').forEach((el) => el.classList.toggle('hidden', isMac));
  $('#autostartTitle').textContent = isMac ? '로그인 시 자동 실행' : 'Windows 시작 시 자동 실행';
  $('#keyLayoutHint').textContent = {
    position: isMac
      ? '윈도우 키보드: Alt → ⌘, Windows 키 → ⌥, Ctrl → Ctrl. 키 위치가 맥 키보드와 같아져요.'
      : '맥 키보드: ⌘ → Alt, ⌥ → Windows 키, Ctrl → Ctrl. 키 위치가 윈도우 키보드와 같아져요.',
    ctrl_cmd: 'Ctrl과 ⌘(Windows 키)를 맞바꿔요. 윈도우 Ctrl+C가 맥에서 ⌘C가 돼요.',
    none: '키를 바꾸지 않고 그대로 보내요.',
  }[S.settings.key_layout] || '';
  $('#receiveDir').textContent = S.receive_dir;
}

function diagItems() {
  const p = S.perm || {};
  const d = S.diag || {};
  const yes = (v) => [v ? '켜짐' : '꺼짐', v ? 'good' : 'bad'];
  const items = [];
  if (S.me.os === 'mac') {
    items.push(['손쉬운 사용', ...yes(p.accessibility)]);
    items.push(['입력 모니터링', ...yes(p.input_monitoring)]);
    items.push(['설치 위치', p.installed ? '응용 프로그램' : 'dmg / 임시 위치', p.installed ? 'good' : 'bad']);
  }
  items.push(['입력 가로채기', S.permission ? '동작 중' : '멈춤', S.permission ? 'good' : 'bad']);
  items.push(['연결된 기기', `${S.devices.filter((x) => x.trusted && x.online).length}대`, '']);
  items.push(['받은 커서 진입', `${d.enters || 0}회${d.last_controller ? ` · ${d.last_controller}` : ''}`, '']);
  items.push(['받은 커서 이동', `${d.moves_in || 0}회`, '']);
  items.push(['보낸 커서 이동', `${d.moves_out || 0}회`, '']);
  return items;
}

function renderDiag() {
  $('#diagGrid').innerHTML = diagItems()
    .map(([label, value, cls]) => `<div class="diag-item"><span class="diag-label">${esc(label)}</span><span class="diag-value ${cls}">${esc(value)}</span></div>`)
    .join('');
}

function saveSetting(key, value) {
  S.settings = { ...S.settings, [key]: value };
  invoke('set_settings', { settings: S.settings });
}

// ----------------------------------------------------------------- modal
let modal = null; // { kind: 'input'|'code'|'confirm'|'done', ... }
let countdownTimer = null;

function openModal(html, state) {
  clearInterval(countdownTimer);
  modal = state;
  $('#modalBody').innerHTML = html;
  $('#modal').classList.remove('hidden');
}

function closeModal() {
  clearInterval(countdownTimer);
  modal = null;
  $('#modal').classList.add('hidden');
}

function showPairInput(id, error) {
  const name = esc(nameOf(id));
  openModal(`
    <div class="modal-icon">${ICON.link}</div>
    <h3>${name}에 연결</h3>
    <p><b>${name}</b> 화면에 뜬 6자리 코드를 입력하세요.</p>
    <div class="code-inputs" id="codeInputs">${'<input inputmode="numeric" autocomplete="off" />'.repeat(6)}</div>
    <div class="error" id="pairError">${esc(error || '')}</div>
    <div class="actions">
      <button class="btn ghost" data-act="cancel">취소</button>
      <button class="btn primary" data-act="submit" id="pairSubmit">연결</button>
    </div>`, { kind: 'input', id });

  const inputs = $$('#codeInputs input');
  inputs.forEach((inp, i) => {
    inp.addEventListener('input', () => {
      const digits = inp.value.replace(/\D/g, '');
      inp.value = '';
      digits.split('').slice(0, 6 - i).forEach((d, k) => { inputs[i + k].value = d; });
      const next = Math.min(i + Math.max(digits.length, 0), 5);
      if (digits) inputs[next].focus();
      if (inputs.every((x) => x.value)) submitPair();
    });
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Backspace' && !inp.value && i > 0) inputs[i - 1].focus();
      if (e.key === 'Enter') submitPair();
    });
    inp.addEventListener('paste', (e) => {
      const digits = (e.clipboardData.getData('text') || '').replace(/\D/g, '').slice(0, 6);
      if (!digits) return;
      e.preventDefault();
      digits.split('').forEach((d, k) => { if (inputs[k]) inputs[k].value = d; });
      inputs[Math.min(digits.length, 5)].focus();
      if (digits.length === 6) submitPair();
    });
  });
  setTimeout(() => inputs[0].focus(), 50);
}

function submitPair() {
  if (!modal || modal.kind !== 'input' || modal.busy) return;
  const code = $$('#codeInputs input').map((x) => x.value).join('');
  if (code.length !== 6) {
    $('#pairError').textContent = '6자리를 모두 입력해 주세요.';
    return;
  }
  modal.busy = true;
  const btn = $('#pairSubmit');
  btn.innerHTML = '<span class="spinner"></span>';
  btn.disabled = true;
  $('#pairError').textContent = '';
  invoke('pair_submit', { id: modal.id, code });
}

function pairFailed(error) {
  if (!modal || modal.kind !== 'input') return;
  const id = modal.id;
  showPairInput(id, `${error} 상대 화면에 새 코드가 떠요.`);
  $('#codeInputs').classList.add('shake');
  invoke('pair_request', { id });
}

function showPairCode(ev) {
  const digits = ev.code.split('').map((d) => `<span>${d}</span>`).join('');
  openModal(`
    <div class="modal-icon">${ICON.shield}</div>
    <h3>${esc(ev.name)}에서 연결을 요청했어요</h3>
    <p>${esc(ev.name)}에 아래 코드를 입력하세요.<br>내가 요청한 게 아니라면 거절하세요.</p>
    <div class="code-display">${digits}</div>
    <div class="countdown" id="countdown"></div>
    <div class="actions"><button class="btn" data-act="cancel">거절</button></div>`, { kind: 'code', id: ev.id });
  const until = Date.now() + 120000;
  const tick = () => {
    const left = Math.max(0, Math.round((until - Date.now()) / 1000));
    $('#countdown').textContent = `${Math.floor(left / 60)}:${String(left % 60).padStart(2, '0')} 후 만료`;
    if (!left) { invoke('pair_cancel'); closeModal(); }
  };
  tick();
  countdownTimer = setInterval(tick, 1000);
}

function showPairDone(name) {
  openModal(`
    <div class="modal-icon ok">${ICON.check}</div>
    <h3>연결됐어요!</h3>
    <p><b>${esc(name)}</b>와(과) 이제 키보드·마우스를 함께 써요.<br>화면 배치에서 실제 위치대로 맞춰 주세요.</p>
    <div class="actions"><button class="btn primary" data-act="to-layout">화면 배치로</button></div>`, { kind: 'done' });
}

function showUnpairConfirm(id) {
  const name = esc(nameOf(id));
  openModal(`
    <h3>${name} 연결을 해제할까요?</h3>
    <p>다시 쓰려면 코드로 다시 연결해야 해요.</p>
    <div class="actions">
      <button class="btn ghost" data-act="close">취소</button>
      <button class="btn danger" data-act="unpair">연결 해제</button>
    </div>`, { kind: 'confirm', id });
}

$('#modal').addEventListener('click', (e) => {
  const act = e.target.closest('[data-act]')?.dataset.act;
  if (!act && e.target.id === 'modal' && modal?.kind !== 'code') return closeModal();
  if (!act) return;
  if (act === 'submit') return submitPair();
  if (act === 'cancel') { invoke('pair_cancel'); return closeModal(); }
  if (act === 'close') return closeModal();
  if (act === 'unpair') { invoke('unpair', { id: modal.id }); return closeModal(); }
  if (act === 'to-layout') { closeModal(); return showTab('layout'); }
});

// ------------------------------------------------------------- transfers
const transfers = new Map();

function fmtBytes(n) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(0)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 / 1024).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

function onTransfer(ev) {
  const key = `${ev.incoming ? 'in' : 'out'}-${ev.peer}-${ev.id}`;
  let el = transfers.get(key);
  if (!el) {
    el = document.createElement('div');
    el.className = 'transfer';
    el.innerHTML = `
      <div class="transfer-top">
        <div class="transfer-icon">${ev.incoming ? ICON.down : ICON.up}</div>
        <div class="transfer-text"><div class="transfer-title"></div><div class="transfer-sub"></div></div>
      </div>
      <div class="bar"><i></i></div>`;
    $('#transfers').appendChild(el);
    transfers.set(key, el);
  }
  const pct = ev.total ? Math.min(100, (ev.done / ev.total) * 100) : ev.finished ? 100 : 0;
  $('.transfer-title', el).textContent = ev.title || '파일';
  const dir = ev.incoming ? `${ev.peer}에서 받는 중` : `${ev.peer}로 보내는 중`;
  let sub = `${dir} · ${fmtBytes(ev.done)} / ${fmtBytes(ev.total)}`;
  if (ev.finished && !ev.error) {
    el.classList.add('done');
    $('.transfer-icon', el).innerHTML = ICON.check;
    sub = ev.incoming ? `${ev.peer}에서 받기 완료 · ${fmtBytes(ev.total)}` : `${ev.peer}로 보내기 완료 · ${fmtBytes(ev.total)}`;
  }
  if (ev.error) {
    el.classList.add('failed');
    $('.transfer-icon', el).innerHTML = ICON.x;
    sub = ev.error;
  }
  $('.transfer-sub', el).textContent = sub;
  $('.bar i', el).style.width = `${pct}%`;
  if (ev.finished) {
    setTimeout(() => { el.remove(); transfers.delete(key); }, ev.error ? 8000 : 4500);
  }
}

// ---------------------------------------------------------- file drops
let dropTile = null;
function setDropTile(tile) {
  if (dropTile === tile) return;
  dropTile?.classList.remove('drop-ok', 'drop-no');
  dropTile = tile;
  if (!tile) return;
  const d = device(tile.dataset.id);
  const ok = d && d.id !== S.me.id && d.online;
  tile.classList.add(ok ? 'drop-ok' : 'drop-no');
}

function tileAtPhysical(pos) {
  const x = pos.x / window.devicePixelRatio;
  const y = pos.y / window.devicePixelRatio;
  return document.elementFromPoint(x, y)?.closest('.tile') || null;
}

function setupFileDrop() {
  if (MOCK || !T.webview) return;
  T.webview.getCurrentWebview().onDragDropEvent((e) => {
    const p = e.payload;
    if (p.type === 'leave') return setDropTile(null);
    if (p.type === 'enter' && !$('#tab-layout').classList.contains('active')) showTab('layout');
    const tile = p.position ? tileAtPhysical(p.position) : null;
    if (p.type === 'enter' || p.type === 'over') return setDropTile(tile);
    if (p.type === 'drop') {
      setDropTile(null);
      const d = tile && device(tile.dataset.id);
      if (!d || d.id === S.me.id) return toast('보낼 기기 위에 놓아 주세요', 'info');
      if (!d.online) return toast(`${d.name}이(가) 연결되어 있지 않아요`, 'error');
      invoke('send_files', { id: d.id, paths: p.paths || [] });
    }
  });
}

// ------------------------------------------------------------- events
function onCore(ev) {
  switch (ev.type) {
    case 'state':
      S = ev;
      renderAll();
      break;
    case 'toast':
      toast(ev.text, ev.level);
      break;
    case 'pairCode':
      showPairCode(ev);
      break;
    case 'pairCodeClosed':
      if (modal?.kind === 'code') closeModal();
      break;
    case 'pairResult':
      if (ev.ok) showPairDone(ev.name);
      else pairFailed(ev.error || '연결하지 못했어요.');
      break;
    case 'transfer':
      onTransfer(ev);
      break;
    case 'received':
      toast(`${ev.peer}에서 파일을 받았어요`, 'ok', { label: '보기', fn: () => invoke('reveal', { path: ev.paths[0] }) });
      break;
  }
}

function renderAll() {
  if (!S) return;
  renderStatus();
  renderStage();
  renderDevices();
  renderClipboard();
  renderSettings();
}

function bind() {
  $$('.nav-item').forEach((b) => b.addEventListener('click', () => showTab(b.dataset.tab)));
  $('#lockToggle').addEventListener('change', (e) => invoke('set_locked', { locked: e.target.checked }));
  $('#autostartToggle').addEventListener('change', async (e) => {
    try {
      e.target.checked = await invoke('set_autostart', { enabled: e.target.checked });
    } catch {
      toast('자동 실행 설정을 바꾸지 못했어요', 'error');
      e.target.checked = !e.target.checked;
    }
  });
  $('#permActions').addEventListener('click', (e) => {
    const act = e.target.closest('[data-perm]')?.dataset.perm;
    if (act === 'permission') invoke('request_permission');
    if (act === 'input') invoke('request_input_monitoring');
    if (act === 'restart') invoke('restart_app');
  });
  $('#openDirBtn').addEventListener('click', () => invoke('open_receive_dir'));

  $('#clipGrid').addEventListener('click', (e) => {
    const pull = e.target.closest('[data-clip-pull]')?.dataset.clipPull;
    const push = e.target.closest('[data-clip-push]')?.dataset.clipPush;
    if (pull) {
      invoke('clip_pull', { id: pull });
      toast(`${nameOf(pull)}에서 가져오는 중…`);
    }
    if (push) invoke('clip_push', { id: push });
  });
  // "n분 전" labels stay fresh while the tab is open.
  setInterval(() => {
    if ($('#tab-clipboard').classList.contains('active')) renderClipboard();
  }, 15000);

  $('#trustedList').addEventListener('click', (e) => {
    const id = e.target.closest('[data-unpair]')?.dataset.unpair;
    if (id) showUnpairConfirm(id);
  });
  $('#nearbyList').addEventListener('click', (e) => {
    const id = e.target.closest('[data-pair]')?.dataset.pair;
    if (!id) return;
    invoke('pair_request', { id });
    showPairInput(id);
  });
  $('#probeForm').addEventListener('submit', (e) => {
    e.preventDefault();
    const ip = $('#probeInput').value.trim();
    if (!ip) return;
    invoke('probe', { ip });
    toast(`${ip}에서 찾는 중…`);
    $('#probeInput').value = '';
  });

  for (const el of $$('[data-setting]')) {
    const key = el.dataset.setting;
    if (el.classList.contains('segmented')) {
      el.addEventListener('click', (e) => {
        const v = e.target.closest('button')?.dataset.value;
        if (v) { saveSetting(key, v); renderSettings(); }
      });
    } else if (el.type === 'checkbox') {
      el.addEventListener('change', () => saveSetting(key, el.checked));
    } else if (el.type === 'range') {
      el.addEventListener('input', () => { $('#speedValue').textContent = `${Number(el.value).toFixed(1)}×`; });
      el.addEventListener('change', () => saveSetting(key, Number(el.value)));
    }
  }
  const name = $('#nameInput');
  const saveName = () => { if (name.value.trim() && name.value.trim() !== S.me.name) invoke('set_name', { name: name.value.trim() }); };
  name.addEventListener('change', saveName);
  name.addEventListener('keydown', (e) => { if (e.key === 'Enter') name.blur(); });

  $('#diagCopy').addEventListener('click', async () => {
    const text = [`NoopFlow ${$('.version').textContent.replace('NoopFlow ', '')} · ${OS_NAME[S.me.os]} · ${S.me.name}`]
      .concat(diagItems().map(([l, v]) => `${l}: ${v}`))
      .join('\n');
    try {
      await navigator.clipboard.writeText(text);
      toast('진단 내용을 복사했어요', 'ok');
    } catch {
      toast('복사하지 못했어요', 'error');
    }
  });
  // Counters change constantly, so refresh them while the settings tab is open.
  setInterval(async () => {
    if (MOCK || !$('#tab-settings').classList.contains('active')) return;
    try {
      S = await invoke('get_state');
      renderDiag();
      renderPermBanner();
    } catch {}
  }, 1000);

  window.addEventListener('resize', () => renderStage());
  document.addEventListener('keydown', (e) => {
    if (e.key === 'Escape' && modal && modal.kind !== 'code') {
      if (modal.kind === 'input') invoke('pair_cancel');
      closeModal();
    }
  });
}

// --------------------------------------------------- window close (Windows)
function onCloseRequested() {
  const action = S?.settings?.close_action || 'ask';
  if (action === 'tray') return invoke('hide_window');
  if (action === 'quit') return invoke('quit_app');
  openModal(`
    <div class="modal-icon">${ICON.cursor}</div>
    <h3>NoopFlow를 어떻게 할까요?</h3>
    <p>트레이로 최소화하면 키보드·마우스 공유가 계속 동작해요.<br>종료하면 다른 컴퓨터와 연결이 끊겨요.</p>
    <label class="remember"><input type="checkbox" id="closeRemember" /> 다음부터 묻지 않기</label>
    <div class="actions">
      <button class="btn ghost" data-close="cancel">취소</button>
      <button class="btn danger" data-close="quit">종료</button>
      <button class="btn primary" data-close="tray">최소화</button>
    </div>`, { kind: 'close' });
}

$('#modal').addEventListener('click', (e) => {
  const act = e.target.closest('[data-close]')?.dataset.close;
  if (!act) return;
  if (act !== 'cancel' && $('#closeRemember')?.checked) saveSetting('close_action', act);
  closeModal();
  if (act === 'tray') invoke('hide_window');
  if (act === 'quit') invoke('quit_app');
});

async function showVersion() {
  let v = (document.querySelector('.version')?.textContent || '').replace('NoopFlow ', '').trim();
  try {
    if (!MOCK && T.app?.getVersion) v = await T.app.getVersion();
  } catch {}
  if (!v) return;
  $('#appVersion').textContent = `v${v}`;
  const footer = document.querySelector('.version');
  if (footer) footer.textContent = `NoopFlow ${v}`;
}

async function init() {
  bind();
  showVersion();
  if (!MOCK) {
    await T.event.listen('core', (e) => onCore(e.payload));
    await T.event.listen('close-requested', onCloseRequested);
    await T.event.listen('open-tab', (e) => showTab(e.payload));
    await T.event.listen('autostart', (e) => { $('#autostartToggle').checked = !!e.payload; });
    invoke('get_autostart').then((on) => { $('#autostartToggle').checked = !!on; });
    setupFileDrop();
  }
  for (let i = 0; i < 20 && !S; i++) {
    try {
      S = await invoke('get_state');
    } catch {
      await new Promise((r) => setTimeout(r, 150));
    }
  }
  renderAll();
  if (MOCK) applyPreviewHash();
}

// ----------------------------------------------- browser preview (no Tauri)
// #devices / #settings open a tab, #pair shows the pairing code dialog,
// ?os=mac previews the Mac-only settings. Used for README screenshots.
function applyPreviewHash() {
  const h = location.hash.slice(1);
  if (h === 'devices' || h === 'settings' || h === 'clipboard') showTab(h);
  if (h === 'pair') showPairCode({ id: 'mac', name: 'MacBook Pro', code: '482915' });
  if (h === 'close') onCloseRequested();
}
function mockInvoke(cmd, args) {
  if (cmd === 'get_state') {
    return Promise.resolve({
      me: { id: 'me', name: 'JH-DESKTOP', os: new URLSearchParams(location.search).get('os') || 'windows', trusted: true, online: true, nearby: true, addr: null, screen: { x: 0, y: 0, w: 2560, h: 1440 } },
      devices: [
        { id: 'mac', name: 'MacBook Pro', os: 'mac', trusted: true, online: true, nearby: true, addr: '192.168.0.24', screen: { x: 0, y: 0, w: 1728, h: 1117 }, version: 3 },
        { id: 'lap', name: '거실 노트북', os: 'windows', trusted: false, online: false, nearby: true, addr: '192.168.0.31', screen: null },
      ],
      layout: { version: 1, cells: [{ id: 'me', x: 0, y: 0 }, { id: 'mac', x: 1, y: 0 }] },
      active: 'mac',
      controlled_by: null,
      locked: false,
      settings: { right_cmd_hangul: true, lang1_hangul: true, right_alt_hangul: false, hangul_method: 'direct', key_layout: 'position', clipboard_sync: true, paste_files: true, invert_scroll: false, mouse_speed: 1.0, receive_dir: null, corner_guard: 6, close_action: 'ask' },
      // ?perm=install|access|input|restart previews the Mac permission banner.
      ...(() => {
        const q = new URLSearchParams(location.search).get('perm');
        const states = {
          install: { installed: false, accessibility: false, input_monitoring: false },
          access: { installed: true, accessibility: false, input_monitoring: false },
          input: { installed: true, accessibility: true, input_monitoring: false },
          restart: { installed: true, accessibility: true, input_monitoring: true },
        };
        return { permission: !q, perm: states[q] || states.restart };
      })(),
      clipboards: [
        { device_id: 'me', kind: 'text', text: 'const sum = (a, b) => a + b;\nconsole.log(sum(2, 3)); // 5\n\n// 맥에서 바로 붙여넣기 해 보세요', count: 82, width: 0, height: 0, thumb: null, files: [], at: Date.now() - 45000 },
        { device_id: 'mac', kind: 'files', text: null, count: 3, width: 0, height: 0, thumb: null, files: [{ path: '기획안_최종.pdf', size: 2483000 }, { path: 'screenshot 2026-11-22.png', size: 812000 }, { path: 'notes.md', size: 4100 }], at: Date.now() - 360000 },
      ],
      receive_dir: 'C:\\Users\\me\\Downloads\\NoopFlow',
      pairing_with: null,
    });
  }
  if (cmd === 'pair_submit') setTimeout(() => onCore({ type: 'pairResult', ok: true, name: nameOf(args.id) }), 700);
  return Promise.resolve();
}

init();
