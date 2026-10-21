import type { Limit, Session } from '../types';
import { BADGE_W } from './layout';
import { BEAT } from '../renderer/scenes';

export const AGENT_COLOR: Record<string, string> = { claude: '#D97757', codex: '#5DCAA5', antigravity: '#3DDC84' };
const TRACK = 'rgba(128,128,128,0.30)';
const HOT = '#E24B4A';
const ACTIVE = new Set(['thinking', 'working', 'compacting', 'needs_you']);

export const clampPct = (v: number) => (Number.isFinite(v) ? Math.min(100, Math.max(0, v)) : 0);

export function progressFraction(p: Session['progress']): number | null {
  if (!p || !(p.total > 0)) return null;
  return Math.min(1, Math.max(0, p.done / p.total));
}

/**
 * Progress bar under the pet: 28×2 px; without a task list, it pulses while the session works.
 * `music`: the pet listens to music; gray equalizer bars replace the progress bar (gray = idle, not working).
 */
export function drawProgress(x: CanvasRenderingContext2D, cx: number, y: number, s: Session, t: number, music: EqMode = null): void {
  if (music) { drawEq(x, cx, y, t, music); return; }
  const col = AGENT_COLOR[s.agent] ?? AGENT_COLOR.claude;
  const f = progressFraction(s.progress);
  x.save();
  x.fillStyle = TRACK;
  x.fillRect(cx - 14, y, 28, 2);
  if (f != null) {
    x.fillStyle = col;
    x.fillRect(cx - 14, y, 28 * f, 2);
  } else if (ACTIVE.has(s.state)) {
    x.globalAlpha = .25 + .45 * (.5 + .5 * Math.sin(t * 4));
    x.fillStyle = col;
    x.fillRect(cx - 14, y, 28, 2);
  }
  x.restore();
}

export type EqMode = 'dance' | 'doze' | null;
const EQ = [[1, 0], [1.5, 1.3], [0.75, 2.2], [1.25, 0.6]] as const;

/** EQ bar heights (px, 1–5): dancing bounces to the beat; dozing barely moves. */
export function eqHeights(t: number, mode: 'dance' | 'doze'): number[] {
  const amp = mode === 'dance' ? 4 : 1.5, sp = mode === 'dance' ? 1 : 0.35;
  return EQ.map(([k, ph]) => 1 + amp * Math.abs(Math.sin(t * Math.PI * BEAT * k * sp + ph)));
}

function drawEq(x: CanvasRenderingContext2D, cx: number, y: number, t: number, mode: 'dance' | 'doze'): void {
  x.save();
  x.fillStyle = mode === 'dance' ? 'rgba(150,150,150,0.8)' : 'rgba(140,140,140,0.6)';
  eqHeights(t, mode).forEach((h, i) => x.fillRect(cx - 13 + i * 7, y + 2 - h, 5, h));
  x.restore();
}

/** Badge warning color: router task is stalled or blocked. */
const WARN = '#EF9F27';

/** Agent Router badge: circle in Codex (or warning) color with opposing arrows, above the left arm. */
export function drawRouterBadge(x: CanvasRenderingContext2D, cx: number, y: number, warn = false): void {
  const r = 4.5, bx = cx - 17, by = y;
  x.save();
  x.beginPath();
  x.arc(bx, by, r, 0, Math.PI * 2);
  x.fillStyle = warn ? WARN : AGENT_COLOR.codex;
  x.fill();
  x.lineWidth = 1;
  x.strokeStyle = '#2B1D16';
  x.stroke();
  x.beginPath();
  x.lineCap = 'round';
  x.strokeStyle = '#FFFFFF';
  x.lineWidth = 1.1;
  // upper arrow points right, lower arrow points left
  x.moveTo(bx - 2.2, by - 1.2); x.lineTo(bx + 2.2, by - 1.2); x.lineTo(bx + 1, by - 2.4);
  x.moveTo(bx + 2.2, by + 1.2); x.lineTo(bx - 2.2, by + 1.2); x.lineTo(bx - 1, by + 2.4);
  x.stroke();
  x.restore();
}

export type LimitAgent = 'claude' | 'codex' | 'antigravity';
export const LIMIT_AGENTS: readonly LimitAgent[] = ['claude', 'codex', 'antigravity'];

/** An agent's usage windows: 5h and weekly (plus spend for Claude), or a single monthly one on a free plan (Codex). */
export function limitWindows(agent: LimitAgent, limits: Limit[], spend = true): Limit['window'][] {
  const extra: Limit['window'][] = agent === 'claude' && spend ? ['spend'] : [];
  return limits.some(l => l.agent === agent && l.window === 'monthly') ? ['monthly', ...extra] : ['five_hour', 'weekly', ...extra];
}

export interface LimitBar { agent: LimitAgent; window: 'five_hour' | 'weekly' | 'monthly'; pct: number; stale: boolean }

export function limitBars(limits: Limit[]): LimitBar[] {
  const out: LimitBar[] = [];
  for (const agent of LIMIT_AGENTS) for (const window of limitWindows(agent, limits, false) as LimitBar['window'][]) {
    const l = limits.find(v => v.agent === agent && v.window === window);
    if (l && Number.isFinite(l.used_pct)) out.push({ agent, window, pct: clampPct(l.used_pct), stale: l.stale_since != null });
  }
  return out;
}

/** Vertical 3 px bars: 5h in full color, week dimmed; a larger gap separates agent groups. */
export function drawLimits(x: CanvasRenderingContext2D, lx: number, h: number, bars: LimitBar[]): void {
  const top = 8, H = h - 16;
  let px = lx + 3;
  let prev: string | null = null;
  for (const b of bars) {
    if (prev && prev !== b.agent) px += 3;
    x.save();
    x.fillStyle = TRACK;
    x.fillRect(px, top, 3, H);
    x.globalAlpha = (b.window === 'weekly' ? .6 : 1) * (b.stale ? .5 : 1);
    x.fillStyle = b.pct >= 90 ? HOT : AGENT_COLOR[b.agent];
    const fh = H * b.pct / 100;
    x.fillRect(px, top + H - fh, 3, fh);
    x.restore();
    px += 5;
    prev = b.agent;
  }
}

export function drawBadge(x: CanvasRenderingContext2D, bx: number, h: number, hidden: number, font: string): void {
  const w = BADGE_W - 4, hh = 16, y = (h - hh) / 2;
  x.save();
  x.fillStyle = 'rgba(128,128,128,0.35)';
  x.beginPath();
  x.roundRect(bx + 2, y, w, hh, 5);
  x.fill();
  x.fillStyle = '#FAF9F5';
  x.font = `600 10px ${font}`;
  x.textAlign = 'center';
  x.textBaseline = 'middle';
  x.fillText(`+${hidden}`, bx + 2 + w / 2, y + hh / 2 + .5);
  x.restore();
}

/** "+N" children between a parent and its minis: a small pill level with the minis' bodies (coordinates at 1×, `h` = 48). */
export function drawMiniMore(x: CanvasRenderingContext2D, cx: number, h: number, n: number, font: string): void {
  const w = 14, hh = 11, y = h - 15 - hh / 2;
  x.save();
  x.fillStyle = 'rgba(128,128,128,0.35)';
  x.beginPath();
  x.roundRect(cx - w / 2, y, w, hh, 4);
  x.fill();
  x.fillStyle = '#FAF9F5';
  x.font = `600 8px ${font}`;
  x.textAlign = 'center';
  x.textBaseline = 'middle';
  x.fillText(`+${n}`, cx, y + hh / 2 + .5);
  x.restore();
}
