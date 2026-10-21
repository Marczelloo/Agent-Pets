import { LIMIT_AGENTS, clampPct, limitWindows, progressFraction, type LimitAgent } from '../stage/hud';
import { actionLabel, formatAgo, formatPace, formatReset, limitName } from '../tooltip/text';
import { isLive, routerHealth } from '../stage/router';
import type { AgentUsage, Forecast, Limit, NotificationEntry, Session, UpdateStatus } from '../types';
import { formatTokens } from '../stats/model';
import { t } from '../i18n';
import { agentLabel, hostLabel, modelLabel } from '../model-label';

const URGENT = new Set(['needs_you', 'error']);

/** Sessions waiting for you or in error first, then pinned ones, then by latest activity. Children appear under their parent. */
export function panelSessions(sessions: Session[]): Session[] {
  return sessions.filter(s => !s.parent).sort((a, b) =>
    Number(URGENT.has(b.state)) - Number(URGENT.has(a.state)) || Number(!!b.pinned) - Number(!!a.pinned)
    || b.last_activity - a.last_activity || (a.id < b.id ? -1 : 1));
}

/** What an inline rename does with a key: Enter saves, Escape cancels, anything else keeps typing. */
export type RenameKey = 'save' | 'cancel' | null;
export const renameKey = (key: string): RenameKey => key === 'Enter' ? 'save' : key === 'Escape' ? 'cancel' : null;

/** Name to send after an inline rename: trimmed text, `null` (empty = back to the automatic title), or `undefined` when nothing changed. */
export function renameResult(typed: string, current: string): string | null | undefined {
  const name = typed.trim();
  return name === current ? undefined : name || null;
}

const INACTIVE = new Set(['idle', 'done', 'sleep', 'ended']);
/** Like core `dismiss::inactive`: this removes "Clear inactive" (children disappear with their parent, not separately; pinned stay). */
export const hasInactive = (sessions: Session[]): boolean => sessions.some(s => !s.parent && !s.pinned && INACTIVE.has(s.state));

export type PanelTab = 'sessions' | 'limits';

/** Number on the Sessions tab: top-level sessions that are not inactive. */
export const activeCount = (sessions: Session[]): number => sessions.filter(s => !s.parent && !INACTIVE.has(s.state)).length;

/** Warning dot on the Limits tab: a reading at 80 % or more, or a stale one. */
export const limitsAlert = (limits: Limit[]): boolean =>
  limits.some(l => l.stale_since != null || (Number.isFinite(l.used_pct) && l.used_pct >= 80));

export const COLLAPSE_AFTER = 3;
/** Subagents under a card: more than three fold into "+N subagents" until expanded. */
export function collapseChildren<T>(children: T[], expanded: boolean): { shown: T[]; hidden: number } {
  if (expanded || children.length <= COLLAPSE_AFTER) return { shown: children, hidden: 0 };
  return { shown: children.slice(0, COLLAPSE_AFTER), hidden: children.length - COLLAPSE_AFTER };
}

/** A finished child disappears from the panel after this time. */
export const CHILD_DONE_MS = 10_000;
const FINISHED = new Set(['done', 'error', 'ended']);

/** Session children (subagents, router tasks) oldest first; finished ones remain for 10 s. */
export function childrenOf(sessions: Session[], id: string, nowMs: number): Session[] {
  return sessions.filter(c => c.parent === id && !(FINISHED.has(c.state) && nowMs - c.state_since >= CHILD_DONE_MS))
    .sort((a, b) => a.started_at - b.started_at || (a.id < b.id ? -1 : 1));
}

export interface UpdateBar { text: string; action: string | null; pct: number | null }

/** Bar above the session list: only when an update needs attention (or has an error). */
export function updateBar(u: UpdateStatus | undefined): UpdateBar | null {
  const x = t().panel.update;
  switch (u?.state) {
    case 'available': return { text: x.available(u.version), action: x.install, pct: null };
    case 'downloading': return { text: x.downloading(u.version), action: null, pct: u.pct ?? 0 };
    case 'ready': return { text: x.ready(u.version), action: x.installNow, pct: null };
    case 'error': return u.verify ? { text: u.message, action: null, pct: null } : null;
    default: return null;
  }
}

/** Usage line on a card: session and daily tokens, session cost only above 0 (spec 0.11 §4.2). */
export function usageLine(s: Session, today: AgentUsage | undefined): string | null {
  if (!s.usage) return null;
  const u = t().panel.usage;
  const parts = s.usage.tokens > 0 ? [u.session(formatTokens(s.usage.tokens))] : [];
  if (today) parts.push(u.today(formatTokens(today.tokens_today)));
  if (s.usage.cost > 0) parts.push(u.cost(s.usage.cost));
  return parts.length ? parts.join(' · ') : null;
}

export interface LimitRow { agent: LimitAgent; window: Limit['window']; label: string; pct: number | null; reset: string; stale: boolean; pace: string | null }

/** Claude and Codex keep their 5h and weekly rows (or the monthly one on a free plan); spend and Antigravity need readings. */
export function limitRows(limits: Limit[], nowMs: number, forecasts: Forecast[] = []): LimitRow[] {
  const rows: LimitRow[] = [];
  for (const agent of LIMIT_AGENTS) for (const window of limitWindows(agent, limits)) {
    const l = limits.find(v => v.agent === agent && v.window === window);
    const ok = l != null && Number.isFinite(l.used_pct);
    if (!ok && (agent === 'antigravity' || window === 'spend')) continue;
    const f = ok && l!.stale_since == null && (window !== 'spend' || l!.resets_at != null) ? forecasts.find(v => v.agent === agent && v.window === window) : undefined;
    rows.push({
      agent, window, label: `${limitName(agent)} · ${t().window[window]}`,
      pct: ok ? window === 'spend' ? Math.max(0, l!.used_pct) : clampPct(l!.used_pct) : null,
      reset: !ok ? '' : l!.stale_since != null ? t().limits.asOf(formatAgo(nowMs - l!.stale_since)) : formatReset(l!.resets_at, nowMs),
      stale: ok && l!.stale_since != null,
      pace: f && f.runs_out_at > nowMs ? formatPace(f.runs_out_at, nowMs) : null,
    });
  }
  return rows;
}

const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? '';

export function sessionSubtitle(s: Session): string {
  return [agentLabel(s), modelLabel(s.model), hostLabel(s), basename(s.cwd)].filter(Boolean).join(' · ');
}

export function progressText(s: Session): string | null {
  return progressFraction(s.progress) == null || !s.progress ? null : `${s.progress.done}/${s.progress.total}`;
}

/** Fill of the thin context bar on a card. */
export const contextPct = (s: Session): number | null => s.context && s.context.max > 0 ? clampPct(s.context.used * 100 / s.context.max) : null;

export function contextText(s: Session): string | null {
  return s.context && s.context.max > 0 ? `${Math.round(clampPct(s.context.used * 100 / s.context.max))}%` : null;
}

export interface ChildLabel { kind: 'type' | 'router' | 'bg' | 'warn'; text: string }

/** Subagent labels: type (unless already the title), "Router", "in background", and a stalled router-task warning. */
export function childLabels(c: Session, nowMs: number): ChildLabel[] {
  const out: ChildLabel[] = [];
  const x = t().panel.child;
  if (c.sub?.kind === 'router') out.push({ kind: 'router', text: x.router });
  else if (c.sub?.agent_type && c.sub.agent_type !== c.title) out.push({ kind: 'type', text: c.sub.agent_type });
  if (c.sub?.background) out.push({ kind: 'bg', text: x.background });
  if (c.router_task && isLive(c.router_task)) {
    const h = routerHealth(c.router_task, nowMs, c.last_activity);
    if (h === 'stalled' || h === 'blocked') out.push({ kind: 'warn', text: t().router.health[h] });
  }
  return out;
}

export type ChildMark = 'run' | 'ok' | 'err' | 'idle';
export function childMark(c: Session): ChildMark {
  if (c.state === 'error') return 'err';
  if (c.state === 'done' || c.state === 'ended') return 'ok';
  return c.state === 'idle' || c.state === 'sleep' ? 'idle' : 'run';
}

/** Second row line: current action, then result on completion ("Finished" also for `ended`). */
export function childLine(c: Session): string {
  switch (childMark(c)) {
    case 'ok': return t().state.done;
    case 'err': return t().state.error;
    default: return c.action || actionLabel(c);
  }
}

/** Working time in clock format: 0:09, 1:12, 1:02:03. */
export function clock(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000)), p = (n: number) => String(n).padStart(2, '0');
  const h = Math.floor(s / 3600), m = Math.floor(s / 60) % 60;
  return h ? `${h}:${p(m)}:${p(s % 60)}` : `${m}:${p(s % 60)}`;
}

/** Unread count for the bell badge. */
export const unreadCount = (items: readonly NotificationEntry[]): number => items.filter(e => !e.read).length;

/** Time of a notification: "5 min ago" for the first day, then the date. */
export function notificationTime(at: number, nowMs: number): string {
  const d = nowMs - at;
  if (d < 24 * 3_600_000) return formatAgo(Math.max(0, d));
  return new Date(at).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

export interface AgentLimitCard { agent: LimitAgent; rows: LimitRow[]; noData: boolean; stale: boolean }

/** One card per agent on the Limits tab; `noData` when none of its rows has a reading. */
export function limitCards(limits: Limit[], nowMs: number, forecasts: Forecast[] = []): AgentLimitCard[] {
  const rows = limitRows(limits, nowMs, forecasts);
  return LIMIT_AGENTS.map(agent => {
    const mine = rows.filter(r => r.agent === agent);
    return { agent, rows: mine, noData: mine.every(r => r.pct == null), stale: mine.some(r => r.stale) };
  }).filter(c => c.rows.length > 0);
}
