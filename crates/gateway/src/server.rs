use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::{HeaderMap, Method, StatusCode};
use axum::response::IntoResponse;
use axum::response::Response;
use bytes::Bytes;
use http_body_util::BodyStream;
use openstack_aws_protocol::{
    AwsProtocol, ec2::parse_ec2_request, json::parse_json_request, query::parse_query_request,
    rest_json::parse_rest_json_request, rest_xml::parse_rest_xml_request,
};
use openstack_config::Config;
use openstack_service_framework::traits::ResponseBody;
use openstack_service_framework::{BodyReader, ServicePluginManager, SpooledBody};
use openstack_state::StateManager;
use rand::RngExt;
use rand::SeedableRng;
use rand::rngs::SmallRng;
use tokio_util::io::StreamReader;
use tower::ServiceBuilder;
use tower_http::compression::CompressionLayer;
use tracing::{debug, error, info, warn};

use crate::context::RequestContext;
use crate::cors::CorsHandler;
use crate::sigv4::{
    DEFAULT_ACCESS_KEY, DEFAULT_REGION, access_key_to_account_id, is_valid_region, parse_sigv4_auth,
};

const STUDIO_SPA: &str = r#"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width,initial-scale=1" />
  <title>openstack studio</title>
  <link rel="stylesheet" href="/_localstack/studio/assets/app.css" />
</head>
<body>
  <div id="studio-app">Loading Studio dashboard...</div>
  <script src="/_localstack/studio/assets/app.js"></script>
</body>
</html>
"#;

const STUDIO_ASSET_JS: &str = r#"(function () {
'use strict';
// ─── openstack Studio SPA v2 ───────────────────────────────────────────────
// Tab-based service explorer: Overview · Operations · Storage · Transactions
// Zero external dependencies. Compiled into the gateway binary.
// ───────────────────────────────────────────────────────────────────────────

const root = document.getElementById('studio-app');
if (!root) return;

// ── State ──────────────────────────────────────────────────────────────────
const S = {
  // catalogue data
  services: [],       // [{name, status, support_tier}]
  flowCatalog: [],    // [{service, protocol, flow_count, maturity}]
  flowCoverage: [],   // [{service, has_manifest, l1_flows, quality}]
  opsCatalog: {},     // service → [{name,method,path,has_guided_flow}]

  // explorer navigation
  selectedService: null,
  activeTab: 'overview',   // overview | operations | storage | transactions

  // service detail data (per selected service)
  flowDefinition: null,    // {flows:[{id,level,steps,cleanup}], inputs:[]}
  storage: null,           // {snapshot:{...}}
  transactions: [],        // [{id,method,path,status,outcome,...}]
  txSummary: null,

  // guided interaction form
  guided: {
    selectedFlowId: null,
    inputs: {},
    captures: {},
    running: false,
    cleaningUp: false,
    log: [],             // [{stepId, title, status, body, duration_ms, isCleanup, captures}]
  },

  // raw console
  raw: { method: 'GET', path: '/_localstack/health', headers: '', body: '' },
  rawResponse: null,

  // tx filter
  txFilter: { outcome: '', guidedOnly: false },

  // ops filter
  opsFilter: { query: '', guidedOnly: false },

  // polling
  runtimeConfig: null,  // populated from /_localstack/studio-api/runtime-config
  _pollTimers: [],      // managed by startPolling/stopPolling

  // ui state
  loading: {},   // keyed loading flags
  errors: {},    // keyed error messages
  theme: localStorage.getItem('studio-theme') || 'dark',
};

// ── Helpers ────────────────────────────────────────────────────────────────
function esc(v) {
  return String(v ?? '').replace(/[&<>"']/g, c =>
    ({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
}

function setTheme(t) {
  S.theme = t;
  localStorage.setItem('studio-theme', t);
  document.documentElement.setAttribute('data-theme', t);
}

function fmt(v, fallback = '—') { return v != null && v !== '' ? v : fallback; }

function outcomeClass(o) {
  return {success:'outcome-ok', client_error:'outcome-warn',
          server_error:'outcome-err', pending:'outcome-pending'}[o] || '';
}

function methodBadge(m) {
  const cls = {GET:'method-get',POST:'method-post',PUT:'method-put',
               DELETE:'method-del',PATCH:'method-patch',HEAD:'method-head'}[m] || 'method-other';
  return `<span class="method-badge ${cls}">${esc(m)}</span>`;
}

function statusBadge(s) {
  const cls = s >= 500 ? 'status-err' : s >= 400 ? 'status-warn' : s >= 200 ? 'status-ok' : 'status-pending';
  return `<span class="status-badge ${cls}">${esc(s || '…')}</span>`;
}

// ── API ────────────────────────────────────────────────────────────────────
const BASE = '';

async function api(path, opts = {}) {
  const r = await fetch(BASE + path, {
    headers: { accept: 'application/json', ...opts.headers },
    ...opts,
  });
  if (!r.ok) throw new Error(`${path} → ${r.status}`);
  return r.json();
}

async function postJson(path, body) {
  return api(path, {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
}

// ── Data loading ──────────────────────────────────────────────────────────
async function loadCatalogue() {
  setLoading('catalogue', true);
  try {
    const [svcs, cat, cov, ops] = await Promise.all([
      api('/_localstack/studio-api/services'),
      api('/_localstack/studio-api/flows/catalog'),
      api('/_localstack/studio-api/flows/coverage'),
      api('/_localstack/studio-api/operations'),
    ]);
    S.services    = svcs.services  || [];
    S.flowCatalog = cat.services   || [];
    S.flowCoverage = cov.services  || [];
    for (const svcOps of (ops.services || [])) {
      S.opsCatalog[svcOps.service] = svcOps.operations || [];
    }
    clearError('catalogue');
  } catch (e) {
    setError('catalogue', e.message);
  } finally {
    setLoading('catalogue', false);
  }
}

async function loadServiceDetail(service) {
  setLoading('detail', true);
  try {
    const [flowDef, stor, txResp] = await Promise.all([
      api(`/_localstack/studio-api/flows/${service}`).catch(() => ({ flows: [], inputs: [] })),
      api(`/_localstack/studio-api/storage/${service}`).catch(() => null),
      api(`/_localstack/studio-api/transactions/${service}?limit=100`).catch(() => ({ transactions: [], total: 0 })),
    ]);
    S.flowDefinition = flowDef;
    S.storage        = stor;
    S.transactions   = txResp.transactions || [];
    S.txSummary      = { total: txResp.total || 0 };
    // Pre-select first flow
    if (flowDef.flows?.length && !S.guided.selectedFlowId) {
      S.guided.selectedFlowId = flowDef.flows[0].id;
      S.guided.inputs = {};
      S.guided.captures = {};
      S.guided.log = [];
    }
    clearError('detail');
  } catch (e) {
    setError('detail', e.message);
  } finally {
    setLoading('detail', false);
  }
}

async function refreshTransactions() {
  if (!S.selectedService) return;
  try {
    const params = new URLSearchParams({ limit: 200 });
    if (S.txFilter.outcome) params.set('outcome', S.txFilter.outcome);
    if (S.txFilter.guidedOnly) params.set('guided_only', 'true');
    const r = await api(`/_localstack/studio-api/transactions/${S.selectedService}?${params}`);
    S.transactions = r.transactions || [];
    S.txSummary = { total: r.total || 0 };
    delete S.errors.transactions;
    if (S.activeTab === 'transactions') render();
  } catch (e) {
    S.errors.transactions = e.message;
    if (S.activeTab === 'transactions') render();
  }
}

async function refreshStorage() {
  if (!S.selectedService) return;
  try {
    S.storage = await api(`/_localstack/studio-api/storage/${S.selectedService}`);
    delete S.errors.storage;
    render();
  } catch (e) {
    S.errors.storage = e.message;
    render();
  }
}

// ── Loading / error helpers ────────────────────────────────────────────────
function setLoading(k, v) { S.loading[k] = v; render(); }
function setError(k, v)   { S.errors[k] = v; render(); }
function clearError(k)    { delete S.errors[k]; }

// ── Render ────────────────────────────────────────────────────────────────
function render() {
  root.innerHTML = layout();
  bindEvents();
}

function layout() {
  return `
<div class="app" data-theme="${esc(S.theme)}">
  ${topBar()}
  <div class="workspace">
    ${sidebar()}
    <div class="main">
      ${S.selectedService ? explorerView() : welcomeView()}
    </div>
  </div>
</div>`;
}

// ── Top bar ────────────────────────────────────────────────────────────────
function topBar() {
  const health = S.services.length ? `<span class="chip chip-ok">${S.services.length} services</span>` : '';
  const pollIndicator = S.selectedService
    ? `<span class="chip chip-live" title="Polling active — transactions every ${S.runtimeConfig?.polling?.transactions_interval_ms||3000}ms, storage every ${S.runtimeConfig?.polling?.storage_interval_ms||5000}ms">● live</span>`
    : '';
  return `
<header class="topbar">
  <div class="topbar-brand">
    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
      <rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>
    </svg>
    <span class="topbar-title">openstack studio</span>
    ${health}
    ${pollIndicator}
  </div>
  <div class="topbar-actions">
    <button class="icon-btn" data-action="refresh-all" title="Refresh all">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <path d="M3 12a9 9 0 0 1 9-9 9.75 9.75 0 0 1 6.74 2.74L21 8"/><path d="M21 3v5h-5"/>
        <path d="M21 12a9 9 0 0 1-9 9 9.75 9.75 0 0 1-6.74-2.74L3 16"/><path d="M8 16H3v5"/>
      </svg>
    </button>
    <button class="icon-btn" data-action="toggle-theme" title="Toggle theme">
      <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
        <circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>
      </svg>
    </button>
  </div>
</header>`;
}

// ── Sidebar ────────────────────────────────────────────────────────────────
function sidebar() {
  if (S.loading.catalogue) return `<aside class="sidebar"><div class="sidebar-loading">Loading services…</div></aside>`;
  if (S.errors.catalogue) return `<aside class="sidebar"><div class="sidebar-error">${esc(S.errors.catalogue)}</div></aside>`;

  const byFlow = new Map(S.flowCatalog.map(x => [x.service, x]));
  const rows = S.services.map(s => {
    const flow = byFlow.get(s.name) || {};
    const isSelected = s.name === S.selectedService;
    const tier = s.support_tier === 'guided' ? `<span class="chip chip-guided">guided</span>` : `<span class="chip chip-raw">raw</span>`;
    const statusDot = `<span class="dot dot-${s.status}"></span>`;
    return `
<button class="svc-card ${isSelected ? 'svc-card--active' : ''}" data-select="${esc(s.name)}">
  <div class="svc-card-row">
    ${statusDot}<strong>${esc(s.name)}</strong>${tier}
  </div>
  <div class="svc-card-meta">${esc(flow.protocol || '—')} · ${esc(flow.flow_count ?? 0)} flows</div>
</button>`;
  });

  return `
<aside class="sidebar">
  <div class="sidebar-search">
    <input class="search-input" placeholder="Filter services…" id="svc-search" value="${esc(S._svcSearch || '')}">
  </div>
  <div class="sidebar-list" id="svc-list">
    ${rows.join('') || '<div class="sidebar-empty">No services registered</div>'}
  </div>
</aside>`;
}

// ── Welcome ────────────────────────────────────────────────────────────────
function welcomeView() {
  const guided = S.services.filter(s => s.support_tier === 'guided').length;
  const total  = S.services.length;
  return `
<div class="welcome">
  <div class="welcome-hero">
    <svg width="48" height="48" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" opacity="0.4">
      <rect x="2" y="3" width="20" height="14" rx="2"/><path d="M8 21h8M12 17v4"/>
    </svg>
    <h1>openstack studio</h1>
    <p>Select a service from the sidebar to inspect its operations, storage state, and live transactions.</p>
  </div>
  ${total ? `
  <div class="stat-row">
    <div class="stat-card"><div class="stat-num">${total}</div><div class="stat-label">Services</div></div>
    <div class="stat-card"><div class="stat-num">${guided}</div><div class="stat-label">Guided</div></div>
    <div class="stat-card"><div class="stat-num">${Object.keys(S.opsCatalog).reduce((a,k)=>a+S.opsCatalog[k].length,0)}</div><div class="stat-label">Operations</div></div>
  </div>` : ''}
</div>`;
}

// ── Explorer ───────────────────────────────────────────────────────────────
function explorerView() {
  const tabs = ['overview','operations','storage','transactions'];
  const tabBar = tabs.map(t => `
<button class="tab ${S.activeTab === t ? 'tab--active' : ''}" data-tab="${t}">
  ${t.charAt(0).toUpperCase() + t.slice(1)}${tabBadge(t)}
</button>`).join('');

  const panel = {
    overview:     overviewTab,
    operations:   operationsTab,
    storage:      storageTab,
    transactions: transactionsTab,
  }[S.activeTab]?.() ?? '';

  return `
<div class="explorer">
  <div class="explorer-header">
    <div class="explorer-title">
      <h2>${esc(S.selectedService)}</h2>
      ${serviceMeta()}
    </div>
    <div class="tab-bar">${tabBar}</div>
  </div>
  <div class="explorer-body">
    ${S.loading.detail ? '<div class="panel-loading">Loading…</div>' :
      S.errors.detail ? `<div class="tab-error">⚠ ${esc(S.errors.detail)} <button class="btn btn-ghost" data-action="retry-detail">Retry</button></div>` :
      panel}
  </div>
</div>`;
}

function tabBadge(t) {
  if (t === 'transactions' && S.transactions.length) return ` <span class="tab-badge">${S.transactions.length}</span>`;
  if (t === 'operations') {
    const ops = S.opsCatalog[S.selectedService] || [];
    if (ops.length) return ` <span class="tab-badge">${ops.length}</span>`;
  }
  return '';
}

function serviceMeta() {
  const svc = S.services.find(s => s.name === S.selectedService);
  const flow = S.flowCatalog.find(f => f.service === S.selectedService);
  if (!svc) return '';
  const statusDot = `<span class="dot dot-${svc.status}"></span>`;
  return `<div class="explorer-meta">${statusDot} ${esc(svc.status)} · ${esc(flow?.protocol || '—')}</div>`;
}

// ── Overview tab ───────────────────────────────────────────────────────────
function overviewTab() {
  const svc   = S.services.find(s => s.name === S.selectedService) || {};
  const flow  = S.flowCatalog.find(f => f.service === S.selectedService) || {};
  const cov   = S.flowCoverage.find(f => f.service === S.selectedService) || {};
  const ops   = S.opsCatalog[S.selectedService] || [];
  const stor  = S.storage?.snapshot;
  const txs   = S.transactions;

  const storCount  = storageResourceCount(stor);
  const errCount   = txs.filter(t => t.outcome === 'client_error' || t.outcome === 'server_error').length;
  const okCount    = txs.filter(t => t.outcome === 'success').length;
  const avgDur     = txs.filter(t=>t.duration_ms!=null).length
    ? Math.round(txs.reduce((a,t)=>a+(t.duration_ms||0),0)/txs.filter(t=>t.duration_ms!=null).length)
    : null;

  return `
<div class="overview-grid">
  <div class="ov-card">
    <div class="ov-label">Status</div>
    <div class="ov-value"><span class="dot dot-${svc.status}"></span> ${esc(svc.status || '—')}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Protocol</div>
    <div class="ov-value">${esc(flow.protocol || '—')}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Tier</div>
    <div class="ov-value">${svc.support_tier === 'guided'
      ? '<span class="chip chip-guided">guided</span>'
      : '<span class="chip chip-raw">raw</span>'}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Operations</div>
    <div class="ov-value ov-num">${ops.length || '—'}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Guided flows</div>
    <div class="ov-value ov-num">${flow.flow_count ?? '—'}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Coverage</div>
    <div class="ov-value">${esc(cov.quality || '—')}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Storage resources</div>
    <div class="ov-value ov-num">${storCount ?? '—'}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Transactions (session)</div>
    <div class="ov-value ov-num">${txs.length}</div>
  </div>
  <div class="ov-card">
    <div class="ov-label">Errors</div>
    <div class="ov-value ${errCount > 0 ? 'ov-err' : 'ov-ok'}">${errCount}</div>
  </div>
  ${avgDur != null ? `
  <div class="ov-card">
    <div class="ov-label">Avg latency</div>
    <div class="ov-value ov-num">${avgDur} ms</div>
  </div>` : ''}
</div>
${guidedPanel()}`;
}

function storageResourceCount(snap) {
  if (!snap) return null;
  const arrays = Object.values(snap).filter(Array.isArray);
  return arrays.reduce((a, arr) => a + arr.length, 0);
}

// ── Guided interaction panel ───────────────────────────────────────────────
function guidedPanel() {
  const flows = S.flowDefinition?.flows || [];
  if (!flows.length) return `
<div class="panel-section">
  <div class="panel-section-title">Guided interaction</div>
  <div class="empty-state">No guided flows available for this service.</div>
  ${rawConsolePanel()}
</div>`;

  const flowTabs = flows.map(f => `
<button class="sub-tab ${S.guided.selectedFlowId === f.id ? 'sub-tab--active' : ''}" data-flow="${esc(f.id)}">
  ${esc(f.id)} <span class="chip chip-level">${esc(f.level)}</span>
</button>`).join('');

  const selectedFlow = flows.find(f => f.id === S.guided.selectedFlowId) || flows[0];
  const inputs = S.flowDefinition?.inputs || [];

  const inputFields = inputs.map(inp => `
<div class="field-row">
  <label class="field-label">${esc(inp.name)}${inp.required ? ' <span class="req">*</span>' : ''}</label>
  ${inp.description ? `<div class="field-desc">${esc(inp.description)}</div>` : ''}
  <input class="field-input" data-input="${esc(inp.name)}"
    value="${esc(S.guided.inputs[inp.name] || '')}"
    placeholder="${esc(inp.name)}" />
</div>`).join('');

  const steps = selectedFlow?.steps || [];
  const stepPreview = steps.map((s,i) => {
    const log = S.guided.log.find(l => l.stepId === s.id);
    const stateClass = log ? (log.isCleanup ? (log.success ? 'step-cleanup-ok' : 'step-cleanup-err') : (log.success ? 'step-ok' : 'step-err')) : '';
    return `
<div class="step-row ${stateClass}">
  <span class="step-num">${i+1}</span>
  <div class="step-body">
    <div class="step-title">${esc(s.title)}</div>
    <div class="step-op">${methodBadge(s.operation?.method || 'GET')} <code>${esc(s.operation?.path || '/')}</code></div>
    ${log ? `<div class="step-result">${statusBadge(log.status)} ${log.duration_ms != null ? `<span class="step-dur">${log.duration_ms}ms</span>` : ''}
      <pre class="step-body-preview">${esc((log.body||'').slice(0,400))}</pre>
    </div>` : ''}
    ${s.error_guidance && log && !log.success ? `<div class="step-guidance">${esc(s.error_guidance)}</div>` : ''}
  </div>
</div>`;
  }).join('');

  return `
<div class="panel-section">
  <div class="panel-section-title">Guided interaction</div>
  <div class="sub-tabs">${flowTabs}</div>
  ${inputs.length ? `
  <div class="guided-inputs">
    <div class="inputs-title">Inputs</div>
    ${inputFields}
  </div>` : ''}
  <div class="guided-steps">${stepPreview || '<div class="empty-state">No steps in this flow.</div>'}</div>
  <div class="guided-actions">
    <button class="btn btn-primary ${S.guided.running || S.guided.cleaningUp ? 'btn-loading' : ''}"
      data-action="run-guided" ${S.guided.running || S.guided.cleaningUp ? 'disabled' : ''}>
      ${S.guided.cleaningUp ? 'Cleaning up…' : S.guided.running ? 'Running…' : '▶ Run flow'}
    </button>
    ${S.guided.log.length ? `<button class="btn btn-ghost" data-action="clear-guided">Clear results</button>` : ''}
  </div>
</div>
${rawConsolePanel()}`;
}

// ── Raw console panel ──────────────────────────────────────────────────────
function rawConsolePanel() {
  const resp = S.rawResponse;
  return `
<div class="panel-section">
  <div class="panel-section-title">Raw request</div>
  <div class="raw-form">
    <div class="raw-row">
      <select class="raw-method" id="raw-method">
        ${['GET','POST','PUT','DELETE','PATCH','HEAD'].map(m =>
          `<option ${S.raw.method===m?'selected':''}>${m}</option>`).join('')}
      </select>
      <input class="raw-path" id="raw-path" value="${esc(S.raw.path)}" placeholder="/_localstack/health" />
    </div>
    <textarea class="raw-body" id="raw-body" rows="3" placeholder="Request body (JSON / XML)">${esc(S.raw.body)}</textarea>
    <button class="btn btn-primary" data-action="run-raw">Send</button>
  </div>
  ${resp ? `
  <div class="raw-response">
    ${statusBadge(resp.status)}
    <pre class="resp-body">${esc(resp.body.slice(0,4000))}</pre>
  </div>` : ''}
</div>`;
}

// ── Operations tab ─────────────────────────────────────────────────────────
function operationsTab() {
  const all  = S.opsCatalog[S.selectedService] || [];
  const q    = (S.opsFilter.query || '').toLowerCase();
  const ops  = all.filter(op => {
    const nameOk = !q || op.name.toLowerCase().includes(q);
    const gOk    = !S.opsFilter.guidedOnly || op.has_guided_flow;
    return nameOk && gOk;
  });

  const guided = all.filter(o => o.has_guided_flow).length;

  if (!all.length) return `<div class="empty-state">No operation catalogue for this service.</div>`;

  const rows = ops.map(op => `
<div class="op-row" data-op="${esc(op.name)}">
  <div class="op-left">
    ${methodBadge(op.method)}
    <div class="op-name">${esc(op.name)}</div>
    ${op.has_guided_flow ? '<span class="chip chip-guided">guided</span>' : ''}
  </div>
  <code class="op-path">${esc(op.path)}</code>
  <button class="btn-tiny" data-fill-raw="${esc(op.name)}" title="Fill raw console">↗</button>
</div>`).join('');

  return `
<div class="ops-toolbar">
  <input class="search-input" id="ops-search" placeholder="Search operations…" value="${esc(S.opsFilter.query)}">
  <label class="toggle-label">
    <input type="checkbox" id="ops-guided-only" ${S.opsFilter.guidedOnly?'checked':''}>
    Guided only
  </label>
  <span class="ops-stats">${ops.length} / ${all.length} · ${guided} guided</span>
</div>
<div class="op-list">
  ${rows || '<div class="empty-state">No operations match the filter.</div>'}
</div>`;
}

// ── Storage tab ────────────────────────────────────────────────────────────
function storageTab() {
  if (S.errors.storage) return `<div class="tab-error">⚠ Failed to load storage: ${esc(S.errors.storage)} <button class="btn btn-ghost" data-action="refresh-storage">Retry</button></div>`;
  const snap = S.storage?.snapshot;

  return `
<div class="storage-toolbar">
  <button class="btn btn-ghost" data-action="refresh-storage">↻ Refresh</button>
</div>
${snap ? renderStorageSnapshot(snap) : '<div class="empty-state">No storage snapshot available. The service may not implement storage introspection.</div>'}`;
}

function renderStorageSnapshot(snap) {
  // snap is {kind:'s3', buckets:[...]} etc.
  const sections = [];
  for (const [key, val] of Object.entries(snap)) {
    if (key === 'kind') continue;
    if (!Array.isArray(val)) continue;
    sections.push(renderStorageSection(key, val));
  }
  if (!sections.length) return '<div class="empty-state">No resources found.</div>';
  return sections.join('');
}

function renderStorageSection(heading, resources) {
  if (!resources.length) return `
<div class="storage-section">
  <div class="storage-section-title">${esc(heading)} <span class="chip">0</span></div>
  <div class="empty-state">Empty</div>
</div>`;

  const rows = resources.map(r => {
    const attrs = (r.attributes || []).map(a =>
      `<span class="attr-pair"><span class="attr-key">${esc(a.key)}</span><span class="attr-val">${esc(a.value)}</span></span>`
    ).join('');
    return `
<div class="resource-row">
  <div class="resource-id" title="${esc(r.id)}">${esc(r.id)}</div>
  ${r.created_at ? `<div class="resource-ts">${esc(r.created_at.slice(0,19).replace('T',' '))}</div>` : ''}
  <div class="resource-attrs">${attrs}</div>
</div>`;
  }).join('');

  return `
<div class="storage-section">
  <div class="storage-section-title">${esc(heading.replace(/_/g,' '))} <span class="chip">${resources.length}</span></div>
  <div class="resource-list">${rows}</div>
</div>`;
}

// ── Transactions tab ──────────────────────────────────────────────────────
function transactionsTab() {
  if (S.errors.transactions) return `<div class="tab-error">⚠ Failed to load transactions: ${esc(S.errors.transactions)} <button class="btn btn-ghost" data-action="refresh-tx">Retry</button></div>`;
  const summary = S.txSummary;
  const txs = S.transactions;

  const summaryBar = summary ? `
<div class="tx-summary">
  <span class="tx-stat">Total <strong>${summary.total}</strong></span>
  <span class="tx-stat outcome-ok">✓ ${txs.filter(t=>t.outcome==='success').length}</span>
  <span class="tx-stat outcome-warn">⚠ ${txs.filter(t=>t.outcome==='client_error').length}</span>
  <span class="tx-stat outcome-err">✗ ${txs.filter(t=>t.outcome==='server_error').length}</span>
</div>` : '';

  const toolbar = `
<div class="tx-toolbar">
  ${summaryBar}
  <div class="tx-filters">
    <select id="tx-outcome-filter" class="filter-select">
      <option value="">All outcomes</option>
      <option value="success" ${S.txFilter.outcome==='success'?'selected':''}>Success</option>
      <option value="client_error" ${S.txFilter.outcome==='client_error'?'selected':''}>Client error</option>
      <option value="server_error" ${S.txFilter.outcome==='server_error'?'selected':''}>Server error</option>
    </select>
    <label class="toggle-label">
      <input type="checkbox" id="tx-guided-only" ${S.txFilter.guidedOnly?'checked':''}>
      Guided only
    </label>
    <button class="btn btn-ghost" data-action="refresh-tx">↻ Refresh</button>
    <button class="btn btn-ghost btn-danger" data-action="clear-tx">Clear</button>
  </div>
</div>`;

  if (!txs.length) return `${toolbar}<div class="empty-state">No transactions recorded yet. Run a guided flow or raw request to populate this log.</div>`;

  const rows = txs.map(t => `
<div class="tx-row ${outcomeClass(t.outcome)}">
  <div class="tx-id">#${t.id}</div>
  ${methodBadge(t.method)}
  <div class="tx-path" title="${esc(t.path)}">${esc(t.path)}</div>
  ${t.operation ? `<code class="tx-op">${esc(t.operation)}</code>` : ''}
  ${statusBadge(t.status)}
  ${t.duration_ms != null ? `<span class="tx-dur">${t.duration_ms}ms</span>` : ''}
  ${t.from_guided_flow ? '<span class="chip chip-guided">guided</span>' : ''}
  <button class="btn-tiny" data-replay-tx="${esc(t.id)}" title="Replay in raw console">↗</button>
</div>`).join('');

  return `${toolbar}<div class="tx-list">${rows}</div>`;
}

// ── Event binding ─────────────────────────────────────────────────────────
function bindEvents() {
  // Theme toggle
  root.querySelector('[data-action="toggle-theme"]')?.addEventListener('click', () => {
    setTheme(S.theme === 'dark' ? 'light' : 'dark');
    render();
  });

  // Refresh all
  root.querySelector('[data-action="refresh-all"]')?.addEventListener('click', async () => {
    await loadCatalogue();
    if (S.selectedService) await loadServiceDetail(S.selectedService);
  });

  // Service search
  const search = root.querySelector('#svc-search');
  if (search) {
    search.addEventListener('input', e => {
      S._svcSearch = e.target.value;
      const q = e.target.value.toLowerCase();
      root.querySelectorAll('.svc-card').forEach(card => {
        const name = card.dataset.select || '';
        card.style.display = !q || name.includes(q) ? '' : 'none';
      });
    });
  }

  // Service selection
  root.querySelectorAll('[data-select]').forEach(btn => {
    btn.addEventListener('click', async () => {
      const svc = btn.dataset.select;
      if (S.selectedService !== svc) {
        S.selectedService = svc;
        S.activeTab = 'overview';
        S.guided = { selectedFlowId: null, inputs: {}, captures: {}, running: false, cleaningUp: false, log: [] };
        S.rawResponse = null;
        render();
        await loadServiceDetail(svc);
      }
    });
  });

  // Tab switching
  root.querySelectorAll('[data-tab]').forEach(btn => {
    btn.addEventListener('click', () => {
      S.activeTab = btn.dataset.tab;
      render();
    });
  });

  // Flow sub-tab
  root.querySelectorAll('[data-flow]').forEach(btn => {
    btn.addEventListener('click', () => {
      S.guided.selectedFlowId = btn.dataset.flow;
      S.guided.log = [];
      S.guided.captures = {};
      render();
    });
  });

  // Guided inputs
  root.querySelectorAll('[data-input]').forEach(inp => {
    inp.addEventListener('input', e => {
      S.guided.inputs[e.target.dataset.input] = e.target.value;
    });
  });

  // Run guided
  root.querySelector('[data-action="run-guided"]')?.addEventListener('click', runGuided);

  // Clear guided
  root.querySelector('[data-action="clear-guided"]')?.addEventListener('click', () => {
    S.guided.log = [];
    S.guided.captures = {};
    render();
  });

  // Keep raw editor state synced so background renders don't discard unsent edits.
  root.querySelector('#raw-method')?.addEventListener('change', e => {
    S.raw.method = e.target.value;
  });
  root.querySelector('#raw-path')?.addEventListener('input', e => {
    S.raw.path = e.target.value;
  });
  root.querySelector('#raw-body')?.addEventListener('input', e => {
    S.raw.body = e.target.value;
  });

  // Run raw
  root.querySelector('[data-action="run-raw"]')?.addEventListener('click', runRaw);

  // Ops filter
  root.querySelector('#ops-search')?.addEventListener('input', e => {
    const input = e.target;
    const selStart = input.selectionStart;
    const selEnd = input.selectionEnd;
    S.opsFilter.query = input.value;
    render();
    const next = root.querySelector('#ops-search');
    if (next) {
      next.focus();
      if (selStart != null && selEnd != null) next.setSelectionRange(selStart, selEnd);
    }
  });
  root.querySelector('#ops-guided-only')?.addEventListener('change', e => {
    S.opsFilter.guidedOnly = e.target.checked;
    render();
  });

  // Fill raw from op
  root.querySelectorAll('[data-fill-raw]').forEach(btn => {
    btn.addEventListener('click', () => {
      const opName = btn.dataset.fillRaw;
      const ops = S.opsCatalog[S.selectedService] || [];
      const op = ops.find(o => o.name === opName);
      if (op) {
        S.raw.method = op.method;
        S.raw.path   = op.path;
        S.activeTab  = 'overview';
        render();
      }
    });
  });

  // Storage refresh
  root.querySelector('[data-action="refresh-storage"]')?.addEventListener('click', refreshStorage);

  // Retry detail load on error
  root.querySelector('[data-action="retry-detail"]')?.addEventListener('click', async () => {
    if (S.selectedService) {
      delete S.errors.detail;
      render();
      await loadServiceDetail(S.selectedService);
    }
  });

  // Tx filters
  root.querySelector('#tx-outcome-filter')?.addEventListener('change', e => {
    S.txFilter.outcome = e.target.value;
    refreshTransactions();
  });
  root.querySelector('#tx-guided-only')?.addEventListener('change', e => {
    S.txFilter.guidedOnly = e.target.checked;
    refreshTransactions();
  });
  root.querySelector('[data-action="refresh-tx"]')?.addEventListener('click', refreshTransactions);
  root.querySelector('[data-action="clear-tx"]')?.addEventListener('click', async () => {
    const scope = S.selectedService ? `/${encodeURIComponent(S.selectedService)}` : '';
    const r = await fetch(`/_localstack/studio-api/transactions${scope}`, { method: 'DELETE' });
    if (!r.ok) return;
    S.transactions = [];
    S.txSummary = { total: 0 };
    render();
  });

  // Replay tx in raw console
  root.querySelectorAll('[data-replay-tx]').forEach(btn => {
    btn.addEventListener('click', () => {
      const id = Number(btn.dataset.replayTx);
      const tx = S.transactions.find(t => t.id === id);
      if (tx) {
        S.raw.method = tx.method;
        S.raw.path   = tx.path;
        S.raw.body   = tx.request_body_preview || '';
        S.activeTab  = 'overview';
        render();
      }
    });
  });
}

// ── SigV4 signer (pure JS, no external deps) ──────────────────────────────
// Implements AWS Signature Version 4 so guided flows and raw requests can be
// sent as properly signed AWS API calls.  The gateway accepts unsigned requests
// too (falling back to DEFAULT_ACCESS_KEY), but signing is the correct path.

async function sha256Hex(message) {
  const data = typeof message === 'string' ? new TextEncoder().encode(message) : message;
  const hashBuffer = await crypto.subtle.digest('SHA-256', data);
  return Array.from(new Uint8Array(hashBuffer)).map(b => b.toString(16).padStart(2,'0')).join('');
}

async function hmacSha256(key, message) {
  const keyData = typeof key === 'string' ? new TextEncoder().encode(key) : key;
  const msgData = typeof message === 'string' ? new TextEncoder().encode(message) : message;
  const cryptoKey = await crypto.subtle.importKey('raw', keyData, { name:'HMAC', hash:'SHA-256' }, false, ['sign']);
  const sig = await crypto.subtle.sign('HMAC', cryptoKey, msgData);
  return new Uint8Array(sig);
}

async function hmacSha256Hex(key, message) {
  const bytes = await hmacSha256(key, message);
  return Array.from(bytes).map(b => b.toString(16).padStart(2,'0')).join('');
}

async function signRequest(method, url, service, body, creds, region) {
  const urlObj = new URL(url, window.location.origin);
  const host   = urlObj.hostname + (urlObj.port ? ':' + urlObj.port : '');
  const path   = urlObj.pathname || '/';
  const query  = urlObj.searchParams.toString();

  const now     = new Date();
  const amzDate = now.toISOString().replace(/[:-]|\.\d{3}/g, '').slice(0, 15) + 'Z';
  const dateStamp = amzDate.slice(0, 8);

  const bodyStr   = body || '';
  const bodyHash  = await sha256Hex(bodyStr);

  // Canonical headers (must be sorted, lowercase)
  const headers = {
    'host':          host,
    'x-amz-date':    amzDate,
    'x-amz-content-sha256': bodyHash,
  };
  if (creds.session_token) headers['x-amz-security-token'] = creds.session_token;
  const sortedHeaderNames = Object.keys(headers).sort();
  const canonicalHeaders  = sortedHeaderNames.map(k => `${k}:${headers[k]}\n`).join('');
  const signedHeaders     = sortedHeaderNames.join(';');

  const canonicalRequest = [
    method.toUpperCase(),
    path,
    query,
    canonicalHeaders,
    signedHeaders,
    bodyHash,
  ].join('\n');

  const credentialScope = `${dateStamp}/${region}/${service}/aws4_request`;
  const strToSign = [
    'AWS4-HMAC-SHA256',
    amzDate,
    credentialScope,
    await sha256Hex(canonicalRequest),
  ].join('\n');

  // Derive signing key
  const kDate    = await hmacSha256(`AWS4${creds.secret_access_key}`, dateStamp);
  const kRegion  = await hmacSha256(kDate, region);
  const kService = await hmacSha256(kRegion, service);
  const kSigning = await hmacSha256(kService, 'aws4_request');
  const signature = await hmacSha256Hex(kSigning, strToSign);

  const authHeader = [
    `AWS4-HMAC-SHA256 Credential=${creds.access_key_id}/${credentialScope}`,
    `SignedHeaders=${signedHeaders}`,
    `Signature=${signature}`,
  ].join(', ');

  return {
    headers: {
      ...headers,
      'Authorization': authHeader,
    },
  };
}

// Signed fetch — adds SigV4 headers when runtime config is available.
// `awsService` is the AWS service slug, e.g. "s3", "sqs".
async function signedFetch(awsService, path, opts = {}) {
  const cfg = S.runtimeConfig;
  if (!cfg) return fetch(path, opts);

  const body = opts.body || '';
  const method = (opts.method || 'GET').toUpperCase();
  const region = cfg.region || 'us-east-1';
  const creds  = cfg.credentials;

  try {
    const signed = await signRequest(method, path, awsService, body, creds, region);
    return fetch(path, {
      ...opts,
      method,
      headers: { ...(opts.headers || {}), ...signed.headers },
    });
  } catch (_) {
    // Signing failed (e.g. subtle crypto unavailable in non-HTTPS) — fall back unsigned
    return fetch(path, opts);
  }
}

// ── Polling engine ─────────────────────────────────────────────────────────
let _pollTimers = [];

function stopPolling() {
  _pollTimers.forEach(id => clearInterval(id));
  _pollTimers = [];
}

function startPolling() {
  stopPolling();
  const cfg = S.runtimeConfig?.polling || {};
  const storageMs = cfg.storage_interval_ms || 5000;
  const txMs      = cfg.transactions_interval_ms || 3000;

  // Storage poller — only runs when storage tab is active
  _pollTimers.push(setInterval(async () => {
    if (S.selectedService && S.activeTab === 'storage') {
      await refreshStorage();
    }
  }, storageMs));

  // Transaction poller — runs when transactions tab is active OR always in background
  _pollTimers.push(setInterval(async () => {
    if (S.selectedService) {
      await refreshTransactions();
    }
  }, txMs));
}

// ── Actions ────────────────────────────────────────────────────────────────
function getGuidedCaptures() {
  return { ...(S.guided.captures || {}) };
}

function resolveCaptureSource(source, responseBody, captures, inputs) {
  if (!source) return null;

  if (source.startsWith('inputs.')) {
    const key = source.slice('inputs.'.length);
    return inputs[key] ?? null;
  }

  if (source.startsWith('captures.')) {
    const key = source.slice('captures.'.length);
    return captures[key] ?? null;
  }

  // Try JSON body path first (supports dotted path like A.B.C)
  try {
    const parsed = JSON.parse(responseBody || '{}');
    const value = source.split('.').reduce((acc, seg) => (
      acc && typeof acc === 'object' ? acc[seg] : undefined
    ), parsed);
    if (value != null) return String(value);
  } catch (_) {
    // not JSON, continue
  }

  // Fallback to XML/query-like tag extraction (<Tag>value</Tag>)
  const safe = source.replace(/[.*+?^${}()|[\]\\]/g, '\\$&');
  const m = (responseBody || '').match(new RegExp(`<${safe}>([^<]+)</${safe}>`));
  if (m && m[1] != null) return m[1];

  return null;
}

function captureBindingsForStep(step, responseBody, currentCaptures) {
  const next = {};
  for (const binding of (step?.captures || [])) {
    const name = binding?.name;
    const source = binding?.source;
    if (!name || !source) continue;
    const value = resolveCaptureSource(source, responseBody, currentCaptures, S.guided.inputs || {});
    if (value != null && value !== '') {
      next[name] = value;
    }
  }
  return next;
}

function interpolateTemplate(value, captures) {
  if (!value) return value;
  let out = String(value);
  out = out.replace(/\{\{inputs\.([^}]+)\}\}/g, (_,k) => encodeURIComponent(S.guided.inputs[k] || k));
  out = out.replace(/\{\{captures\.([^}]+)\}\}/g, (_,k) => encodeURIComponent(captures[k] || k));
  return out;
}

async function runGuided() {
  const flows = S.flowDefinition?.flows || [];
  const flow  = flows.find(f => f.id === S.guided.selectedFlowId) || flows[0];
  if (!flow) return;

  S.guided.running = true;
  S.guided.log = [];
  S.guided.captures = {};
  render();

  for (const step of (flow.steps || [])) {
    let path = step.operation?.path || '/';
    const method = (step.operation?.method || 'GET').toUpperCase();

    const captures = getGuidedCaptures();
    path = interpolateTemplate(path, captures);

    let bodyStr = step.operation?.body;
    if (bodyStr) {
      // Preserve previous behavior for request bodies: raw value substitution,
      // no URL-encoding (JSON/query payloads expect plain token replacement).
      bodyStr = bodyStr
        .replace(/\{\{inputs\.([^}]+)\}\}/g, (_,k) => S.guided.inputs[k] || '')
        .replace(/\{\{captures\.([^}]+)\}\}/g, (_,k) => captures[k] || k);
    }

    const t0 = Date.now();
    let status = 0, body = '', success = false;
    try {
      // Derive AWS service slug from the selected service name for signing.
      // Studio requests go to the local gateway so we sign with the service slug.
      const r = await signedFetch(S.selectedService || 's3', path, {
        method,
        headers: { 'content-type': 'application/json' },
        body: bodyStr,
      });
      status = r.status;
      body   = await r.text();
      // Evaluate assertions. If none are declared, require HTTP success.
      const assertions = step.assertions || [];
      success = assertions.length === 0 ? r.ok : assertions.every(a => {
        if (a.kind === 'status') return String(status) === String(a.expected);
        return true;
      });
    } catch (e) {
      body = e.message;
    }
    const duration_ms = Date.now() - t0;

    // Record to transaction log (fire-and-forget)
    postJson('/_localstack/studio-api/transactions/record', {
      service:   S.selectedService,
      operation: step.id,
      method,
      path,
      status,
      responseBodyPreview: body.slice(0,512),
      startedAtMs: t0,
      durationMs: duration_ms,
      fromGuidedFlow: true,
    }).catch(() => {});

    const stepCaptures = captureBindingsForStep(step, body, captures);
    S.guided.captures = { ...(S.guided.captures || {}), ...stepCaptures };
    S.guided.log.push({
      stepId: step.id,
      title: step.title,
      status,
      body,
      success,
      duration_ms,
      captures: stepCaptures,
    });
    render();
    if (!success) break;
  }

  // Run cleanup steps regardless of outcome (resource hygiene).
  // Cleanup failures are recorded but don't affect the overall result display.
  const cleanupSteps = flow.cleanup || [];
  if (cleanupSteps.length > 0) {
    S.guided.cleaningUp = true;
    render();
    for (const step of cleanupSteps) {
      const captures = getGuidedCaptures();
      let path = step.operation?.path || '/';
      const method = (step.operation?.method || 'DELETE').toUpperCase();
      path = interpolateTemplate(path, captures);
      let bodyStr = step.operation?.body;
      if (bodyStr) {
        bodyStr = bodyStr
          .replace(/\{\{inputs\.([^}]+)\}\}/g, (_,k) => S.guided.inputs[k] || '')
          .replace(/\{\{captures\.([^}]+)\}\}/g, (_,k) => captures[k] || k);
      }
      const t0 = Date.now();
      try {
        const r = await signedFetch(S.selectedService || 's3', path, { method, body: bodyStr });
        const body = await r.text();
        const duration_ms = Date.now() - t0;
        postJson('/_localstack/studio-api/transactions/record', {
          service: S.selectedService, operation: `cleanup:${step.id}`,
          method, path, status: r.status, durationMs: duration_ms,
          startedAtMs: t0, fromGuidedFlow: true,
        }).catch(() => {});
        S.guided.log.push({ stepId: `cleanup:${step.id}`, title: `[cleanup] ${step.title}`, status: r.status, body, success: r.ok, duration_ms, isCleanup: true });
      } catch (e) {
        S.guided.log.push({ stepId: `cleanup:${step.id}`, title: `[cleanup] ${step.title}`, status: 0, body: e.message, success: false, duration_ms: 0, isCleanup: true });
      }
      render();
    }
    S.guided.cleaningUp = false;
  }

  S.guided.running = false;
  render();
}

async function runRaw() {
  const method  = root.querySelector('#raw-method')?.value || S.raw.method;
  const path    = root.querySelector('#raw-path')?.value   || S.raw.path;
  const bodyStr = root.querySelector('#raw-body')?.value   || '';
  S.raw = { method, path, body: bodyStr };

  const t0 = Date.now();
  try {
    const opts = {
      method: method.toUpperCase(),
      headers: { 'x-openstack-studio-origin': '1' },
    };
    if (bodyStr) opts.body = bodyStr;
    // Sign the raw request against the selected service (or a generic slug).
    const r = await signedFetch(S.selectedService || 'execute-api', path, opts);
    const text = await r.text();
    let pretty = text;
    try { pretty = JSON.stringify(JSON.parse(text), null, 2); } catch (_) {}
    S.rawResponse = { status: r.status, body: pretty };

    // Record to transaction log
    postJson('/_localstack/studio-api/transactions/record', {
      service: S.selectedService || 'raw',
      method: method.toUpperCase(),
      path,
      status: r.status,
      requestBodyPreview: bodyStr.slice(0,256),
      responseBodyPreview: text.slice(0,512),
      startedAtMs: t0,
      durationMs: Date.now() - t0,
      fromGuidedFlow: false,
    }).catch(() => {});
  } catch (e) {
    S.rawResponse = { status: 0, body: `Error: ${e.message}` };
  }
  render();
}

// ── Bootstrap ──────────────────────────────────────────────────────────────
setTheme(S.theme);

// Load runtime config first (credentials + polling intervals), then catalogue.
(async () => {
  try {
    const cfg = await api('/_localstack/studio-api/runtime-config');
    S.runtimeConfig = cfg;
    // Start polling as soon as we have the config (intervals may be non-default)
    startPolling();
  } catch (_) {
    // runtime-config unavailable — use defaults and still poll
    S.runtimeConfig = {
      endpoint: '',
      credentials: { access_key_id: 'test', secret_access_key: 'test', session_token: null },
      region: 'us-east-1',
      polling: { storage_interval_ms: 5000, transactions_interval_ms: 3000 },
    };
    startPolling();
  }
  await loadCatalogue();
})();

})();
"#;
const STUDIO_ASSET_CSS: &str = r#"*,::before,::after{box-sizing:border-box;margin:0;padding:0}
:root{color-scheme:light dark;
  --bg:#0d1117;--bg2:#161b22;--bg3:#21262d;--border:#30363d;
  --fg:#e6edf3;--fg2:#8b949e;--fg3:#6e7681;
  --accent:#2f81f7;--accent-hover:#388bfd;
  --ok:#3fb950;--warn:#d29922;--err:#f85149;--pending:#8b949e;
  --radius:8px;--radius-sm:5px;
  --shadow:0 1px 3px rgba(0,0,0,.4);
  --font: -apple-system,BlinkMacSystemFont,"Segoe UI",Helvetica,Arial,sans-serif;
}
[data-theme="light"]{
  --bg:#f6f8fa;--bg2:#ffffff;--bg3:#f0f2f5;--border:#d0d7de;
  --fg:#1f2328;--fg2:#636c76;--fg3:#8c959f;
  --shadow:0 1px 3px rgba(0,0,0,.1);
}
body{font-family:var(--font);background:var(--bg);color:var(--fg);font-size:13px;line-height:1.5;height:100vh;overflow:hidden}
.app{display:flex;flex-direction:column;height:100vh}

/* ── topbar ──────────────────────────────────────────────────────────── */
.topbar{display:flex;align-items:center;justify-content:space-between;
  padding:0 16px;height:48px;background:var(--bg2);border-bottom:1px solid var(--border);
  flex-shrink:0;gap:12px}
.topbar-brand{display:flex;align-items:center;gap:8px}
.topbar-title{font-weight:600;font-size:14px}
.topbar-actions{display:flex;gap:6px}
.icon-btn{background:none;border:1px solid transparent;color:var(--fg2);border-radius:var(--radius-sm);
  padding:5px;cursor:pointer;display:flex;align-items:center}
.icon-btn:hover{background:var(--bg3);border-color:var(--border);color:var(--fg)}

/* ── workspace ───────────────────────────────────────────────────────── */
.workspace{display:flex;flex:1;overflow:hidden}

/* ── sidebar ─────────────────────────────────────────────────────────── */
.sidebar{width:220px;flex-shrink:0;background:var(--bg2);border-right:1px solid var(--border);
  display:flex;flex-direction:column;overflow:hidden}
.sidebar-search{padding:10px 10px 6px}
.sidebar-list{overflow-y:auto;flex:1;padding:0 6px 10px}
.sidebar-loading,.sidebar-error,.sidebar-empty{padding:16px 10px;color:var(--fg2);font-size:12px}
.sidebar-error{color:var(--err)}
.search-input{width:100%;background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:5px 8px;font-size:12px;font-family:inherit}
.search-input:focus{outline:none;border-color:var(--accent)}

.svc-card{width:100%;text-align:left;background:none;border:1px solid transparent;
  border-radius:var(--radius-sm);padding:7px 8px;cursor:pointer;color:var(--fg);margin-bottom:2px}
.svc-card:hover{background:var(--bg3);border-color:var(--border)}
.svc-card--active{background:var(--bg3);border-color:var(--accent)}
.svc-card-row{display:flex;align-items:center;gap:5px;font-size:12px;font-weight:600}
.svc-card-meta{font-size:11px;color:var(--fg2);margin-top:2px;padding-left:14px}

/* ── main area ───────────────────────────────────────────────────────── */
.main{flex:1;overflow-y:auto;display:flex;flex-direction:column}

/* ── welcome ─────────────────────────────────────────────────────────── */
.welcome{display:flex;flex-direction:column;align-items:center;justify-content:center;
  flex:1;gap:24px;padding:32px}
.welcome-hero{text-align:center;display:flex;flex-direction:column;align-items:center;gap:12px}
.welcome-hero h1{font-size:22px;font-weight:600}
.welcome-hero p{color:var(--fg2);max-width:420px;font-size:13px}
.stat-row{display:flex;gap:12px}
.stat-card{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);
  padding:16px 24px;text-align:center;min-width:100px}
.stat-num{font-size:28px;font-weight:700;color:var(--accent)}
.stat-label{font-size:11px;color:var(--fg2);margin-top:4px}

/* ── explorer ────────────────────────────────────────────────────────── */
.explorer{display:flex;flex-direction:column;flex:1;overflow:hidden}
.explorer-header{padding:12px 16px 0;background:var(--bg2);border-bottom:1px solid var(--border);flex-shrink:0}
.explorer-title{display:flex;align-items:baseline;gap:10px;margin-bottom:10px}
.explorer-title h2{font-size:16px;font-weight:600}
.explorer-meta{font-size:12px;color:var(--fg2);display:flex;align-items:center;gap:4px}
.explorer-body{flex:1;overflow-y:auto;padding:16px}

/* tabs */
.tab-bar{display:flex;gap:2px}
.tab{background:none;border:none;border-bottom:2px solid transparent;color:var(--fg2);
  padding:6px 12px 8px;cursor:pointer;font-size:12px;font-family:inherit;display:flex;align-items:center;gap:5px}
.tab:hover{color:var(--fg)}
.tab--active{color:var(--fg);border-bottom-color:var(--accent)}
.tab-badge{background:var(--bg3);border:1px solid var(--border);border-radius:10px;
  padding:0 6px;font-size:10px;line-height:16px}

/* ── chips ───────────────────────────────────────────────────────────── */
.chip{display:inline-flex;align-items:center;border-radius:10px;padding:1px 7px;
  font-size:10px;font-weight:500;line-height:16px;background:var(--bg3);border:1px solid var(--border)}
.chip-ok{background:#1c3a20;border-color:#3fb950;color:#3fb950}
.chip-live{background:#1a2d18;border-color:#3fb950;color:#3fb950;animation:pulse 2s infinite}
@keyframes pulse{0%,100%{opacity:1}50%{opacity:.6}}
.chip-guided{background:#1a2d4a;border-color:var(--accent);color:var(--accent)}
.chip-raw{background:var(--bg3);border-color:var(--border);color:var(--fg2)}
.chip-level{background:#2a2016;border-color:var(--warn);color:var(--warn)}
.dot{width:7px;height:7px;border-radius:50%;flex-shrink:0;display:inline-block}
.dot-running{background:var(--ok)}.dot-starting{background:var(--warn)}
.dot-stopping,.dot-stopped{background:var(--fg3)}.dot-error{background:var(--err)}
.dot-available{background:var(--fg2)}

/* ── method badges ───────────────────────────────────────────────────── */
.method-badge{display:inline-block;border-radius:var(--radius-sm);padding:1px 6px;font-size:10px;font-weight:700;
  font-family:ui-monospace,SFMono-Regular,Menlo,monospace;min-width:44px;text-align:center}
.method-get{background:#1c3a20;color:#3fb950}.method-post{background:#1a2d4a;color:#2f81f7}
.method-put{background:#2a2016;color:#d29922}.method-del{background:#3a1a1a;color:#f85149}
.method-patch{background:#261a3a;color:#bc8cff}.method-head{background:#1a2a2a;color:#39d353}
.method-other{background:var(--bg3);color:var(--fg2)}

/* ── status badges ───────────────────────────────────────────────────── */
.status-badge{display:inline-block;border-radius:var(--radius-sm);padding:1px 7px;font-size:11px;font-weight:600;
  font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.status-ok{background:#1c3a20;color:#3fb950}.status-warn{background:#2d2a10;color:#d29922}
.status-err{background:#3a1a1a;color:#f85149}.status-pending{background:var(--bg3);color:var(--fg2)}

/* ── buttons ─────────────────────────────────────────────────────────── */
.btn{border-radius:var(--radius-sm);padding:5px 12px;font-size:12px;font-family:inherit;cursor:pointer;
  border:1px solid var(--border);background:var(--bg3);color:var(--fg);transition:background .1s}
.btn:hover{background:var(--border)}
.btn:disabled{opacity:.5;cursor:not-allowed}
.btn-primary{background:var(--accent);border-color:var(--accent);color:#fff}
.btn-primary:hover:not(:disabled){background:var(--accent-hover);border-color:var(--accent-hover)}
.btn-primary.btn-loading{opacity:.7}
.btn-ghost{background:none;border-color:transparent;color:var(--fg2)}
.btn-ghost:hover{background:var(--bg3);border-color:var(--border);color:var(--fg)}
.btn-danger{color:var(--err)}
.btn-tiny{background:none;border:none;color:var(--fg3);cursor:pointer;padding:2px 5px;
  border-radius:3px;font-size:11px}
.btn-tiny:hover{background:var(--bg3);color:var(--fg)}

/* ── panels & sections ───────────────────────────────────────────────── */
.panel-section{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);
  padding:14px 16px;margin-bottom:14px}
.panel-section-title{font-size:11px;font-weight:600;color:var(--fg2);text-transform:uppercase;
  letter-spacing:.06em;margin-bottom:10px}
.panel-loading{padding:32px;text-align:center;color:var(--fg2)}
.tab-error{padding:16px;background:#1e0a0a;border:1px solid var(--err);border-radius:var(--radius);
  color:var(--err);display:flex;align-items:center;gap:10px;margin-bottom:12px}
.empty-state{color:var(--fg2);font-size:12px;padding:12px 0}

/* ── overview ────────────────────────────────────────────────────────── */
.overview-grid{display:grid;grid-template-columns:repeat(auto-fill,minmax(160px,1fr));gap:10px;margin-bottom:14px}
.ov-card{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius);padding:10px 14px}
.ov-label{font-size:10px;color:var(--fg2);text-transform:uppercase;letter-spacing:.05em;margin-bottom:4px}
.ov-value{font-size:14px;font-weight:600;display:flex;align-items:center;gap:5px}
.ov-num{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:18px;color:var(--accent)}
.ov-ok{color:var(--ok)}.ov-err{color:var(--err)}

/* ── guided ──────────────────────────────────────────────────────────── */
.sub-tabs{display:flex;gap:4px;margin-bottom:10px;flex-wrap:wrap}
.sub-tab{background:var(--bg3);border:1px solid var(--border);border-radius:var(--radius-sm);
  padding:3px 10px;font-size:11px;cursor:pointer;color:var(--fg2);font-family:inherit;display:flex;align-items:center;gap:4px}
.sub-tab:hover{border-color:var(--accent);color:var(--fg)}
.sub-tab--active{background:var(--bg3);border-color:var(--accent);color:var(--fg)}
.guided-inputs{margin-bottom:10px}
.inputs-title{font-size:11px;color:var(--fg2);margin-bottom:6px}
.field-row{margin-bottom:8px}
.field-label{font-size:12px;font-weight:500;margin-bottom:3px;display:block}
.req{color:var(--err)}
.field-desc{font-size:11px;color:var(--fg2);margin-bottom:3px}
.field-input{width:100%;background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:5px 8px;font-size:12px;font-family:inherit}
.field-input:focus{outline:none;border-color:var(--accent)}
.guided-steps{margin-bottom:10px}
.step-row{display:flex;gap:10px;padding:8px;border-radius:var(--radius-sm);margin-bottom:6px;
  border:1px solid var(--border)}
.step-ok{border-color:var(--ok);background:#0f2010}
.step-err{border-color:var(--err);background:#200f0f}
.step-cleanup-ok{border-color:var(--fg3);background:var(--bg);opacity:.7}
.step-cleanup-err{border-color:var(--warn);background:#1a1200;opacity:.8}
.step-num{font-size:10px;color:var(--fg2);min-width:16px;padding-top:2px}
.step-body{flex:1;min-width:0}
.step-title{font-size:12px;font-weight:600;margin-bottom:3px}
.step-op{display:flex;align-items:center;gap:6px;font-size:11px}
.step-op code{color:var(--fg2);font-family:ui-monospace,SFMono-Regular,Menlo,monospace;word-break:break-all}
.step-result{margin-top:6px;display:flex;align-items:center;gap:6px;flex-wrap:wrap}
.step-dur{font-size:11px;color:var(--fg2)}
.step-body-preview{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:10px;
  background:var(--bg);border-radius:4px;padding:6px 8px;margin-top:4px;
  max-height:120px;overflow-y:auto;word-break:break-all;width:100%;white-space:pre-wrap;color:var(--fg2)}
.step-guidance{font-size:11px;color:var(--warn);margin-top:4px;padding:4px 6px;
  background:#1e1800;border-radius:4px;border:1px solid #3d3000}
.guided-actions{display:flex;gap:8px;align-items:center}

/* ── raw console ─────────────────────────────────────────────────────── */
.raw-form{display:flex;flex-direction:column;gap:8px}
.raw-row{display:flex;gap:6px}
.raw-method{background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:5px 8px;font-size:12px;font-family:inherit;width:90px}
.raw-path{flex:1;background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:5px 8px;font-size:12px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.raw-path:focus,.raw-method:focus{outline:none;border-color:var(--accent)}
.raw-body{width:100%;background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:6px 8px;font-size:11px;resize:vertical;
  font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.raw-body:focus{outline:none;border-color:var(--accent)}
.raw-response{margin-top:10px;display:flex;flex-direction:column;gap:6px}
.resp-body{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:11px;
  background:var(--bg);border:1px solid var(--border);border-radius:var(--radius-sm);
  padding:8px;max-height:240px;overflow-y:auto;white-space:pre-wrap;word-break:break-all}

/* ── operations tab ──────────────────────────────────────────────────── */
.ops-toolbar{display:flex;align-items:center;gap:10px;margin-bottom:10px;flex-wrap:wrap}
.ops-stats{font-size:11px;color:var(--fg2);margin-left:auto}
.toggle-label{display:flex;align-items:center;gap:5px;font-size:12px;color:var(--fg2);cursor:pointer;white-space:nowrap}
.toggle-label input{accent-color:var(--accent)}
.op-list{display:flex;flex-direction:column;gap:2px}
.op-row{display:flex;align-items:center;gap:8px;padding:6px 10px;border-radius:var(--radius-sm);
  border:1px solid transparent;background:var(--bg2)}
.op-row:hover{border-color:var(--border)}
.op-left{display:flex;align-items:center;gap:6px;min-width:260px;flex-shrink:0}
.op-name{font-size:12px;font-weight:500}
.op-path{font-size:11px;color:var(--fg2);flex:1;word-break:break-all;
  font-family:ui-monospace,SFMono-Regular,Menlo,monospace}

/* ── storage tab ─────────────────────────────────────────────────────── */
.storage-toolbar{margin-bottom:12px}
.storage-section{margin-bottom:16px}
.storage-section-title{font-size:11px;font-weight:600;text-transform:uppercase;letter-spacing:.05em;
  color:var(--fg2);margin-bottom:8px;display:flex;align-items:center;gap:6px}
.resource-list{display:flex;flex-direction:column;gap:4px}
.resource-row{background:var(--bg2);border:1px solid var(--border);border-radius:var(--radius-sm);
  padding:8px 12px;display:flex;align-items:flex-start;gap:12px;flex-wrap:wrap}
.resource-id{font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:11px;
  font-weight:600;min-width:160px;word-break:break-all}
.resource-ts{font-size:10px;color:var(--fg3);white-space:nowrap}
.resource-attrs{display:flex;flex-wrap:wrap;gap:6px;flex:1}
.attr-pair{display:inline-flex;align-items:center;gap:3px;font-size:10px;
  background:var(--bg3);border:1px solid var(--border);border-radius:3px;padding:1px 6px}
.attr-key{color:var(--fg2)}.attr-val{color:var(--fg);font-weight:500}

/* ── transactions tab ────────────────────────────────────────────────── */
.tx-toolbar{margin-bottom:10px}
.tx-summary{display:flex;align-items:center;gap:12px;margin-bottom:8px;flex-wrap:wrap}
.tx-stat{font-size:12px;color:var(--fg2)}
.tx-stat strong{color:var(--fg)}
.tx-filters{display:flex;align-items:center;gap:8px;flex-wrap:wrap}
.filter-select{background:var(--bg3);border:1px solid var(--border);color:var(--fg);
  border-radius:var(--radius-sm);padding:4px 8px;font-size:12px;font-family:inherit}
.filter-select:focus{outline:none;border-color:var(--accent)}
.tx-list{display:flex;flex-direction:column;gap:2px}
.tx-row{display:flex;align-items:center;gap:8px;padding:6px 10px;border-radius:var(--radius-sm);
  border:1px solid transparent;background:var(--bg2);flex-wrap:wrap}
.tx-row:hover{border-color:var(--border)}
.outcome-ok{border-left:2px solid var(--ok)}
.outcome-warn{border-left:2px solid var(--warn)}
.outcome-err{border-left:2px solid var(--err)}
.outcome-pending{border-left:2px solid var(--fg3)}
.tx-id{font-size:10px;color:var(--fg3);min-width:30px;font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.tx-path{font-size:11px;color:var(--fg2);flex:1;word-break:break-all;
  font-family:ui-monospace,SFMono-Regular,Menlo,monospace;min-width:120px}
.tx-op{font-size:10px;color:var(--fg2);font-family:ui-monospace,SFMono-Regular,Menlo,monospace}
.tx-dur{font-size:11px;color:var(--fg3);white-space:nowrap}

/* ── outcome colour helpers ──────────────────────────────────────────── */
.outcome-ok .tx-path,.outcome-ok .tx-id{color:inherit}

/* ── scrollbars ──────────────────────────────────────────────────────── */
::-webkit-scrollbar{width:6px;height:6px}
::-webkit-scrollbar-track{background:var(--bg)}
::-webkit-scrollbar-thumb{background:var(--bg3);border-radius:3px}
::-webkit-scrollbar-thumb:hover{background:var(--border)}

/* ── responsive ──────────────────────────────────────────────────────── */
@media(max-width:768px){
  .sidebar{width:180px}
  .overview-grid{grid-template-columns:repeat(2,1fr)}
  .op-left{min-width:180px}
}"#;
const STUDIO_GUIDED_MAX_PAYLOAD_BYTES: usize = 256 * 1024;

// Thread-local fast RNG — seeded once per thread from the OS RNG, avoiding a
// getrandom syscall on every request.
thread_local! {
    static FAST_RNG: RefCell<SmallRng> = RefCell::new(SmallRng::from_rng(&mut rand::rng()));
}

/// Generate a UUID v4-formatted request ID using the thread-local fast RNG.
///
/// ~10x faster than `Uuid::new_v4()` which hits the kernel CSPRNG on every call.
fn fast_request_id() -> String {
    FAST_RNG.with(|rng| {
        let mut rng = rng.borrow_mut();
        let mut b = [0u8; 16];
        rng.fill(&mut b);
        // Set UUID v4 version and variant bits for RFC 4122 compliance.
        b[6] = (b[6] & 0x0f) | 0x40;
        b[8] = (b[8] & 0x3f) | 0x80;
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut out = [0u8; 36];
        let mut o = 0;
        for (i, &byte) in b.iter().enumerate() {
            if i == 4 || i == 6 || i == 8 || i == 10 {
                out[o] = b'-';
                o += 1;
            }
            out[o] = HEX[(byte >> 4) as usize];
            o += 1;
            out[o] = HEX[(byte & 0x0f) as usize];
            o += 1;
        }
        // SAFETY: HEX table and '-' are all valid ASCII/UTF-8.
        unsafe { String::from_utf8_unchecked(out.to_vec()) }
    })
}

/// Adapter that converts `http_body_util::BodyStream<axum::body::Body>` into
/// a `futures_core::Stream<Item = Result<Bytes, io::Error>>` suitable for
/// `SpooledBody::write_from_stream()`.
struct BodyStreamAdapter {
    inner: BodyStream<Body>,
    bytes_read: usize,
    max_bytes: Option<usize>,
}

impl BodyStreamAdapter {
    fn new(body: Body, max_bytes: Option<usize>) -> Self {
        Self {
            inner: BodyStream::new(body),
            bytes_read: 0,
            max_bytes,
        }
    }
}

impl futures_core::Stream for BodyStreamAdapter {
    type Item = Result<Bytes, io::Error>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.inner).poll_next(cx) {
                Poll::Ready(Some(Ok(frame))) => {
                    if let Ok(data) = frame.into_data() {
                        self.bytes_read = self.bytes_read.saturating_add(data.len());
                        if self.max_bytes.is_some_and(|limit| self.bytes_read > limit) {
                            return Poll::Ready(Some(Err(io::Error::new(
                                io::ErrorKind::InvalidData,
                                "guided execution payload exceeds configured limit",
                            ))));
                        }
                        return Poll::Ready(Some(Ok(data)));
                    }
                    // Skip non-data frames (trailers, etc.)
                    continue;
                }
                Poll::Ready(Some(Err(e))) => return Poll::Ready(Some(Err(io::Error::other(e)))),
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

/// The main HTTP gateway for openstack.
pub struct Gateway {
    config: Config,
    plugin_manager: ServicePluginManager,
    shutdown_tx: tokio::sync::broadcast::Sender<()>,
    /// Whether the Studio UI subsystem (TX log, operation catalog) is active.
    studio_enabled: bool,
}

/// Shared application state passed to all axum handlers.
#[derive(Clone)]
struct AppState {
    config: Config,
    plugin_manager: ServicePluginManager,
    cors: Arc<CorsHandler>,
    internal_api_router: Router,
    studio_enabled: bool,
}

impl Gateway {
    pub fn new(config: Config, plugin_manager: ServicePluginManager) -> Self {
        let (shutdown_tx, _) = tokio::sync::broadcast::channel(16);
        // Inherit studio mode from env var or debug flag at construction time.
        let studio_enabled =
            config.debug || std::env::var("STUDIO").is_ok_and(|v| v == "1" || v == "true");
        Self {
            config,
            plugin_manager,
            shutdown_tx,
            studio_enabled,
        }
    }

    /// Construct a Studio-enabled gateway explicitly (e.g. from `openstack start --studio`).
    pub fn new_with_studio(config: Config, plugin_manager: ServicePluginManager) -> Self {
        let mut gw = Self::new(config, plugin_manager);
        gw.studio_enabled = true;
        gw
    }

    /// Returns whether the Studio UI subsystem is active for this gateway instance.
    pub fn studio_enabled(&self) -> bool {
        self.studio_enabled
    }

    /// Build the axum Router for this gateway (useful for testing).
    fn build_app(&self) -> Router {
        let cors = Arc::new(CorsHandler::new(&self.config));
        let internal_state = openstack_internal_api::ApiState::new_with_studio(
            self.config.clone(),
            self.plugin_manager.clone(),
            self.shutdown_tx.clone(),
            self.studio_enabled,
        );
        let internal_api_router = openstack_internal_api::internal_api_router(internal_state);
        let app_state = AppState {
            config: self.config.clone(),
            plugin_manager: self.plugin_manager.clone(),
            cors,
            internal_api_router,
            studio_enabled: self.studio_enabled,
        };
        Router::new()
            .fallback(handle_request)
            .layer(ServiceBuilder::new().layer(CompressionLayer::new()))
            .with_state(app_state)
    }

    #[doc(hidden)]
    pub fn build_app_for_tests(&self) -> Router {
        self.build_app()
    }

    /// Run the gateway using a pre-bound listener and an external shutdown signal.
    /// Useful for integration tests where you need to control port allocation.
    pub async fn run_with_listener(
        self,
        listener: tokio::net::TcpListener,
        mut shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> Result<(), anyhow::Error> {
        if self.config.eager_service_loading {
            info!("Eagerly starting all services...");
            self.plugin_manager.start_all().await;
        }
        let app = self.build_app();
        let mut api_shutdown = self.shutdown_tx.subscribe();
        tokio::select! {
            result = axum::serve(listener, app) => { result?; }
            _ = &mut shutdown => {
                info!("Shutdown signal received");
                self.plugin_manager.stop_all().await;
            }
            _ = api_shutdown.recv() => {
                info!("Shutdown requested via internal API");
                self.plugin_manager.stop_all().await;
            }
        }
        Ok(())
    }

    pub async fn run(self, state_manager: StateManager) -> Result<(), anyhow::Error> {
        let config = self.config.clone();

        // Eager service loading if configured
        if config.eager_service_loading {
            info!("Eagerly starting all services...");
            self.plugin_manager.start_all().await;
        }

        let app = self.build_app();
        let mut api_shutdown = self.shutdown_tx.subscribe();

        // Bind to all configured addresses
        let addrs = config.gateway_listen.clone();
        let mut handles = Vec::new();

        for addr in addrs {
            let app_clone = app.clone();
            let addr_str = addr.to_string();
            let listener = tokio::net::TcpListener::bind(addr).await?;
            info!("Gateway listening on {}", addr_str);

            let handle = tokio::spawn(async move { axum::serve(listener, app_clone).await });
            handles.push(handle);
        }

        // Wait for shutdown signal
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {
                info!("Shutdown signal received");
            }
            _ = api_shutdown.recv() => {
                info!("Shutdown requested via internal API");
            }
        }

        // Save state on shutdown
        state_manager.save_on_shutdown().await?;

        // Stop all services
        self.plugin_manager.stop_all().await;

        Ok(())
    }
}

/// The main request handler - processes all incoming AWS API requests.
async fn handle_request(
    State(state): State<AppState>,
    method: Method,
    headers: HeaderMap,
    req: axum::extract::Request,
) -> Response {
    let request_start = std::time::Instant::now();

    // Fast-path early exits: check cheap conditions BEFORE allocating a
    // request ID, owned path string, or parsing query parameters.

    // Handle CORS preflight — only needs method + headers, zero allocs.
    if CorsHandler::is_preflight(&method, &headers) {
        let mut resp_headers = HeaderMap::new();
        state.cors.add_cors_headers(
            &mut resp_headers,
            headers.get("origin").and_then(|v| v.to_str().ok()),
        );
        let mut response = StatusCode::OK.into_response();
        *response.headers_mut() = resp_headers;
        return response;
    }

    // Studio SPA/asset routes and internal API routes are resolved before
    // any allocation.  `uri.path()` returns a `&str` into the URI — no
    // heap allocation until we actually need an owned `String`.
    let uri = req.uri().clone();
    let path_str = uri.path();

    if is_studio_asset_route(path_str) {
        return studio_asset_response(path_str);
    }

    if is_studio_spa_route(path_str) {
        return studio_spa_response();
    }

    // Internal API routes (/_localstack/*) — allocate path + query only here.
    if path_str.starts_with("/_localstack/") {
        // Check studio guided execution payload size before reading body.
        if is_studio_guided_execution_route(path_str)
            && headers
                .get(axum::http::header::CONTENT_LENGTH)
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<usize>().ok())
                .is_some_and(|len| len > STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
        {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "guided execution payload exceeds configured limit",
            )
                .into_response();
        }

        let guided_limit = if is_studio_guided_execution_route(path_str) {
            Some(STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
        } else {
            None
        };

        // Stream and buffer the body for internal API dispatch.
        let threshold = state.config.body_spool_threshold_bytes;
        let mut spooled = SpooledBody::new(threshold);
        let stream = BodyStreamAdapter::new(req.into_body(), guided_limit);
        if let Err(e) = spooled.write_from_stream(stream).await {
            if e.kind() == io::ErrorKind::InvalidData {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "guided execution payload exceeds configured limit",
                )
                    .into_response();
            }
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
        let body_bytes = match spooled.into_bytes() {
            Ok(b) => b,
            Err(e) => {
                error!("Failed to materialize request body: {}", e);
                return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
            }
        };

        let path = path_str.to_string();
        let query_string = uri.query().unwrap_or("");
        let query_params: HashMap<String, String> = if query_string.is_empty() {
            HashMap::new()
        } else {
            serde_urlencoded::from_str(query_string).unwrap_or_default()
        };

        return handle_internal_api(path, &method, &headers, &query_params, &body_bytes, &state)
            .await;
    }

    // --- AWS service request path ---
    // Only now do we allocate request ID, owned path, and parse query params.
    let request_id = fast_request_id();
    let path = path_str.to_string();
    let query_string = uri.query().unwrap_or("");

    // Parse query parameters
    let query_params: HashMap<String, String> = if query_string.is_empty() {
        HashMap::new()
    } else {
        serde_urlencoded::from_str(query_string).unwrap_or_default()
    };

    // Check studio guided execution payload size (for any studio route that
    // slipped through the /_localstack/ prefix check above — shouldn't happen
    // in practice, but kept as a safety net).
    if is_studio_guided_execution_route(&path)
        && headers
            .get(axum::http::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<usize>().ok())
            .is_some_and(|len| len > STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
    {
        return (
            StatusCode::PAYLOAD_TOO_LARGE,
            "guided execution payload exceeds configured limit",
        )
            .into_response();
    }

    let guided_limit = if is_studio_guided_execution_route(&path) {
        Some(STUDIO_GUIDED_MAX_PAYLOAD_BYTES)
    } else {
        None
    };

    // Determine whether this is an S3 object-body request BEFORE reading the
    // body.  For virtual-hosted-style S3 requests we normalize path first so
    // classification sees /bucket/key rather than /key.
    let path_for_body_check = if should_rewrite_s3_vhost_for_body_check(&headers, &query_params) {
        rewrite_s3_virtual_hosted_path(&path, &headers, &state.config.localstack_host)
    } else {
        path.clone()
    };

    // For S3 PutObject / UploadPart we bypass the intermediate SpooledBody
    // disk spool entirely: the raw axum body stream is wrapped in a
    // `StreamReader` and passed directly to the S3 provider so it can write
    // object data to persistent storage in a single pass.
    let is_s3_body =
        is_s3_object_body_request(&method, &path_for_body_check, &headers, &query_params);

    let (body_bytes, body_reader): (Bytes, Option<BodyReader>) = if is_s3_body {
        // S3 object-body path: wrap the axum body as an AsyncRead and pass it
        // through to the S3 provider.  The parsers only need an empty slice
        // (S3 PutObject has no XML/JSON protocol body to parse).
        let stream = BodyStreamAdapter::new(req.into_body(), guided_limit);
        let reader: BodyReader = Box::new(StreamReader::new(stream));
        (Bytes::new(), Some(reader))
    } else {
        // Non-S3 path (or S3 sub-resource operations like CompleteMultipart):
        // spool the body into memory/disk, then materialise as Bytes for
        // protocol parsing.  No provider other than S3 reads `body_reader`.
        let threshold = state.config.body_spool_threshold_bytes;
        let mut spooled = SpooledBody::new(threshold);
        let stream = BodyStreamAdapter::new(req.into_body(), guided_limit);
        if let Err(e) = spooled.write_from_stream(stream).await {
            if e.kind() == io::ErrorKind::InvalidData {
                return (
                    StatusCode::PAYLOAD_TOO_LARGE,
                    "guided execution payload exceeds configured limit",
                )
                    .into_response();
            }
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
        }
        match spooled.into_bytes() {
            Ok(b) => (b, None),
            Err(e) => {
                error!("Failed to materialize request body: {}", e);
                return (StatusCode::BAD_REQUEST, "Failed to read request body").into_response();
            }
        }
    };

    // Extract the origin header before consuming headers (needed for CORS later).
    let origin_header: Option<String> = headers
        .get("origin")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    // Marker used to suppress duplicate Studio-origin transaction recording.
    let is_studio_origin = headers
        .get("x-openstack-studio-origin")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == "1" || v.eq_ignore_ascii_case("true"));

    // Build request context
    let context_start = std::time::Instant::now();
    let ctx = match build_request_context(
        &method,
        path,
        query_params,
        headers,
        &body_bytes,
        &request_id,
        &state.config,
        body_reader,
    ) {
        Ok(ctx) => ctx,
        Err(resp) => return resp,
    };
    let context_latency_us = context_start.elapsed().as_micros();
    let protocol = ctx.protocol.clone();

    debug!(
        request_id = %request_id,
        service = %ctx.service,
        operation = %ctx.operation,
        region = %ctx.region,
        account_id = %ctx.account_id,
        context_latency_us = context_latency_us,
        "Dispatching request"
    );

    // Convert to service framework context (consumes ctx — SpooledBody is not Clone)
    let svc_ctx = ctx.to_service_request_context();

    // Dispatch to the service provider
    let start = std::time::Instant::now();
    let result = state.plugin_manager.dispatch(&svc_ctx).await;
    let latency_ms = start.elapsed().as_millis();
    let total_latency_ms = request_start.elapsed().as_millis();

    let (status, resp_body, content_type, extra_headers) = match result {
        Ok(response) => {
            info!(
                request_id = %request_id,
                service = %svc_ctx.service,
                operation = %svc_ctx.operation,
                status = response.status_code,
                latency_ms = latency_ms,
                total_latency_ms = total_latency_ms,
                "Request completed"
            );
            (
                StatusCode::from_u16(response.status_code).unwrap_or(StatusCode::OK),
                response.body,
                response.content_type,
                response.headers,
            )
        }
        Err(e) => {
            use openstack_service_framework::traits::DispatchError;
            let (code, message, http_status) = match &e {
                DispatchError::NotImplemented(op) => (
                    "NotImplemented",
                    format!("Operation '{}' is not implemented", op),
                    501u16,
                ),
                DispatchError::ServiceNotFound(svc) => (
                    "InternalFailure",
                    format!(
                        "Service '{}' is not enabled. Please check your 'SERVICES' configuration variable.",
                        svc
                    ),
                    501,
                ),
                DispatchError::ServiceUnavailable(msg) => ("ServiceUnavailable", msg.clone(), 503),
                DispatchError::ProviderError(msg) => ("InternalFailure", msg.clone(), 500),
                DispatchError::SerializationError(msg) => ("InternalFailure", msg.clone(), 500),
            };

            warn!(
                request_id = %request_id,
                service = %svc_ctx.service,
                operation = %svc_ctx.operation,
                error = %e,
                http_status = http_status,
                latency_ms = latency_ms,
                total_latency_ms = total_latency_ms,
                "Request failed"
            );

            let (status_code, body, ct) = openstack_aws_protocol::serialize_error(
                &protocol,
                code,
                &message,
                http_status,
                &request_id,
            );
            (
                StatusCode::from_u16(status_code).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
                ResponseBody::Buffered(body),
                std::borrow::Cow::Borrowed(ct),
                Vec::new(),
            )
        }
    };

    // Build the response based on body variant
    let mut response = match resp_body {
        ResponseBody::Buffered(bytes) => Response::builder()
            .status(status)
            .header("content-type", &*content_type)
            .header("x-amzn-requestid", &request_id)
            .body(Body::from(bytes))
            .unwrap_or_default(),
        ResponseBody::Streaming {
            stream,
            content_length,
        } => {
            let mut builder = Response::builder()
                .status(status)
                .header("content-type", &*content_type)
                .header("x-amzn-requestid", &request_id)
                // Prevent tower-http CompressionLayer from compressing binary
                // object data (S3 GET). Compressing random/incompressible bytes
                // burns CPU for zero benefit and adds significant latency.
                .header("content-encoding", "identity");
            if let Some(len) = content_length {
                builder = builder.header("content-length", len.to_string());
            }
            builder.body(Body::from_stream(stream)).unwrap_or_default()
        }
    };

    // Add extra headers from the provider
    for (key, value) in extra_headers {
        if let Ok(v) = axum::http::HeaderValue::from_str(&value) {
            response.headers_mut().insert(
                axum::http::HeaderName::from_bytes(key.as_bytes()).unwrap(),
                v,
            );
        }
    }

    // Add CORS headers
    state
        .cors
        .add_cors_headers(response.headers_mut(), origin_header.as_deref());

    // Fire-and-forget: record this transaction to the Studio transaction log.
    // Skipped entirely when Studio is disabled to avoid spawn overhead on every request.
    // Requests initiated by Studio itself attach x-openstack-studio-origin=1
    // and are recorded client-side to avoid duplicate transaction rows.
    if state.studio_enabled && !is_studio_origin {
        let service = svc_ctx.service.clone();
        // Allow rest-xml services (S3) to provide the true operation name.
        let operation = state
            .plugin_manager
            .derive_operation(&svc_ctx)
            .unwrap_or_else(|| svc_ctx.operation.clone());
        let method = method.to_string();
        let path_str = svc_ctx.path.clone();
        let status_u16 = status.as_u16();
        let dur_ms = latency_ms as u64;
        let router = state.internal_api_router.clone();

        tokio::spawn(async move {
            use axum::body::Body;
            let payload = serde_json::json!({
                "service":   service,
                "operation": operation,
                "method":    method,
                "path":      path_str,
                "status":    status_u16,
                "startedAtMs": 0u64,
                "durationMs":  dur_ms,
                "fromGuidedFlow": false,
            });
            let req = axum::http::Request::builder()
                .method("POST")
                .uri("/_localstack/studio-api/transactions/record")
                .header("content-type", "application/json")
                .body(Body::from(serde_json::to_vec(&payload).unwrap_or_default()))
                .unwrap_or_default();
            let _ = tower::ServiceExt::oneshot(router, req).await;
        });
    }

    response
}

fn is_studio_spa_route(path: &str) -> bool {
    (path == "/_localstack/studio"
        || path == "/_localstack/studio/"
        || path.starts_with("/_localstack/studio/"))
        && !path.starts_with("/_localstack/studio/assets/")
}

fn is_studio_asset_route(path: &str) -> bool {
    path.starts_with("/_localstack/studio/assets/")
}

fn studio_spa_response() -> Response {
    Response::builder()
        .status(StatusCode::OK)
        .header("content-type", "text/html; charset=utf-8")
        .header("cache-control", "no-cache")
        .header("etag", "\"studio-shell-v1\"")
        .body(Body::from(STUDIO_SPA))
        .unwrap_or_default()
}

fn studio_asset_response(path: &str) -> Response {
    let (status, content_type, body, cache_control) = match path {
        "/_localstack/studio/assets/app.js" => (
            StatusCode::OK,
            "application/javascript; charset=utf-8",
            STUDIO_ASSET_JS,
            "public, max-age=31536000, immutable",
        ),
        "/_localstack/studio/assets/app.css" => (
            StatusCode::OK,
            "text/css; charset=utf-8",
            STUDIO_ASSET_CSS,
            "public, max-age=31536000, immutable",
        ),
        _ => (
            StatusCode::NOT_FOUND,
            "text/plain; charset=utf-8",
            "Not found",
            "no-cache",
        ),
    };

    Response::builder()
        .status(status)
        .header("content-type", content_type)
        .header("cache-control", cache_control)
        .header("etag", "\"studio-asset-v1\"")
        .body(Body::from(body))
        .unwrap_or_default()
}

#[allow(clippy::result_large_err)]
fn build_request_context(
    method: &Method,
    path: String,
    query_params: HashMap<String, String>,
    headers: HeaderMap,
    body: &Bytes,
    request_id: &str,
    config: &Config,
    body_reader: Option<BodyReader>,
) -> Result<RequestContext, Response> {
    // Parse SigV4 Authorization or inject default.
    // SigV4Auth borrows from the auth header string — no allocations here.
    // We convert to owned Strings only at RequestContext construction.
    let (access_key, region, service_from_auth): (&str, &str, Option<&str>) =
        if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok()) {
            if let Some(sigv4) = parse_sigv4_auth(auth) {
                (sigv4.access_key, sigv4.region, Some(sigv4.service))
            } else {
                (DEFAULT_ACCESS_KEY, DEFAULT_REGION, None)
            }
        } else {
            (DEFAULT_ACCESS_KEY, DEFAULT_REGION, None)
        };

    // Derive account ID from access key
    let account_id = access_key_to_account_id(access_key);

    // Determine the target service
    let service =
        detect_service(&path, &query_params, &headers, body, service_from_auth).into_owned();

    // Virtual-hosted-style S3 rewriting:
    //   bucket.s3.amazonaws.com/key  →  path = /bucket/key
    //   bucket.localhost:4566/key    →  path = /bucket/key
    //
    // The AWS SDK v3 defaults to virtual-hosted style.  We normalise the path
    // here so the S3 provider always sees path-style, matching its routing logic.
    let path = if service == "s3" {
        rewrite_s3_virtual_hosted_path(&path, &headers, &config.localstack_host)
    } else {
        path
    };

    // Validate / normalize region
    let region = if config.allow_nonstandard_regions || is_valid_region(region) {
        region.to_string()
    } else {
        warn!("Invalid region '{}', falling back to us-east-1", region);
        DEFAULT_REGION.to_string()
    };

    // Determine the protocol used by this service
    let protocol = AwsProtocol::from_service(&service);

    // Parse the request body according to protocol
    let (operation, params) =
        match parse_operation_and_params(method, &path, &query_params, &headers, body, &protocol) {
            Ok(result) => result,
            Err(e) => {
                error!("Failed to parse request: {}", e);
                let body = format!("Failed to parse request: {}", e);
                return Err((StatusCode::BAD_REQUEST, body).into_response());
            }
        };

    Ok(RequestContext {
        service,
        operation,
        region,
        account_id,
        access_key: access_key.to_string(),
        protocol,
        params,
        raw_body: if body.is_empty() {
            None
        } else {
            Some(body.clone())
        },
        headers,
        path,
        method: method.to_string(),
        query_params,
        request_id: request_id.to_string(),
        spooled_body: None,
        body_reader,
    })
}

/// Rewrite an S3 virtual-hosted-style path to path-style.
///
/// If the `Host` header looks like `<bucket>.<s3-endpoint>` (e.g.
/// `my-bucket.s3.amazonaws.com`, `my-bucket.localhost:4566`,
/// `my-bucket.s3.us-east-1.localhost.localstack.cloud`), extract `<bucket>`
/// and prepend it to `path` so the S3 provider always sees `/bucket[/key]`.
///
/// Path-style requests are returned unchanged.
fn rewrite_s3_virtual_hosted_path(
    path: &str,
    headers: &HeaderMap,
    localstack_host: &str,
) -> String {
    let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) else {
        return path.to_string();
    };
    // Strip port so comparison works for both localhost:4566 and bare hostnames.
    let host_no_port = host.split(':').next().unwrap_or(host);
    let localstack_host_no_port = localstack_host.split(':').next().unwrap_or(localstack_host);

    // Patterns that indicate virtual-hosted style:
    //   <bucket>.s3.amazonaws.com
    //   <bucket>.s3.<region>.amazonaws.com
    //   <bucket>.s3.<region>.localhost.localstack.cloud
    //   <bucket>.localhost
    //   <bucket>.<localstack_host>
    let bucket = if let Some(rest) = host_no_port.strip_suffix(".s3.amazonaws.com") {
        Some(rest)
    } else if let Some(b) = extract_bucket_before_s3_region(host_no_port) {
        Some(b)
    } else {
        // <bucket>.localhost or <bucket>.<localstack_host>
        host_no_port
            .strip_suffix(&format!(".{localstack_host_no_port}"))
            .or_else(|| host_no_port.strip_suffix(".localhost"))
            .filter(|rest| !rest.contains('.') && !rest.is_empty())
    };

    match bucket {
        Some(b) if !b.is_empty() => {
            // Avoid double-prefix: if path already starts with /bucket skip.
            let stripped = path.trim_start_matches('/');
            if stripped.starts_with(b)
                && (stripped.len() == b.len() || stripped.as_bytes().get(b.len()) == Some(&b'/'))
            {
                // Already path-style (shouldn't happen in practice).
                path.to_string()
            } else {
                format!("/{b}{path}")
            }
        }
        _ => path.to_string(),
    }
}

/// Extract the bucket name from hosts like
/// `bucket.s3.us-east-1.amazonaws.com` or
/// `bucket.s3.us-east-1.localhost.localstack.cloud`.
fn extract_bucket_before_s3_region(host: &str) -> Option<&str> {
    // Must have at least 4 parts: bucket . s3 . region . tld
    let parts: Vec<&str> = host.split('.').collect();
    if parts.len() < 4 {
        return None;
    }
    // Second segment must be "s3".
    if parts[1].eq_ignore_ascii_case("s3") {
        Some(parts[0])
    } else {
        None
    }
}

/// Detect which AWS service is being targeted.
fn detect_service(
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &Bytes,
    service_from_auth: Option<&str>,
) -> Cow<'static, str> {
    // 1. Authorization header credential scope (highest priority)
    if let Some(svc) = service_from_auth {
        return normalize_service_name(svc);
    }

    // 2. Host header: sqs.us-east-1.localhost.localstack.cloud
    if let Some(host) = headers.get("host").and_then(|v| v.to_str().ok()) {
        let host = host.split(':').next().unwrap_or(host);
        let parts: Vec<&str> = host.split('.').collect();
        if parts.len() >= 2 {
            let potential_service = normalize_service_name(parts[0]);
            if potential_service
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit())
                && is_known_service(&potential_service)
            {
                return potential_service;
            }
        }
    }

    // 3. X-Amz-Target header: "DynamoDB_20120810.GetItem"
    if path == "/"
        && query_params.is_empty()
        && body.is_empty()
        && !headers.contains_key("x-amz-target")
    {
        return Cow::Borrowed("s3");
    }

    if let Some(target) = headers.get("x-amz-target").and_then(|v| v.to_str().ok())
        && let Some(svc) = service_from_target(target)
    {
        return Cow::Borrowed(svc);
    }

    // 4. Query protocol Action (POST form body or query string)
    if let Some(svc) = service_from_query_action(query_params, body) {
        return Cow::Borrowed(svc);
    }

    // 5. URL path patterns
    if let Some(svc) = service_from_path(path) {
        return Cow::Borrowed(svc);
    }

    // 6. S3 path-style heuristic for unsigned endpoint-url calls
    let trimmed = path.trim_start_matches('/');
    if !trimmed.is_empty() {
        return Cow::Borrowed("s3");
    }

    Cow::Borrowed("unknown")
}

fn normalize_service_name(service: &str) -> Cow<'static, str> {
    if service.eq_ignore_ascii_case("es") {
        return Cow::Borrowed("opensearch");
    }
    if service.eq_ignore_ascii_case("cognito") || service.eq_ignore_ascii_case("cognito-idp") {
        return Cow::Borrowed("cognito-idp");
    }
    // AWS SDKs always send lowercase service names in credentials, so the
    // common path avoids `to_ascii_lowercase`'s character-by-character scan.
    if service.bytes().all(|b| !b.is_ascii_uppercase()) {
        // Try to resolve to a known static string to avoid allocating.
        if let Some(s) = known_service_static(service) {
            Cow::Borrowed(s)
        } else {
            Cow::Owned(service.to_string())
        }
    } else {
        Cow::Owned(service.to_ascii_lowercase())
    }
}

/// Return the canonical `&'static str` for a known service name.
/// This allows callers to avoid allocating when the input matches a known service.
fn known_service_static(name: &str) -> Option<&'static str> {
    match name {
        "s3" => Some("s3"),
        "sqs" => Some("sqs"),
        "sns" => Some("sns"),
        "dynamodb" => Some("dynamodb"),
        "lambda" => Some("lambda"),
        "iam" => Some("iam"),
        "sts" => Some("sts"),
        "kms" => Some("kms"),
        "cloudformation" => Some("cloudformation"),
        "cloudwatch" => Some("cloudwatch"),
        "logs" => Some("logs"),
        "kinesis" => Some("kinesis"),
        "firehose" => Some("firehose"),
        "events" => Some("events"),
        "states" => Some("states"),
        "apigateway" => Some("apigateway"),
        "ec2" => Some("ec2"),
        "route53" => Some("route53"),
        "ses" => Some("ses"),
        "ssm" => Some("ssm"),
        "secretsmanager" => Some("secretsmanager"),
        "acm" => Some("acm"),
        "ecr" => Some("ecr"),
        "opensearch" => Some("opensearch"),
        "redshift" => Some("redshift"),
        "ecs" => Some("ecs"),
        "cloudtrail" => Some("cloudtrail"),
        "cognito" => Some("cognito-idp"),
        "cognito-idp" => Some("cognito-idp"),
        "elasticache" => Some("elasticache"),
        "rds" => Some("rds"),
        _ => None,
    }
}

/// Zero-copy scan of URL-encoded body bytes for `Action=<value>`.
/// Returns the value slice without allocating.
fn extract_action_value(body: &[u8]) -> Option<&str> {
    let s = std::str::from_utf8(body).ok()?;
    s.split('&')
        .find_map(|segment| segment.strip_prefix("Action="))
}

fn service_from_query_action(
    query_params: &HashMap<String, String>,
    body: &Bytes,
) -> Option<&'static str> {
    let action: &str = if let Some(a) = query_params.get("Action").map(String::as_str) {
        a
    } else {
        extract_action_value(body)?
    };

    match action {
        // SQS
        "CreateQueue"
        | "DeleteQueue"
        | "GetQueueUrl"
        | "GetQueueAttributes"
        | "SetQueueAttributes"
        | "SendMessage"
        | "ReceiveMessage"
        | "DeleteMessage"
        | "PurgeQueue"
        | "ListQueues"
        | "SendMessageBatch"
        | "DeleteMessageBatch"
        | "ChangeMessageVisibility"
        | "ChangeMessageVisibilityBatch" => Some("sqs"),
        // STS
        "GetCallerIdentity"
        | "AssumeRole"
        | "GetSessionToken"
        | "GetAccessKeyInfo"
        | "DecodeAuthorizationMessage" => Some("sts"),
        // SNS
        "CreateTopic"
        | "DeleteTopic"
        | "Publish"
        | "Subscribe"
        | "Unsubscribe"
        | "ListTopics"
        | "SetTopicAttributes"
        | "GetTopicAttributes"
        | "ListSubscriptions"
        | "ListSubscriptionsByTopic"
        | "GetSubscriptionAttributes" => Some("sns"),
        // IAM
        "CreateRole" | "DeleteRole" | "ListRoles" | "GetRole" | "CreateUser" | "DeleteUser"
        | "ListUsers" | "GetUser" => Some("iam"),
        // CloudFormation
        "CreateStack" | "DeleteStack" | "DescribeStacks" | "ListStacks" | "GetTemplate"
        | "ValidateTemplate" | "UpdateStack" => Some("cloudformation"),
        // CloudWatch (query actions)
        "PutMetricData"
        | "ListMetrics"
        | "GetMetricStatistics"
        | "DescribeAlarms"
        | "PutMetricAlarm"
        | "DeleteAlarms"
        | "SetAlarmState" => Some("cloudwatch"),
        // EC2
        "DescribeVpcs"
        | "CreateVpc"
        | "DeleteVpc"
        | "DescribeSubnets"
        | "CreateSubnet"
        | "DescribeSecurityGroups"
        | "CreateSecurityGroup"
        | "AuthorizeSecurityGroupIngress"
        | "RunInstances"
        | "DescribeInstances"
        | "TerminateInstances" => Some("ec2"),
        // Redshift
        "CreateCluster" | "DeleteCluster" | "DescribeClusters" | "ModifyCluster"
        | "RebootCluster" => Some("redshift"),
        // SES
        "VerifyEmailIdentity"
        | "VerifyDomainIdentity"
        | "ListIdentities"
        | "SendEmail"
        | "SendRawEmail"
        | "GetIdentityVerificationAttributes"
        | "DeleteIdentity" => Some("ses"),
        _ => None,
    }
}

fn is_known_service(name: &str) -> bool {
    matches!(
        name,
        "s3" | "sqs"
            | "sns"
            | "dynamodb"
            | "lambda"
            | "iam"
            | "sts"
            | "kms"
            | "cloudformation"
            | "cloudwatch"
            | "logs"
            | "kinesis"
            | "firehose"
            | "events"
            | "states"
            | "apigateway"
            | "ec2"
            | "route53"
            | "ses"
            | "ssm"
            | "secretsmanager"
            | "acm"
            | "ecr"
            | "opensearch"
            | "redshift"
            | "ecs"
            | "cloudtrail"
            | "cognito"
            | "cognito-idp"
            | "elasticache"
            | "rds"
    )
}

/// Derive the AWS service name from an `X-Amz-Target` header value.
///
/// Formats seen in the wild:
/// - `"DynamoDB_20120810.GetItem"`
/// - `"AmazonSQS.CreateQueue"`
/// - `"AWSSecurityTokenServiceV20110615.GetCallerIdentity"`
///
/// Uses `eq_ignore_ascii_case` matching throughout — no heap allocation.
fn service_from_target(target: &str) -> Option<&'static str> {
    // Take everything before the first '.' then before the first '_'.
    let prefix = target.split('.').next().unwrap_or(target);
    let prefix = prefix.split('_').next().unwrap_or(prefix);

    // Strip trailing version suffixes like "V20110615" or "v20120810".
    let prefix = if prefix.len() > 9
        && (prefix[prefix.len() - 9..].eq_ignore_ascii_case("v20110615")
            || prefix[prefix.len() - 9..].eq_ignore_ascii_case("v20120810"))
    {
        &prefix[..prefix.len() - 9]
    } else {
        prefix
    };

    if prefix.eq_ignore_ascii_case("dynamodb") {
        Some("dynamodb")
    } else if prefix.eq_ignore_ascii_case("certificatemanager") {
        Some("acm")
    } else if prefix.eq_ignore_ascii_case("kinesis") {
        Some("kinesis")
    } else if prefix.eq_ignore_ascii_case("firehose") {
        Some("firehose")
    } else if prefix.eq_ignore_ascii_case("lambda") {
        Some("lambda")
    } else if prefix.eq_ignore_ascii_case("awsstepfunctions") {
        Some("states")
    } else if prefix.eq_ignore_ascii_case("logs") {
        Some("logs")
    } else if prefix.eq_ignore_ascii_case("amazonssm") {
        Some("ssm")
    } else if prefix.eq_ignore_ascii_case("kms") || prefix.eq_ignore_ascii_case("trentservice") {
        Some("kms")
    } else if prefix.eq_ignore_ascii_case("secretsmanager") {
        Some("secretsmanager")
    } else if prefix.eq_ignore_ascii_case("ssm") {
        Some("ssm")
    } else if prefix.eq_ignore_ascii_case("cloudwatch") {
        Some("cloudwatch")
    } else if prefix.eq_ignore_ascii_case("awsevents") || prefix.eq_ignore_ascii_case("events") {
        Some("events")
    } else if prefix.eq_ignore_ascii_case("awscognitoidentityproviderservice") {
        Some("cognito-idp")
    } else if prefix.eq_ignore_ascii_case("amazonec2containerservice") {
        Some("ecs")
    } else if prefix.eq_ignore_ascii_case("cloudtrail")
        || prefix.eq_ignore_ascii_case("com.amazonaws.cloudtrail")
    {
        Some("cloudtrail")
    } else if prefix.eq_ignore_ascii_case("amazonec2containerregistry")
        || prefix.eq_ignore_ascii_case("ecr")
    {
        Some("ecr")
    } else if prefix.eq_ignore_ascii_case("sns") {
        Some("sns")
    } else if prefix.eq_ignore_ascii_case("amazonsqs") || prefix.eq_ignore_ascii_case("sqs") {
        Some("sqs")
    } else if prefix.eq_ignore_ascii_case("awssecuritytokenservice")
        || prefix.eq_ignore_ascii_case("sts")
    {
        Some("sts")
    } else {
        None
    }
}

fn service_from_path(path: &str) -> Option<&'static str> {
    // Common path-based routing
    let path = path.trim_start_matches('/');
    if path.starts_with("restapis") {
        return Some("apigateway");
    }
    if path.starts_with("2021-01-01/opensearch/domain") || path == "2021-01-01/domain" {
        return Some("opensearch");
    }
    if path.starts_with("2013-04-01/hostedzone") {
        return Some("route53");
    }
    if path.starts_with("2015-03-31/functions")
        || path.starts_with("2015-03-31/event-source-mappings")
    {
        return Some("lambda");
    }
    if path.starts_with("2012-12-01/") || path.contains("elasticloadbalancing") {
        return Some("elb");
    }
    // Default: can't determine from path alone
    None
}

fn parse_operation_and_params(
    method: &Method,
    path: &str,
    query_params: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &Bytes,
    protocol: &AwsProtocol,
) -> Result<(String, serde_json::Value), String> {
    match protocol {
        AwsProtocol::Query => match parse_query_request(body) {
            Ok((op, params)) => Ok((op, params)),
            Err(e) => {
                let missing_action = e.to_string().contains("Missing 'Action' parameter");
                let query_mode = headers
                    .get("x-amzn-query-mode")
                    .and_then(|v| v.to_str().ok())
                    .map(|v| v == "true")
                    .unwrap_or(false);
                if missing_action && query_mode {
                    let target = headers
                        .get("x-amz-target")
                        .and_then(|v| v.to_str().ok())
                        .ok_or_else(|| e.to_string())?;
                    let operation = target
                        .split('.')
                        .nth(1)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| e.to_string())?
                        .to_string();
                    let params = if body.is_empty() {
                        serde_json::json!({})
                    } else {
                        serde_json::from_slice(body).map_err(|err| err.to_string())?
                    };
                    Ok((operation, params))
                } else {
                    Err(e.to_string())
                }
            }
        },
        AwsProtocol::Ec2 => {
            let (op, params) = parse_ec2_request(body).map_err(|e| e.to_string())?;
            Ok((op, params))
        }
        AwsProtocol::Json => {
            let target = headers.get("x-amz-target").and_then(|v| v.to_str().ok());
            let (op, params) = parse_json_request(body, target).map_err(|e| e.to_string())?;
            Ok((op, params))
        }
        AwsProtocol::RestJson => {
            let params = parse_rest_json_request(method.as_str(), path, body, query_params)
                .map_err(|e| e.to_string())?;
            // For REST-JSON, operation comes from path routing (service-specific)
            let op = extract_rest_operation(method.as_str(), path, &params);
            Ok((op, params))
        }
        AwsProtocol::RestXml => {
            let params = parse_rest_xml_request(method.as_str(), path, body, query_params)
                .map_err(|e| e.to_string())?;
            let op = extract_rest_operation(method.as_str(), path, &params);
            Ok((op, params))
        }
    }
}

/// Extract operation name from REST path + method.
/// The actual operation mapping is done per-service in the provider.
fn extract_rest_operation(method: &str, path: &str, _params: &serde_json::Value) -> String {
    if path == "/2015-03-31/functions/" || path == "/2015-03-31/functions" {
        return match method {
            "GET" => "ListFunctions".to_string(),
            "POST" => "CreateFunction".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path == "/2021-01-01/opensearch/domain" {
        return match method {
            "POST" => "CreateDomain".to_string(),
            "GET" => "ListDomainNames".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path == "/2021-01-01/domain" {
        return match method {
            "GET" => "ListDomainNames".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2021-01-01/opensearch/domain/") {
        if path.ends_with("/config") {
            return match method {
                "GET" => "DescribeDomainConfig".to_string(),
                "POST" => "UpdateDomainConfig".to_string(),
                _ => format!("{}:{}", method, path),
            };
        }
        return match method {
            "GET" => "DescribeDomain".to_string(),
            "DELETE" => "DeleteDomain".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path == "/2013-04-01/hostedzone" {
        return match method {
            "POST" => "CreateHostedZone".to_string(),
            "GET" => "ListHostedZones".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2013-04-01/hostedzone/") {
        if path.ends_with("/rrset") {
            return match method {
                "GET" => "ListResourceRecordSets".to_string(),
                "POST" => "ChangeResourceRecordSets".to_string(),
                _ => format!("{}:{}", method, path),
            };
        }
        return match method {
            "GET" => "GetHostedZone".to_string(),
            "DELETE" => "DeleteHostedZone".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2013-04-01/change/") {
        return match method {
            "GET" => "GetChange".to_string(),
            _ => format!("{}:{}", method, path),
        };
    }
    if path.starts_with("/2015-03-31/functions/") {
        let suffix = path.trim_start_matches("/2015-03-31/functions/");
        if suffix.ends_with("/code") && method == "PUT" {
            return "UpdateFunctionCode".to_string();
        }
        if suffix.ends_with("/invocations") && method == "POST" {
            return "Invoke".to_string();
        }
        if method == "GET" {
            return "GetFunction".to_string();
        }
        if method == "DELETE" {
            return "DeleteFunction".to_string();
        }
        if method == "PUT" {
            return "UpdateFunctionConfiguration".to_string();
        }
    }
    // For REST protocols, the operation is inferred by the service provider
    // We store method + path in the params for the provider to use
    format!("{}:{}", method, path)
}

/// Handle internal API requests (/_localstack/*)
async fn handle_internal_api(
    path: String,
    method: &Method,
    _headers: &HeaderMap,
    _query_params: &HashMap<String, String>,
    _body: &Bytes,
    _state: &AppState,
) -> Response {
    if is_studio_guided_execution_route(&path) {
        if *method != Method::POST {
            return (
                StatusCode::METHOD_NOT_ALLOWED,
                "method not allowed for guided execution endpoint",
            )
                .into_response();
        }
        if _body.len() > STUDIO_GUIDED_MAX_PAYLOAD_BYTES {
            return (
                StatusCode::PAYLOAD_TOO_LARGE,
                "guided execution payload exceeds configured limit",
            )
                .into_response();
        }
    }

    let uri = if _query_params.is_empty() {
        path
    } else {
        let query = serde_urlencoded::to_string(_query_params).unwrap_or_default();
        format!("{}?{}", path, query)
    };

    let mut req_builder = axum::http::Request::builder()
        .method(method.clone())
        .uri(uri);
    // HeaderMap iter yields (&HeaderName, &HeaderValue) — copy directly, no string round-trip.
    for (k, v) in _headers {
        req_builder = req_builder.header(k, v);
    }
    let req = req_builder
        .body(Body::from(_body.clone()))
        .unwrap_or_default();

    use tower::ServiceExt;
    match _state.internal_api_router.clone().oneshot(req).await {
        Ok(resp) => resp,
        Err(e) => {
            error!(error = %e, "internal API router dispatch failed");
            (StatusCode::INTERNAL_SERVER_ERROR, "Internal API error").into_response()
        }
    }
}

fn is_studio_guided_execution_route(path: &str) -> bool {
    path == "/_localstack/studio-api/flows/execute"
        || path == "/_localstack/studio-api/flows/replay"
}

/// Returns `true` for S3 requests where the body is raw object data
/// (binary), not XML/JSON.  For these requests we skip materializing the
/// body into a `Bytes` heap allocation and let the S3 provider stream it
/// directly from the `SpooledBody`.
///
/// The heuristic: the Authorization header identifies the service as "s3",
/// the method is PUT or POST with a non-empty key segment in the path.
/// Content-Type is NOT checked because the S3 SDK may omit it or set it
/// to "application/octet-stream" for arbitrary object data.
fn should_rewrite_s3_vhost_for_body_check(
    headers: &HeaderMap,
    query_params: &HashMap<String, String>,
) -> bool {
    // Signed header style: Authorization: .../<region>/s3/aws4_request
    if headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(parse_sigv4_auth)
        .is_some_and(|auth| auth.service.eq_ignore_ascii_case("s3"))
    {
        return true;
    }

    // Presigned URL style: X-Amz-Credential=.../<region>/s3/aws4_request
    query_params
        .get("X-Amz-Credential")
        .or_else(|| query_params.get("x-amz-credential"))
        .is_some_and(|cred| cred.split('/').nth(3).is_some_and(|svc| svc == "s3"))
}

fn is_s3_object_body_request(
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    query_params: &HashMap<String, String>,
) -> bool {
    // Must be PUT or POST (GET, HEAD, DELETE have no object body).
    if method != Method::PUT && method != Method::POST {
        return false;
    }

    // Path must have at least two non-empty segments: /bucket/key
    // (bucket-level PUT operations like ?versioning, ?policy have no key).
    //
    // S3 path-style URLs have a plain /bucket/key structure.  No other
    // service uses this pattern for binary object uploads:
    //   - Lambda paths start with 2015-03-31/
    //   - SQS/SNS/DynamoDB POST to the root (/) or /?QueueUrl= style
    //   - ELB paths start with 2012-12-01/
    //
    // We intentionally do NOT require an Authorization header here so that
    // benchmark and integration clients that omit auth (e.g. curl/oha without
    // signing) still get the streaming path and never load binary object
    // bodies into heap memory.
    let path_no_query = path.split('?').next().unwrap_or(path);
    let segments: Vec<&str> = path_no_query
        .trim_start_matches('/')
        .split('/')
        .filter(|s| !s.is_empty())
        .collect();

    if segments.len() < 2 {
        return false;
    }

    // If request clearly targets another SigV4 service, this is not S3.
    if let Some(auth) = headers.get("authorization").and_then(|v| v.to_str().ok())
        && let Some(sigv4) = parse_sigv4_auth(auth)
        && !sigv4.service.eq_ignore_ascii_case("s3")
    {
        return false;
    }

    // Exclude paths that belong to known non-S3 services (versioned REST APIs).
    let first = segments[0];
    // Lambda, ELB, EC2 (rare REST calls)
    if first.starts_with("2015-03-31")
        || first.starts_with("2012-12-01")
        || first.starts_with("2016-11-15")
    {
        return false;
    }

    // Exclude sub-resource operations that carry XML bodies, not binary
    // object data.  These are identified by specific query params:
    //
    //  - CompleteMultipartUpload: POST /bucket/key?uploadId=<id>  (no partNumber)
    //  - DeleteObjects:           POST /bucket?delete             (single-segment path)
    //
    // UploadPart is a POST with BOTH ?uploadId= AND ?partNumber= — that IS a
    // binary body and must NOT be excluded.
    if method == Method::POST {
        let has_upload_id = query_params.contains_key("uploadId");
        let has_part_number = query_params.contains_key("partNumber");
        if has_upload_id && !has_part_number {
            // CompleteMultipartUpload — XML body, not binary.
            return false;
        }
        if !has_upload_id && !has_part_number {
            // Any other key-level POST without multipart params (e.g. restore-object)
            // may also carry XML.  Be conservative and only treat UploadPart as binary.
            return false;
        }
    }

    // XML-body bucket/object subresources should go through normal body parsing.
    // These requests are not raw object-data uploads.
    const XML_BODY_SUBRESOURCES: &[&str] = &[
        "acl",
        "tagging",
        "policy",
        "website",
        "cors",
        "lifecycle",
        "notification",
        "replication",
        "requestPayment",
        "versioning",
        "logging",
        "encryption",
        "object-lock",
        "ownershipControls",
        "accelerate",
        "inventory",
        "analytics",
        "metrics",
    ];
    if query_params
        .keys()
        .any(|key| XML_BODY_SUBRESOURCES.contains(&key.as_str()))
    {
        return false;
    }

    // Accept unsigned simple path-style uploads (/bucket/key) for parity tools,
    // but require explicit S3 hints for deeper multi-segment paths so non-S3
    // REST APIs are not misclassified.
    let has_s3_hint = {
        let host_hint = headers
            .get("host")
            .and_then(|v| v.to_str().ok())
            .map(|host| host.split(':').next().unwrap_or(host))
            .map(|host| {
                host.eq_ignore_ascii_case("s3") || host.starts_with("s3.") || host.contains(".s3.")
            })
            .unwrap_or(false);

        let auth_hint = headers
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .map(|auth| auth.contains("/s3/aws4_request") || auth.contains("/S3/aws4_request"))
            .unwrap_or(false);

        host_hint
            || auth_hint
            || headers.contains_key("x-amz-content-sha256")
            || headers.contains_key("x-amz-storage-class")
    };

    if !has_s3_hint && segments.len() != 2 {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use axum::http::{HeaderMap, HeaderValue, Method};
    use bytes::Bytes;
    use serde_json::json;

    use super::{
        detect_service, extract_rest_operation, is_s3_object_body_request,
        rewrite_s3_virtual_hosted_path, should_rewrite_s3_vhost_for_body_check,
    };

    #[test]
    fn maps_lambda_create_function_with_or_without_trailing_slash() {
        let params = json!({});
        assert_eq!(
            extract_rest_operation("POST", "/2015-03-31/functions/", &params),
            "CreateFunction"
        );
        assert_eq!(
            extract_rest_operation("POST", "/2015-03-31/functions", &params),
            "CreateFunction"
        );
    }

    #[test]
    fn maps_lambda_list_functions_with_or_without_trailing_slash() {
        let params = json!({});
        assert_eq!(
            extract_rest_operation("GET", "/2015-03-31/functions/", &params),
            "ListFunctions"
        );
        assert_eq!(
            extract_rest_operation("GET", "/2015-03-31/functions", &params),
            "ListFunctions"
        );
    }

    #[test]
    fn maps_opensearch_config_routes() {
        let params = json!({});
        assert_eq!(
            extract_rest_operation(
                "POST",
                "/2021-01-01/opensearch/domain/bench-domain/config",
                &params,
            ),
            "UpdateDomainConfig"
        );
        assert_eq!(
            extract_rest_operation(
                "GET",
                "/2021-01-01/opensearch/domain/bench-domain/config",
                &params,
            ),
            "DescribeDomainConfig"
        );
    }

    #[test]
    fn maps_route53_hosted_zone_get_route() {
        let params = json!({});
        assert_eq!(
            extract_rest_operation("GET", "/2013-04-01/hostedzone/Z123456789", &params),
            "GetHostedZone"
        );
    }

    #[test]
    fn maps_route53_change_get_route() {
        let params = json!({});
        assert_eq!(
            extract_rest_operation("GET", "/2013-04-01/change/C123456789", &params),
            "GetChange"
        );
    }

    #[test]
    fn detect_service_normalizes_es_host_alias() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "host",
            HeaderValue::from_static("es.us-east-1.localhost.localstack.cloud"),
        );
        let query = HashMap::new();

        let service = detect_service("/my-index/_doc/1", &query, &headers, &Bytes::new(), None);
        assert_eq!(service, "opensearch");
    }

    #[test]
    fn detect_service_maps_extended_query_protocol_actions() {
        let headers = HeaderMap::new();
        let query = HashMap::new();

        let cases = [
            ("Action=DescribeAlarms&Version=2010-08-01", "cloudwatch"),
            (
                "Action=GetIdentityVerificationAttributes&Version=2010-12-01&Identities.member.1=bench%40example.com",
                "ses",
            ),
            ("Action=ListSubscriptions&Version=2010-03-31", "sns"),
            ("Action=GetSessionToken&Version=2011-06-15", "sts"),
            (
                "Action=GetAccessKeyInfo&AccessKeyId=AKIAIOSFODNN7EXAMPLE&Version=2011-06-15",
                "sts",
            ),
            (
                "Action=ModifyCluster&Version=2012-12-01&ClusterIdentifier=bench&NodeType=ra3.xlplus",
                "redshift",
            ),
            (
                "Action=RebootCluster&Version=2012-12-01&ClusterIdentifier=bench",
                "redshift",
            ),
        ];

        for (body, expected) in cases {
            let service = detect_service("/", &query, &headers, &Bytes::from(body), None);
            assert_eq!(service, expected, "body={body}");
        }
    }

    #[test]
    fn s3_object_body_detection_rejects_non_s3_unsigned_multisegment_put() {
        let headers = HeaderMap::new();
        let query = std::collections::HashMap::new();

        assert!(!is_s3_object_body_request(
            &Method::PUT,
            "/my-index/_doc/1",
            &headers,
            &query,
        ));
    }

    #[test]
    fn s3_object_body_detection_accepts_unsigned_simple_path_style_put() {
        let headers = HeaderMap::new();
        let query = std::collections::HashMap::new();

        assert!(is_s3_object_body_request(
            &Method::PUT,
            "/bench-bucket/object",
            &headers,
            &query,
        ));
    }

    #[test]
    fn s3_object_body_detection_rejects_xml_subresource_uploads() {
        let headers = HeaderMap::new();
        let query = std::collections::HashMap::from([("tagging".to_string(), String::new())]);

        assert!(!is_s3_object_body_request(
            &Method::PUT,
            "/bench-bucket/object",
            &headers,
            &query,
        ));
    }

    // ── S3 virtual-hosted path rewriting ─────────────────────────────────

    fn vhost(bucket: &str, host_suffix: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::HOST,
            HeaderValue::from_str(&format!("{bucket}.{host_suffix}")).unwrap(),
        );
        h
    }

    #[test]
    fn s3_vhost_amazonaws_root() {
        let h = vhost("my-bucket", "s3.amazonaws.com");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/", &h, "localhost:4566"),
            "/my-bucket/"
        );
    }

    #[test]
    fn s3_vhost_amazonaws_with_key() {
        let h = vhost("my-bucket", "s3.amazonaws.com");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/my-key.txt", &h, "localhost:4566"),
            "/my-bucket/my-key.txt"
        );
    }

    #[test]
    fn s3_vhost_amazonaws_regional() {
        let h = vhost("my-bucket", "s3.us-east-1.amazonaws.com");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/img/cat.png", &h, "localhost:4566"),
            "/my-bucket/img/cat.png"
        );
    }

    #[test]
    fn s3_vhost_localhost_4566() {
        let h = vhost("my-bucket", "localhost:4566");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/object.txt", &h, "localhost:4566"),
            "/my-bucket/object.txt"
        );
    }

    #[test]
    fn s3_vhost_localstack_cloud() {
        let h = vhost("my-bucket", "s3.us-east-1.localhost.localstack.cloud");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/k", &h, "localhost.localstack.cloud:4566"),
            "/my-bucket/k"
        );
    }

    #[test]
    fn s3_path_style_unchanged() {
        let h = HeaderMap::new(); // no host header
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/my-bucket/my-key", &h, "localhost:4566"),
            "/my-bucket/my-key"
        );
    }

    #[test]
    fn s3_path_style_with_s3_host_unchanged() {
        // s3.amazonaws.com without a bucket subdomain = path-style
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::header::HOST,
            HeaderValue::from_static("s3.amazonaws.com"),
        );
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/my-bucket/my-key", &h, "localhost:4566"),
            "/my-bucket/my-key"
        );
    }

    #[test]
    fn s3_vhost_root_path() {
        // Root path on a vhost bucket should become /bucket/
        let h = vhost("my-bucket", "s3.amazonaws.com");
        assert_eq!(
            rewrite_s3_virtual_hosted_path("/", &h, "localhost:4566"),
            "/my-bucket/"
        );
    }

    #[test]
    fn body_check_rewrite_enabled_for_sigv4_s3_auth() {
        let mut headers = HeaderMap::new();
        headers.insert(
            "authorization",
            HeaderValue::from_static(
                "AWS4-HMAC-SHA256 Credential=test/20260402/us-east-1/s3/aws4_request, SignedHeaders=host;x-amz-date, Signature=abc",
            ),
        );
        let query = std::collections::HashMap::new();
        assert!(should_rewrite_s3_vhost_for_body_check(&headers, &query));
    }

    #[test]
    fn body_check_rewrite_disabled_without_s3_markers() {
        let headers = HeaderMap::new();
        let query = std::collections::HashMap::new();
        assert!(!should_rewrite_s3_vhost_for_body_check(&headers, &query));
    }
}
