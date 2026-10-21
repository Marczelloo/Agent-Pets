import type { Limit, Media, Session, TooltipContent } from '../types';
import { mediaAppName } from '../stage/media';
import { listens } from '../stage/sceneFor';
import { LIMIT_AGENTS, clampPct, limitWindows, progressFraction, type LimitAgent } from '../stage/hud';
import { routerLine } from '../stage/router';
import { t, lang } from '../i18n';
import { agentLabel, hostLabel, modelLabel } from '../model-label';

const cut = (s: string, n: number) => ([...s].length <= n ? s : [...s].slice(0, n - 1).join('') + '…');
const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? '';
const pad = (n: number) => String(n).padStart(2, '0');

/** `media`: music is playing and the pet is listening → "Idle · Spotify playing", so dancing does not look like work. */
export function actionLabel(s: Pick<Session, 'state' | 'tool'>, media?: Media | null): string {
  if (s.state === 'working') return t().tool[s.tool ?? 'other'] ?? t().tool.other;
  const base = t().state[s.state] ?? t().tool.other;
  return media && listens(s, media.playing) ? `${base} · ${t().media.playing(mediaAppName(media.app))}` : base;
}

export function formatAgo(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 10) return t().time.now;
  if (s < 60) return t().time.secAgo(s);
  if (s < 3600) return t().time.minAgo(Math.floor(s / 60));
  return t().time.hourAgo(Math.floor(s / 3600));
}

/** Duration: "45 s", "3 min", "1 h 5 min". */
export function formatDuration(ms: number): string {
  const s = Math.max(0, Math.floor(ms / 1000));
  if (s < 60) return `${s} s`;
  const m = Math.floor(s / 60);
  return m < 60 ? `${m} min` : `${Math.floor(m / 60)} h ${m % 60} min`;
}

/** Local clock time: 15:40 in Polish, 3:40 PM in English. */
export function formatHm(ms: number): string {
  const d = new Date(ms);
  return lang() === 'pl' ? `${pad(d.getHours())}:${pad(d.getMinutes())}` : new Intl.DateTimeFormat('en-US', { hour: 'numeric', minute: '2-digit' }).format(d);
}

/** "At this pace: 100% around 3:40 PM"; the weekday joins when it is a day or more away. */
export function formatPace(runsOutAt: number, nowMs: number): string {
  const hm = formatHm(runsOutAt);
  return t().limits.pace(runsOutAt - nowMs < 86_400_000 ? hm : `${t().time.days[new Date(runsOutAt).getDay()]} ${hm}`);
}

export function formatReset(resetsAt: number | null, nowMs: number): string {
  if (resetsAt == null) return '';
  if (resetsAt <= nowMs) return t().time.resetSoon;
  const d = new Date(resetsAt);
  const hm = formatHm(resetsAt);
  return resetsAt - nowMs < 86_400_000 ? t().time.resetAt(hm) : t().time.resetDay(t().time.days[d.getDay()], hm);
}

export function petTooltip(s: Session, nowMs: number, media?: Media | null): TooltipContent {
  const lines = [s.action || actionLabel(s, media)];
  if (s.state === 'needs_you' && s.question) lines.push(cut(s.question, 200)); // what the agent asks, also with bubbles switched off
  const f = progressFraction(s.progress);
  if (f != null && s.progress) lines.push(t().tooltip.tasks(s.progress.done, s.progress.total));
  if (s.context && s.context.max > 0) lines.push(t().tooltip.context(Math.round(clampPct(s.context.used * 100 / s.context.max))));
  if (s.router_task) lines.push(routerLine(s.router_task, nowMs, s.last_activity));
  if (s.parent) lines.push(t().tooltip.runningFor(formatDuration(nowMs - s.started_at)));
  lines.push(t().tooltip.lastActivity(formatAgo(nowMs - s.last_activity)));
  return {
    title: cut(s.title || basename(s.cwd) || t().tooltip.untitled, 80),
    subtitle: [agentLabel(s), modelLabel(s.model), hostLabel(s)].filter(Boolean).join(' · '),
    lines,
  };
}

export const limitName = (a: LimitAgent) => a === 'claude' ? t().agent.limitClaude : t().agent[a];

export function limitsTooltip(limits: Limit[], nowMs: number): TooltipContent {
  const lines: string[] = [];
  for (const agent of LIMIT_AGENTS) for (const window of limitWindows(agent, limits)) {
    const l = limits.find(v => v.agent === agent && v.window === window);
    if (!l || !Number.isFinite(l.used_pct)) continue;
    const reset = formatReset(l.resets_at, nowMs);
    const note = l.stale_since != null ? t().limits.asOf(formatAgo(nowMs - l.stale_since)) : reset;
    lines.push(`${limitName(agent)} · ${t().window[window]}: ${Math.round(window === 'spend' ? l.used_pct : clampPct(l.used_pct))}%${note ? ` · ${note}` : ''}`);
  }
  if (limits.some(l => l.stale_since != null)) lines.push(t().limits.staleHint);
  return { title: t().limits.title, subtitle: '', lines };
}

export function badgeTooltip(hidden: Session[]): TooltipContent {
  const n = hidden.length;
  return {
    title: t().tooltip.moreSessions(n),
    subtitle: '',
    lines: hidden.slice(0, 6).map(s => `${cut(s.title || basename(s.cwd) || t().tooltip.untitled, 40)} · ${actionLabel(s)}`),
  };
}
