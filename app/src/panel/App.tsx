import { useEffect, useLayoutEffect, useRef, useState, type KeyboardEvent as ReactKeyEvent } from 'react';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { actionLabel, formatAgo, formatDuration, limitName, petTooltip } from '../tooltip/text';
import type { Media, NotificationEntry, Pets, RouterTask, Session, Settings, SettingsView, Snapshot, UpdateStatus } from '../types';
import { activeCount, childLabels, childLine, childMark, childrenOf, clock, collapseChildren, contextPct, contextText, hasInactive, limitCards, limitsAlert, notificationTime, panelSessions, progressText, renameKey, renameResult, sessionSubtitle, unreadCount, updateBar, usageLine, type PanelTab } from './model';
import { PetCanvas, setPetSaving } from './PetCanvas';
import { BellIcon, GearIcon, PinIcon, StatsIcon } from '../ui/icons';
import { appFor, defaultPets, lookFor } from '../look';
import { isLive, routerHealth, routerLine } from '../stage/router';
import { resolveLang, setLang, setSystemLang, t } from '../i18n';
import { hostLabel } from '../model-label';
import { appBadge, clockText, FOREVER } from '../settings/model';
import { HostIcon } from './HostIcon';
import { applyTheme } from '../theme';

/** Stable ref callback: focuses the first menu item once, when the menu opens (not on every re-render). */
const focusFirstItem = (el: HTMLElement | null) => { el?.querySelector<HTMLElement>('[role="menuitem"]')?.focus(); };

/** Inline rename on a card: the text starts selected; Enter or leaving the field saves, Escape cancels without closing the panel. */
const selectAll = (el: HTMLInputElement | null) => { el?.focus(); el?.select(); };
function RenameInput({ value, label, onDone }: { value: string; label: string; onDone: (name: string | null | undefined) => void }) {
  const [text, setText] = useState(value);
  const done = useRef(false);
  const finish = (save: boolean) => { if (done.current) return; done.current = true; onDone(save ? renameResult(text, value) : undefined); };
  return <input className="rename" type="text" value={text} maxLength={80} aria-label={label} ref={selectAll}
    onChange={e => setText(e.target.value)} onBlur={() => finish(true)}
    onKeyDown={e => {
      const k = renameKey(e.key);
      if (!k || e.nativeEvent.isComposing) return;
      e.preventDefault(); e.stopPropagation(); // Escape must not reach the window handler that hides the panel
      finish(k === 'save');
    }} />;
}

const routerHot = (t: RouterTask, nowMs: number, seenAt: number) =>
  isLive(t) && ['stalled', 'blocked'].includes(routerHealth(t, nowMs, seenAt));

interface JumpResult { method: string; detail: string }
interface ViewProps {
  snap: Snapshot; nowMs: number; status: string | null; focusId: string | null; onJump: (id: string) => void;
  /** hidden panel does not draw pets (WebView2 animates even in a hidden window) */
  animate?: boolean;
  onSettings?: () => void;
  onStats?: () => void;
  /** pet appearance from settings (style, motion, overrides) */
  pets?: Pets;
  update?: UpdateStatus;
  onInstall?: () => void;
  onDismiss?: (ids: string[]) => void;
  onDismissInactive?: () => void;
  /** recently hidden sessions for Undo (disappears after 6 s) */
  undo?: { ids: string[] } | null;
  onUndo?: () => void;
  /** what is playing in Windows (`null` when pets do not react to music) */
  media?: Media | null;
  /** notification center (newest first); the bell appears when the handlers are given */
  notifications?: NotificationEntry[];
  /** open the list right away (tests, deep links) */
  notificationsOpen?: boolean;
  onNotificationsSeen?: () => void;
  onNotificationOpen?: (n: NotificationEntry) => void;
  onNotificationRemove?: (id: number) => void;
  onNotificationsClear?: () => void;
  /** toasts are muted until then (the inbox says so, with a way to turn them back on) */
  muteUntil?: number | null;
  onUnmute?: () => void;
  initialTab?: PanelTab;
  /** copy a session folder (defaults to the clipboard) */
  onCopyPath?: (path: string) => void;
  /** own name for a session (`null` = back to the automatic one) and pinning; the ⋯ menu offers them when given */
  onRename?: (id: string, name: string | null) => void;
  onPin?: (id: string, pinned: boolean) => void;
  /** open a card's menu / rename field right away (tests) */
  initialMenu?: string | null;
  initialRenaming?: string | null;
}

/** Pure panel view: text through JSX only (React escapes characters), without `innerHTML`. */
export function PanelView({ snap, nowMs, status, focusId, onJump, animate = true, onSettings, onStats, pets = defaultPets(), update, onInstall, onDismiss, onDismissInactive, undo, onUndo, media = null,
  notifications = [], notificationsOpen = false, onNotificationsSeen, onNotificationOpen, onNotificationRemove, onNotificationsClear, muteUntil = null, onUnmute, initialTab = 'sessions', onCopyPath, onRename, onPin, initialMenu = null, initialRenaming = null }: ViewProps) {
  const [inbox, setInbox] = useState(notificationsOpen);
  const [tab, setTab] = useState<PanelTab>(initialTab);
  const [menu, setMenu] = useState<string | null>(initialMenu);
  const [renaming, setRenaming] = useState<string | null>(initialRenaming);
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(new Set());
  const unread = unreadCount(notifications);
  /** Arrow keys / Home / End move between the tabs and activate them (roving tabindex). */
  const tabKeys = (e: ReactKeyEvent<HTMLElement>) => {
    const order: PanelTab[] = ['sessions', 'limits'];
    const i = order.indexOf(tab);
    const n = e.key === 'ArrowRight' ? (i + 1) % order.length : e.key === 'ArrowLeft' ? (i + order.length - 1) % order.length
      : e.key === 'Home' ? 0 : e.key === 'End' ? order.length - 1 : -1;
    if (n < 0) return;
    e.preventDefault();
    setTab(order[n]);
    e.currentTarget.querySelectorAll<HTMLElement>('[role="tab"]')[n]?.focus();
  };
  /** In the ⋯ menu: arrows cycle the items, Escape closes it and returns focus to the ⋯ button. */
  const menuKeys = (e: ReactKeyEvent<HTMLElement>) => {
    const items = Array.from(e.currentTarget.querySelectorAll<HTMLElement>('[role="menuitem"]'));
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (e.key === 'Escape') { e.preventDefault(); const btn = e.currentTarget.parentElement?.querySelector<HTMLElement>('.more'); setMenu(null); btn?.focus(); return; }
    const n = e.key === 'ArrowDown' ? (at + 1) % items.length : e.key === 'ArrowUp' ? (at + items.length - 1) % items.length
      : e.key === 'Home' ? 0 : e.key === 'End' ? items.length - 1 : -1;
    if (n < 0) return;
    e.preventDefault();
    items[n]?.focus();
  };
  const toggleInbox = () => { setInbox(o => !o); if (!inbox && unread > 0) onNotificationsSeen?.(); };
  const sessions = panelSessions(snap.sessions);
  const bar = updateBar(update);
  // Update news stays on top of the inbox; Install only while that version is still waiting to be installed.
  const pinned = notifications.filter(n => n.kind === 'update' || n.kind === 'updated');
  const rest = notifications.filter(n => n.kind !== 'update' && n.kind !== 'updated');
  const openLabel = (n: NotificationEntry): string | null => n.kind === 'update' ? bar?.action ?? null
    : n.kind === 'problem' ? t().panel.notifications.diagnostics : n.session_id || n.kind === 'weekly' ? t().panel.open : null;
  const note = (n: NotificationEntry) => (
    <li key={n.id} className={`note ${n.kind}${n.read ? '' : ' unread'}`}>
      <div className="body">
        <div className="line1"><span className="kind">{t().panel.notifications.kind[n.kind]}</span><span className="when">{notificationTime(n.at, nowMs)}</span></div>
        <div className="title">{n.title}</div>
        {n.body && <div className="text">{n.body}</div>}
      </div>
      <div className="actions">
        {onNotificationOpen && openLabel(n) && <button type="button" onClick={() => onNotificationOpen(n)}>{openLabel(n)}</button>}
        {onNotificationRemove && <button type="button" className="remove" aria-label={t().panel.notifications.remove(n.title)}
          title={t().panel.notifications.remove(n.title)} onClick={() => onNotificationRemove(n.id)}>✕</button>}
      </div>
    </li>
  );
  const alert = limitsAlert(snap.limits);
  const cards = limitCards(snap.limits, nowMs, snap.forecasts);
  const toggleExpanded = (id: string) => setExpanded(prev => { const n = new Set(prev); if (!n.delete(id)) n.add(id); return n; });
  const copyPath = onCopyPath ?? ((p: string) => void navigator.clipboard?.writeText(p));

  // Esc closes the menu only (ahead of the window-level handler that hides the panel); a click elsewhere closes it too.
  useEffect(() => {
    if (menu == null) return;
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); setMenu(null); } };
    const away = (e: MouseEvent) => { if (!(e.target as Element | null)?.closest?.('.menu-wrap')) setMenu(null); };
    document.addEventListener('keydown', esc, true);
    document.addEventListener('mousedown', away);
    return () => { document.removeEventListener('keydown', esc, true); document.removeEventListener('mousedown', away); };
  }, [menu]);

  return (
    <div className="panel">
      <header>
        <h1>Agent Pets</h1>
        <div className="icons">
          {onNotificationsSeen && <button type="button" className={`icon-btn bell${inbox ? ' on' : ''}`} aria-label={unread ? `${t().panel.notifications.title}: ${t().panel.notifications.unread(unread)}` : t().panel.notifications.title}
            title={t().panel.notifications.title} aria-pressed={inbox} onClick={toggleInbox}>
            <BellIcon />{unread > 0 && <span className="badge">{unread > 9 ? '9+' : unread}</span>}
          </button>}
          {onStats && <button type="button" className="icon-btn" aria-label={t().panel.stats} title={t().panel.stats} onClick={onStats}><StatsIcon /></button>}
          {onSettings && <button type="button" className="icon-btn" aria-label={t().panel.settings} title={t().panel.settings} onClick={onSettings}><GearIcon /></button>}
        </div>
      </header>
      {bar && <div className="update-bar" role="status">
        <span className="text">{bar.text}</span>
        {bar.pct != null && <span className="bar" role="progressbar" aria-label={t().panel.update.progress}
          aria-valuemin={0} aria-valuemax={100} aria-valuenow={bar.pct}><i style={{ width: `${bar.pct}%` }} /></span>}
        {bar.action && onInstall && <button type="button" onClick={onInstall}>{bar.action}</button>}
      </div>}
      {inbox ? <section className="inbox" aria-label={t().panel.notifications.title}>
        <div className="tools">
          <button type="button" className="quiet" onClick={toggleInbox}>← {t().panel.notifications.back}</button>
          {notifications.length > 0 && onNotificationsClear && <button type="button" className="quiet" onClick={onNotificationsClear}>{t().panel.notifications.clear}</button>}
        </div>
        {muteUntil != null && muteUntil > nowMs && <p className="muted" role="status">
          {muteUntil >= FOREVER ? t().panel.notifications.mutedForever : t().panel.notifications.mutedUntil(clockText(muteUntil))}{onUnmute && <> · <button type="button" className="quiet" onClick={onUnmute}>{t().panel.notifications.unmute}</button></>}
        </p>}
        {notifications.length === 0 && <p className="empty">{t().panel.notifications.empty}</p>}
        {pinned.length > 0 && <ul className="pinned">{pinned.map(note)}</ul>}
        {rest.length > 0 && <ul>{rest.map(note)}</ul>}
      </section> : <>
      <div className="tabs">
        <div className="seg" role="tablist" onKeyDown={tabKeys}>
          <button type="button" role="tab" aria-selected={tab === 'sessions'} tabIndex={tab === 'sessions' ? 0 : -1} className={tab === 'sessions' ? 'on' : ''} onClick={() => setTab('sessions')}>
            {t().panel.sessions}<span className="n">{activeCount(snap.sessions)}</span>
          </button>
          <button type="button" role="tab" aria-selected={tab === 'limits'} tabIndex={tab === 'limits' ? 0 : -1} className={tab === 'limits' ? 'on' : ''} onClick={() => setTab('limits')}>
            {t().panel.limits}{alert && <i className="alert-dot" role="img" aria-label={t().panel.limitsAlert} />}
          </button>
        </div>
        {tab === 'sessions' && onDismissInactive && hasInactive(sessions) &&
          <button type="button" className="quiet clear" onClick={onDismissInactive}>{t().panel.removeInactive}</button>}
      </div>
      {tab === 'limits' ? <section className="limits" role="tabpanel" aria-label={t().panel.limits}>
        {cards.map(c => (
          <div className={c.stale ? 'lcard stale' : 'lcard'} key={c.agent}>
            <div className="lhead"><i className={`dot ${c.agent}`} /><span>{limitName(c.agent)}</span></div>
            {c.noData ? <p className="none">{t().panel.noData}</p> : c.rows.map(r => (
              <div className="limit" key={r.window}>
                <span className="label">{t().window[r.window]}</span>
                {r.pct == null ? <span className="none">{t().panel.noData}</span> : <>
                  <span className={`bar ${r.agent}`}><i style={{ width: `${Math.min(100, r.pct)}%` }} className={r.pct >= 90 ? 'hot' : ''} /></span>
                  <span className="pct">{Math.round(r.pct)}%</span>
                  <span className="reset">{r.reset}</span>
                  {r.pace && <span className="pace">{r.pace}</span>}
                </>}
              </div>
            ))}
          </div>
        ))}
        {cards.some(c => c.stale) && <p className="hint">{t().limits.staleHint}</p>}
      </section> : <section className="sessions" role="tabpanel" aria-label={t().panel.sessions}>
        {sessions.length === 0 && <div className="empty"><p>{t().panel.noSessions}</p><p className="hint">{t().panel.noSessionsHint}</p></div>}
        {sessions.map(s => {
          const kids = childrenOf(snap.sessions, s.id, nowMs);
          const { shown, hidden } = collapseChildren(kids, expanded.has(s.id));
          const title = petTooltip(s, nowMs).title;
          const ctx = contextPct(s);
          const usage = usageLine(s, snap.agent_usage?.find(a => a.agent === s.agent));
          return (<div key={s.id} className="group">
          <article id={`s-${s.id}`} className={`session ${s.state}${focusId === s.id ? ' focus' : ''}`}>
            {animate ? <PetCanvas session={s} look={lookFor(pets, appFor(s))} music={!!media?.playing} /> : <div className="pet" />}
            <div className="info">
              {renaming === s.id && onRename
                ? <RenameInput value={title} label={t().panel.renameField} onDone={name => { setRenaming(null); if (name !== undefined) onRename(s.id, name); }} />
                : <div className="titlerow">
                  <button type="button" className="title open" aria-label={`${t().panel.open}: ${title}`} onClick={() => onJump(s.id)}>{title}</button>
                  {s.pinned && <span className="pin" role="img" aria-label={t().panel.pinned}><PinIcon /></span>}
                </div>}
              <div className="sub">{s.jump.app && hostLabel(s) && <HostIcon app={s.jump.app} />}{sessionSubtitle(s)}</div>
              {s.state === 'needs_you' && s.question && <div className="question">{s.question}</div>}
              <div className="meta">
                <span className="state">{actionLabel(s, media)}</span>
                {appBadge(appFor(s)) && <span className="exp">{appBadge(appFor(s))}</span>}
                {progressText(s) && <span>{t().panel.tasks} {progressText(s)}</span>}
                {s.router_task && <span className={routerHot(s.router_task, nowMs, s.last_activity) ? 'router-hot' : undefined}>
                  {routerLine(s.router_task, nowMs, s.last_activity)}</span>}
                <span>{formatAgo(nowMs - s.last_activity)}</span>
              </div>
              {usage && <div className="usage">{usage}</div>}
            </div>
            <div className="menu-wrap">
              <button type="button" className="icon-btn more" aria-label={`${t().panel.menu}: ${title}`} aria-haspopup="menu" aria-expanded={menu === s.id}
                onClick={() => setMenu(m => (m === s.id ? null : s.id))}>⋯</button>
              {menu === s.id && <div className="menu" role="menu" onKeyDown={menuKeys} ref={focusFirstItem}>
                <button type="button" role="menuitem" onClick={() => { setMenu(null); onJump(s.id); }}>{t().panel.open}</button>
                {onRename && <button type="button" role="menuitem" onClick={() => { setMenu(null); setRenaming(s.id); }}>{t().panel.rename}</button>}
                {onRename && s.renamed && <button type="button" role="menuitem" onClick={() => { setMenu(null); onRename(s.id, null); }}>{t().panel.resetName}</button>}
                {onPin && <button type="button" role="menuitem" onClick={() => { setMenu(null); onPin(s.id, !s.pinned); }}>{s.pinned ? t().panel.unpin : t().panel.pin}</button>}
                {s.cwd && <button type="button" role="menuitem" onClick={() => { setMenu(null); copyPath(s.cwd); }}>{t().panel.copyPath}</button>}
                {onDismiss && <button type="button" role="menuitem" title={t().panel.remove(title)} onClick={() => { setMenu(null); onDismiss([s.id]); }}>
                  {t().panel.remove(title)}</button>}
              </div>}
            </div>
            {ctx != null && <span className="ctx" role="meter" aria-label={t().panel.context} aria-valuemin={0} aria-valuemax={100} aria-valuenow={ctx} aria-valuetext={contextText(s) ?? undefined}
              title={`${t().panel.context} ${contextText(s)}`} style={{ width: `${ctx}%` }} />}
          </article>
          {kids.length > 0 && <ul className="kids" aria-label={t().panel.subagents(kids.length)}>
            {shown.map(c => <SubagentRow key={c.id} c={c} nowMs={nowMs} focus={focusId === c.id} animate={animate} pets={pets} />)}
            {(hidden > 0 || expanded.has(s.id)) && <li className="more-kids">
              <button type="button" className="quiet" aria-expanded={expanded.has(s.id)} onClick={() => toggleExpanded(s.id)}>
                {expanded.has(s.id) ? t().panel.fewerSubagents : t().panel.moreSubagents(hidden)}</button>
            </li>}
          </ul>}
        </div>); })}
      </section>}</>}
      {undo && undo.ids.length > 0 && <footer className="undo" role="status">
        <span>{t().panel.removed(undo.ids.length)}</span>
        {onUndo && <button type="button" onClick={onUndo}>{t().panel.undo}</button>}
      </footer>}
      {status && <footer className="status" role="status">{status}</footer>}
    </div>
  );
}

/** Subagent on a parent card: tree branch, mini pet, title with labels, action, and working time. */
function SubagentRow({ c, nowMs, focus, animate, pets }: { c: Session; nowMs: number; focus: boolean; animate: boolean; pets: Pets }) {
  const mark = childMark(c);
  return (
    <li id={`s-${c.id}`} className={`kid ${c.state}${focus ? ' focus' : ''}`}>
      <span className="branch" aria-hidden="true" />
      {animate ? <PetCanvas session={c} look={lookFor(pets, appFor(c))} mini /> : <div className="pet mini" />}
      <div className="body">
        <div className="line1">
          <span className="title">{petTooltip(c, nowMs).title}</span>
          {childLabels(c, nowMs).map(l => <span key={l.kind} className={`chip ${l.kind}`}>{l.text}</span>)}
        </div>
        <div className={`act ${mark}`}><span className="mark" aria-hidden="true">{mark === 'ok' ? '✓' : mark === 'err' ? '!' : ''}</span>{childLine(c)}</div>
      </div>
      <span className="time" title={t().panel.child.runningFor(formatDuration(nowMs - c.started_at))}>{clock(nowMs - c.started_at)}</span>
    </li>
  );
}

export default function App() {
  const [snap, setSnap] = useState<Snapshot>({ sessions: [], limits: [], now: 0 });
  const [pets, setPets] = useState<Pets>(defaultPets);
  const [offset, setOffset] = useState(0);
  const [status, setStatus] = useState<string | null>(null);
  const [focusId, setFocusId] = useState<string | null>(null);
  const [shown, setShown] = useState(false);
  const [update, setUpdate] = useState<UpdateStatus>({ state: 'idle' });
  const [media, setMedia] = useState<Media>({ playing: false, app: null });
  const [notes, setNotes] = useState<NotificationEntry[]>([]);
  const [muteUntil, setMuteUntil] = useState<number | null>(null);
  const [undo, setUndo] = useState<{ ids: string[] } | null>(null);
  const undoTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const removed = (ids: string[]) => {
    clearTimeout(undoTimer.current);
    setUndo(ids.length ? { ids } : null);
    undoTimer.current = setTimeout(() => setUndo(null), 6000);
  };
  const [, tick] = useState(0);
  const take = useRef((s: Snapshot) => { setSnap(s); setOffset(s.now - Date.now()); });
  // the open/close transition (panel.html): set before paint, so a freshly shown window starts from the faded-out frame
  useLayoutEffect(() => { document.documentElement.dataset.panel = shown ? 'open' : 'closed'; }, [shown]);

  useEffect(() => {
    let unfocus: ReturnType<typeof setTimeout> | undefined;
    const un = [
      listen<Snapshot>('pets://snapshot', e => take.current(e.payload)),
      listen<string>('panel://status', e => setStatus(e.payload)),
      // opening the panel also drops notifications that have grown stale (core prunes them on every list)
      listen<boolean>('panel://visible', e => { setShown(e.payload); if (e.payload) void invoke<NotificationEntry[]>('notifications_list').then(setNotes); }),
      listen<Settings>('pets://settings', e => { setLang(resolveLang(e.payload.language ?? 'auto')); applyTheme(e.payload.theme); setPets(e.payload.pets); setMuteUntil(e.payload.notifications?.muted_until ?? null); }),
      listen<boolean>('pets://power', e => setPetSaving(e.payload)),
      listen<UpdateStatus>('pets://update', e => setUpdate(e.payload)),
      listen<Media>('pets://media', e => setMedia(e.payload)),
      listen<NotificationEntry[]>('pets://notifications', e => setNotes(e.payload)),
      listen<string>('panel://focus', e => {
        setStatus(null);
        setFocusId(e.payload);
        requestAnimationFrame(() => document.getElementById(`s-${e.payload}`)?.scrollIntoView({ block: 'nearest' }));
        clearTimeout(unfocus);
        unfocus = setTimeout(() => setFocusId(null), 2000);
      }),
    ];
    void invoke<Snapshot>('snapshot').then(s => take.current(s));
    void invoke<SettingsView>('settings_get').then(v => {
      setSystemLang(v.system_lang);
      setLang(resolveLang(v.settings.language ?? 'auto'));
      applyTheme(v.settings.theme);
      setPets(v.settings.pets);
      setMuteUntil(v.settings.notifications?.muted_until ?? null);
    });
    void invoke<boolean>('power_get').then(saving => setPetSaving(saving));
    void invoke<UpdateStatus>('update_status').then(setUpdate);
    void invoke<Media>('media_get').then(setMedia);
    void invoke<NotificationEntry[]>('notifications_list').then(setNotes);
    const t = setInterval(() => tick(n => n + 1), 1000);
    const esc = (e: KeyboardEvent) => { if (e.key === 'Escape') void invoke('panel_hide'); };
    addEventListener('keydown', esc);
    // the clear margin around the panel (room for its shadow) is not the panel: a press there closes it
    const outside = (e: MouseEvent) => { if (!(e.target as Element | null)?.closest?.('.panel')) void invoke('panel_hide'); };
    addEventListener('mousedown', outside);
    return () => { clearInterval(t); clearTimeout(unfocus); removeEventListener('keydown', esc); removeEventListener('mousedown', outside);un.forEach(p => void p.then(f => f())); };
  }, []);

  const onJump = async (sessionId: string) => {
    const r = await invoke<JumpResult>('jump', { sessionId });
    // successful navigation hides the panel (handled by the command); clipboard and failure need to be read
    setStatus(r.method === 'clipboard' || r.method === 'none' ? r.detail : null);
  };

  return <PanelView snap={snap} nowMs={Date.now() + offset} status={status} focusId={focusId} onJump={onJump} animate={shown} pets={pets} media={pets.react_to_media !== false ? media : null}
    onSettings={() => void invoke('settings_open')} onStats={() => void invoke('stats_open')} update={update}
    onInstall={() => void invoke('update_install').catch(e => setStatus(String(e)))}
    onDismiss={ids => void invoke<string[]>('session_dismiss', { ids }).then(removed)}
    onDismissInactive={() => void invoke<string[]>('sessions_dismiss_inactive').then(removed)}
    onRename={(id, name) => void invoke('session_rename', { id, name })} onPin={(id, pinned) => void invoke('session_pin', { id, pinned })}
    notifications={notes} onNotificationsSeen={() => void invoke('notifications_read')}
    onNotificationOpen={n => { if (n.session_id) void onJump(n.session_id); else if (n.kind === 'update') void invoke('update_install').catch(e => setStatus(String(e))); else if (n.kind === 'problem') void invoke('settings_open_tab', { tab: 'diag' }); else if (n.kind === 'weekly') void invoke('stats_open'); }}
    onNotificationRemove={id => void invoke('notification_remove', { id })} onNotificationsClear={() => void invoke('notifications_clear')}
    muteUntil={muteUntil} onUnmute={() => void invoke('notifications_mute', { choice: 'off' })}
    undo={undo} onUndo={() => { if (undo) void invoke('session_undismiss', { ids: undo.ids }); clearTimeout(undoTimer.current); setUndo(null); }} />;
}
