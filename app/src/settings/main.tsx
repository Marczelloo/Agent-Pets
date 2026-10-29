import { useCallback, useEffect, useState } from 'react';
import { createRoot } from 'react-dom/client';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import type { AppId, AppRow, Diagnostics, HotkeyStatus, MonitorInfo, Settings, SettingsView as View, StageLayout, UpdateStatus } from '../types';
import { defaultSettings } from './model';
import { SettingsView, TABS, type Tab } from './SettingsView';
import { Wizard } from './Wizard';
import { setLoopSaving } from './look/loop';
import { setPreviewSaving } from './look/PetsCanvas';
import { resolveLang, setLang, setPreviewLang, setSystemLang, t } from '../i18n';
import { applyTheme } from '../theme';

const inTauri = '__TAURI_INTERNALS__' in window;
if (!inTauri) setPreviewLang();

/** Demo data for browser preview (`pnpm dev`, /settings.html; `?wizard` shows the wizard). */
const demoRows: AppRow[] = [
  { id: 'claude_code', detected: { found: true, path: 'C:/Users/ja/.claude', note: null }, status: { installed: true, detail: '' }, enabled: true },
  { id: 'codex', detected: { found: true, path: 'C:/Users/ja/.codex', note: null }, status: { installed: true, detail: '' }, enabled: true },
  { id: 'agent_router', detected: { found: true, path: 'C:/Users/ja/.agent-router', note: null }, status: { installed: true, detail: '' }, enabled: true },
  { id: 'opencode', detected: { found: true, path: 'C:/Users/ja/.config/opencode', note: null }, status: { installed: false, detail: '' }, enabled: false },
  { id: 'copilot', detected: { found: true, path: 'C:/Users/ja/.copilot', note: null }, status: { installed: false, detail: '' }, enabled: false },
  { id: 'cursor', detected: { found: false, path: null, note: 'Not found: ~/.cursor' }, status: { installed: false, detail: '' }, enabled: false },
];
const demoDiag: Diagnostics = { version: '0.5.0', endpoint_port: 61234, settings_path: 'C:/Users/ja/.agent-pets/settings.json', settings_error: null,
  hook_exe: 'C:/Users/ja/.agent-pets/hook.exe', autostart_registered: true, last_seen: { claude_code: Date.now() - 20_000, codex: Date.now() - 300_000 },
  apps: [['claude_code', true, ''], ['codex', true, ''], ['agent_router', true, '']],
  stats_files: 0, stats_scanned_bytes: 0, stats_total_bytes: 0,
  log_path: '', log_tail: [] };

/** Tab from the URL (`settings.html#stage` from the stage menu). */
const tabFrom = (hash: string): Tab | null => {
  const t = hash.replace(/^#/, '') as Tab;
  return TABS.includes(t) ? t : null;
};

function Root() {
  const [view, setView] = useState<View | null>(null);
  const [rows, setRows] = useState<AppRow[]>([]);
  const [diag, setDiag] = useState<Diagnostics | null>(null);
  const [tab, setTab] = useState<Tab>(() => tabFrom(location.hash) ?? 'apps');
  const [message, setMessage] = useState<string | null>(null);
  const [update, setUpdate] = useState<UpdateStatus>({ state: 'idle' });
  const [monitors, setMonitors] = useState<MonitorInfo[]>([]);
  const [leftFallback, setLeftFallback] = useState(false);
  const [verticalBar, setVerticalBar] = useState(false);
  const [hotkeyStatus, setHotkeyStatus] = useState<HotkeyStatus>({ jump: null, panel: null });

  const reload = useCallback(async () => {
    if (!inTauri) {
      setView({ settings: defaultSettings(), first_run: location.search.includes('wizard'), load_error: null });
      setRows(demoRows.map(r => ({ ...r, status: { ...r.status, detail: r.id === 'claude_code' ? t().demo.hooksInstalled : t().demo.nothingToInstall } })));
      setDiag({ ...demoDiag, apps: demoDiag.apps.map(([id, on]) => [id, on, id === 'claude_code' ? t().demo.hooksInstalled : t().demo.nothingToInstall]) });
      return;
    }
    // the theme goes on as soon as the settings arrive, not after the slower integration scan and diagnostics
    const viewP = invoke<View>('settings_get');
    void viewP.then(x => applyTheme(x.settings.theme), () => {});
    const [v, r, d] = await Promise.all([viewP, invoke<AppRow[]>('integrations_list'), invoke<Diagnostics>('diagnostics')]);
    setSystemLang(v.system_lang);
    setLang(resolveLang(v.settings.language ?? 'auto'));
    applyTheme(v.settings.theme);
    setView(v);
    setRows(r);
    setDiag(d);
    if (v.load_error) setMessage(t().settings.broken(v.load_error));
  }, []);

  useEffect(() => {
    void reload();
    if (!inTauri) return;
    const un = listen<Settings>('pets://settings', e => {
      setLang(resolveLang(e.payload.language ?? 'auto'));
      applyTheme(e.payload.theme);
      setView(v => (v ? { ...v, settings: e.payload } : v));
    });
    const power = (s: boolean) => { setLoopSaving(s); setPreviewSaving(s); };
    const unPower = listen<boolean>('pets://power', e => power(e.payload));
    void invoke<boolean>('power_get').then(power);
    const unUpdate = listen<UpdateStatus>('pets://update', e => setUpdate(e.payload));
    void invoke<UpdateStatus>('update_status').then(setUpdate);
    const unTab = listen<string>('settings://tab', e => { const t = tabFrom(e.payload); if (t) setTab(t); });
    const unLayout = listen<StageLayout>('pets://layout', e => { setLeftFallback(!!e.payload.left_fallback); setVerticalBar(!!e.payload.vertical_bar); });
    void invoke<StageLayout | null>('stage_layout').then(l => { setLeftFallback(!!l?.left_fallback); setVerticalBar(!!l?.vertical_bar); });
    const unHotkeys = listen<HotkeyStatus>('pets://hotkeys', e => setHotkeyStatus(e.payload));
    void invoke<HotkeyStatus>('hotkeys_status').then(setHotkeyStatus);
    return () => [un, unPower, unUpdate, unTab, unLayout, unHotkeys].forEach(p => void p.then(f => f()));
  }, [reload]);

  useEffect(() => { if (tab === 'diag' && inTauri) void invoke<Diagnostics>('diagnostics').then(setDiag); }, [tab]);
  useEffect(() => {
    if (tab !== 'stage') return;
    if (inTauri) void invoke<MonitorInfo[]>('monitors_list').then(setMonitors);
    else setMonitors([{ id: 'demo-1', primary: true, width: 2560, height: 1440, index: 1, has_bar: true }, { id: 'demo-2', primary: false, width: 1920, height: 1080, index: 2, has_bar: false }]);
  }, [tab]);

  if (!view) return null;

  if (view.first_run) {
    return <Wizard rows={rows} initial={view.settings}
      onFinish={s => (inTauri ? invoke<string[]>('wizard_finish', { settings: s }) : Promise.resolve([t().wizard.demoResult]))}
      onDone={() => void reload().then(() => setView(v => (v ? { ...v, first_run: false } : v)))} />;
  }

  const onChange = (s: Settings) => {
    setLang(resolveLang(s.language ?? 'auto'));
    applyTheme(s.theme);
    setView({ ...view, settings: s });
    setMessage(null);
    if (!inTauri) return;
    const relang = s.language !== view.settings.language;
    // integration descriptions and diagnostics arrive from Rust in the new language
    void invoke('settings_set', { settings: s }).then(() => { if (relang) void reload(); }).catch(e => setMessage(String(e)));
  };
  const onClaudeMod = async (on: boolean) => {
    if (!inTauri) { setView({ ...view, settings: { ...view.settings, claude_mod: on } }); return ''; }
    try {
      const m = await invoke<string>('claude_mod_set', { on });
      setMessage(m);
      return m;
    } catch (e) {
      setMessage(String(e));
      return String(e);
    } finally {
      await reload();
    }
  };
  const onIntegration = async (id: AppId, on: boolean) => {
    if (!inTauri) return '';
    try {
      const m = await invoke<string>('integration_set', { id, on });
      setMessage(m);
      return m;
    } catch (e) {
      setMessage(String(e));
      return String(e);
    } finally {
      await reload();
    }
  };

  return <SettingsView settings={view.settings} rows={rows} diag={diag} tab={tab} onTab={setTab} onChange={onChange}
    onIntegration={onIntegration} onClaudeMod={onClaudeMod} message={message} update={update} monitors={monitors} leftFallback={leftFallback} verticalBar={verticalBar} hotkeyStatus={hotkeyStatus}
    onMove={() => { if (inTauri) void invoke('stage_move'); }}
    onMute={choice => { if (inTauri) void invoke('notifications_mute', { choice }).catch(e => setMessage(String(e))); }}
    onReport={inTauri ? () => void invoke('report_problem_open') : undefined}
    onExport={inTauri ? () => invoke<string>('settings_export') : undefined}
    onImport={inTauri ? text => invoke<void>('settings_import', { text }) : undefined}
    onReveal={inTauri ? path => invoke<void>('settings_reveal', { path }) : undefined}
    onCheck={() => { if (inTauri) void invoke<UpdateStatus>('update_check').then(setUpdate); else setUpdate({ state: 'latest' }); }}
    onInstall={inTauri ? () => { void invoke<void>('update_install').catch((e: unknown) => setUpdate({ state: 'error', message: String(e), verify: true })); } : undefined} />;
}

createRoot(document.getElementById('root')!).render(<Root />);
