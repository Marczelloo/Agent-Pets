import { describe, expect, it } from 'vitest';
import type { Session, State } from '../types';
import { setLang } from '../i18n';
import { limitBars } from '../stage/hud';
import { CHILD_DONE_MS, activeCount, collapseChildren, contextPct, limitCards, limitsAlert, childLabels, childLine, childMark, childrenOf, clock, contextText, hasInactive, limitRows, panelSessions, progressText, renameKey, renameResult, sessionSubtitle, usageLine } from './model';

const s = (id: string, state: State, last: number): Session => ({
  id, agent: 'claude', origin: 'cli', title: id, cwd: 'C:\\work\\' + id, state, tool: null, progress: null, context: null,
  started_at: 0, last_activity: last, state_since: 0, turn_started_at: null, jump: { pid: null, session_id: id, cwd: '', app: null },
});

describe('opencode usage on its card', () => {
  const oc = { ...s('o', 'working', 0), agent: 'opencode' as const };
  const today = { agent: 'opencode' as const, tokens_today: 4_800_000, cost_today: 0 };
  it('shows session and day tokens, and the cost only when there is one', () => {
    expect(usageLine({ ...oc, usage: { tokens: 1_200_000, cost: 0, account: null } }, today)).toBe('sesja: 1,2 mln tok. · dziś: 4,8 mln tok.');
    expect(usageLine({ ...oc, usage: { tokens: 950, cost: 0.42, account: null } }, { ...today, cost_today: 1.5 }))
      .toBe('sesja: 950 tok. · dziś: 4,8 mln tok. · 0,42 $');
    expect(usageLine({ ...oc, usage: { tokens: 950, cost: 0, account: null } }, undefined)).toBe('sesja: 950 tok.');
  });
  it('leaves out the token part of a session that only has a cost (Claude)', () => {
    const line = usageLine({ ...s('c', 'working', 0), usage: { tokens: 0, cost: 1.42, account: 'claude' } }, undefined);
    expect(line).toBe('1,42 $');
    expect(line).not.toMatch(/tok/);
    expect(usageLine({ ...s('c', 'working', 0), usage: { tokens: 0, cost: 0, account: 'claude' } }, undefined)).toBeNull();
  });
  it('writes the cost the way the UI language does', () => {
    setLang('en');
    try {
      expect(usageLine({ ...oc, usage: { tokens: 950, cost: 0.42, account: null } }, undefined)).toBe('session: 950 tokens · $0.42');
    } finally { setLang('pl'); }
  });
  it('has no line without usage', () => {
    expect(usageLine(oc, today)).toBeNull();
    expect(usageLine({ ...s('c', 'working', 0), usage: null }, today)).toBeNull();
  });
});

describe('pinning and renaming', () => {
  it('sorts pinned sessions after the ones that need you and before the rest', () => {
    const pin = (x: Session): Session => ({ ...x, pinned: true });
    const out = panelSessions([s('a', 'working', 50), pin(s('p', 'idle', 1)), s('n', 'needs_you', 2), pin(s('q', 'working', 9)), s('e', 'error', 1)]);
    expect(out.map(x => x.id)).toEqual(['n', 'e', 'q', 'p', 'a']);
  });
  it('"clear inactive" does not count a pinned session', () => {
    expect(hasInactive([{ ...s('a', 'idle', 0), pinned: true }])).toBe(false);
    expect(hasInactive([{ ...s('a', 'idle', 0), pinned: true }, s('b', 'done', 0)])).toBe(true);
  });
  it('Enter saves, Escape cancels, other keys keep typing', () => {
    expect([renameKey('Enter'), renameKey('Escape'), renameKey('a'), renameKey('Tab')]).toEqual(['save', 'cancel', null, null]);
  });
  it('sends the trimmed text, null for an empty field, and nothing when the title did not change', () => {
    expect(renameResult('  My work  ', 'auto')).toBe('My work');
    expect(renameResult('   ', 'auto')).toBeNull();
    expect(renameResult('auto', 'auto')).toBeUndefined();
    expect(renameResult(' auto ', 'auto')).toBeUndefined();
  });
});

describe('panel model', () => {
  it('puts sessions that need you first, then the most recently active', () => {
    const out = panelSessions([s('a', 'working', 10), s('b', 'needs_you', 1), s('c', 'idle', 30), s('d', 'error', 2)]);
    expect(out.map(x => x.id)).toEqual(['d', 'b', 'c', 'a']);
  });
  it('always shows four limit rows and never invents 0%', () => {
    const now = new Date(2026, 8, 24, 12, 0).getTime();
    const rows = limitRows([{ agent: 'codex', window: 'weekly', used_pct: 91, resets_at: null }], now);
    expect(rows.map(r => `${r.agent}/${r.window}`)).toEqual(['claude/five_hour', 'claude/weekly', 'codex/five_hour', 'codex/weekly']);
    expect(rows[0].pct).toBeNull();
    expect(rows[3].pct).toBe(91);
  });
  it('shows a single monthly row for Codex on a free plan', () => {
    const now = new Date(2026, 8, 24, 12, 0).getTime();
    const rows = limitRows([{ agent: 'codex', window: 'monthly', used_pct: 7, resets_at: null }], now);
    expect(rows.map(r => `${r.agent}/${r.window}`)).toEqual(['claude/five_hour', 'claude/weekly', 'codex/monthly']);
    expect(rows[2]).toMatchObject({ pct: 7, label: 'Codex · miesiąc' });
    expect(limitBars([{ agent: 'codex', window: 'monthly', used_pct: 7, resets_at: null }]).map(b => b.window)).toEqual(['monthly']);
  });
  it('adds Antigravity rows only for windows it reported', () => {
    const now = new Date(2026, 8, 24, 12, 0).getTime();
    const rows = limitRows([{ agent: 'antigravity', window: 'five_hour', used_pct: 20, resets_at: null }], now);
    expect(rows.map(r => `${r.agent}/${r.window}`)).toEqual(['claude/five_hour', 'claude/weekly', 'codex/five_hour', 'codex/weekly', 'antigravity/five_hour']);
    expect(rows[4].label).toMatch(/^Antigravity · /);
    expect(rows[4].pct).toBe(20);
  });
  it('adds Claude spend only with a reading and keeps its real percentage', () => {
    const now = 0;
    setLang('en');
    const spend = { agent: 'claude' as const, window: 'spend' as const, used_pct: 112, resets_at: null };
    expect(limitRows([], now).some(r => r.window === 'spend')).toBe(false);
    const rows = limitRows([spend], now);
    expect(rows[2]).toMatchObject({ window: 'spend', pct: 112, label: 'Claude · spend limit' });
    expect(limitCards([spend], now)[0].rows[2].pct).toBe(112);
    expect(rows[2].pace).toBeNull();
    setLang('pl');
    expect(limitRows([spend], now)[2].label).toBe('Claude · limit wydatków');
  });
  it('shows the reset time when it is known', () => {
    const now = new Date(2026, 8, 24, 12, 0).getTime();
    const rows = limitRows([{ agent: 'claude', window: 'five_hour', used_pct: 40, resets_at: now + 2 * 3_600_000 }], now);
    expect(rows[0].reset).toBe('reset 14:00');
  });
  it('says when a limit runs out at the current pace, in both languages, only for a fresh reading', () => {
    const now = new Date(2026, 8, 24, 12, 0).getTime();
    const l = { agent: 'claude' as const, window: 'five_hour' as const, used_pct: 60, resets_at: now + 5 * 3_600_000 };
    const f = (ms: number) => [{ agent: 'claude' as const, window: 'five_hour' as const, runs_out_at: now + ms }];
    setLang('pl');
    expect(limitRows([l], now, f(3.5 * 3_600_000))[0].pace).toBe('W tym tempie: 100% ok. 15:30');
    expect(limitRows([l], now, f(2 * 86_400_000))[0].pace).toBe('W tym tempie: 100% ok. sob 12:00');
    setLang('en');
    expect(limitRows([l], now, f(3.5 * 3_600_000))[0].pace).toMatch(/^At this pace: 100% around 3:30\sPM$/);
    expect(limitRows([l], now, f(2 * 86_400_000))[0].pace).toMatch(/^At this pace: 100% around Sat 12:00\sPM$/);
    setLang('pl');
    expect(limitRows([l], now)[0].pace).toBeNull();
    expect(limitRows([l], now, f(-1000))[0].pace).toBeNull();
    expect(limitRows([{ ...l, stale_since: now - 1000 }], now, f(3_600_000))[0].pace).toBeNull();
    expect(limitRows([l], now, [{ agent: 'codex', window: 'five_hour', runs_out_at: now + 3_600_000 }])[0].pace).toBeNull();
    expect(limitCards([l], now, f(3_600_000))[0].rows[0].pace).not.toBeNull();
  });
  it('describes a session', () => {
    const x = { ...s('a', 'working', 0), tool: 'bash' as const, origin: 'desktop' as const, progress: { done: 2, total: 5 }, context: { used: 50, max: 200 } };
    expect(sessionSubtitle(x)).toBe('Claude Code · aplikacja · a');
    expect(sessionSubtitle({ ...x, origin: 'cli', model: 'gpt-6-sol', agent: 'opencode', jump: { ...x.jump, app: 'vscode' } })).toBe('opencode · GPT-6 Sol · VS Code · a');
    expect(progressText(x)).toBe('2/5');
    expect(contextText(x)).toBe('25%');
    expect(progressText({ ...x, progress: { done: 0, total: 0 } })).toBeNull();
    expect(contextText({ ...x, context: { used: 1, max: 0 } })).toBeNull();
  });
  it('inactive means idle, done, asleep or ended, like the core', () => {
    expect(hasInactive([s('a', 'working', 0), s('b', 'needs_you', 0), s('c', 'error', 0)])).toBe(false);
    for (const st of ['idle', 'done', 'sleep', 'ended'] as State[]) expect(hasInactive([s('a', st, 0)])).toBe(true);
  });
  it('children stay under their parent, oldest first, and finished ones leave after 10 s', () => {
    const kid = (id: string, started: number, over: Partial<Session> = {}): Session => ({ ...s(id, 'working', 5), parent: 'p', started_at: started, ...over });
    const all = [s('p', 'working', 1), kid('b', 20), kid('a', 10), kid('gone', 5, { state: 'done', state_since: 1_000 }), s('q', 'idle', 2)];
    expect(panelSessions(all).map(x => x.id)).toEqual(['q', 'p']);
    expect(childrenOf(all, 'p', 1_000 + CHILD_DONE_MS - 1).map(x => x.id)).toEqual(['gone', 'a', 'b']);
    expect(childrenOf(all, 'p', 1_000 + CHILD_DONE_MS).map(x => x.id)).toEqual(['a', 'b']);
    expect(hasInactive([s('p', 'working', 1), kid('d', 1, { state: 'done' })]), 'a child is not hidden separately').toBe(false);
  });
  const sub = (over: Partial<Session> = {}): Session => ({
    ...s('k', 'working', 0), parent: 'p', title: 'Znajdź testy', action: 'Szukanie: bubble',
    sub: { kind: 'claude', agent_type: 'Explore', description: 'Znajdź testy', background: false }, ...over,
  });
  it('labels a subagent with its type and "background"; the type is not repeated when it is the title', () => {
    expect(childLabels(sub(), 0)).toEqual([{ kind: 'type', text: 'Explore' }]);
    expect(childLabels(sub({ sub: { kind: 'claude', agent_type: 'Explore', description: 'X', background: true } }), 0))
      .toEqual([{ kind: 'type', text: 'Explore' }, { kind: 'bg', text: 'w tle' }]);
    expect(childLabels(sub({ title: 'Explore', sub: { kind: 'claude', agent_type: 'Explore', description: null, background: false } }), 0)).toEqual([]);
    expect(childLabels(sub({ sub: null }), 0)).toEqual([]);
  });
  it('a router task is labelled Router and warns only when it is stuck or blocked', () => {
    const task = { task_id: 't', status: 'running', last_activity_at: 0, blocked: false, stall_ms: 180_000 };
    const r = sub({ agent: 'codex', sub: { kind: 'router', agent_type: null, description: null, background: false }, router_task: task });
    expect(childLabels(r, 60_000)).toEqual([{ kind: 'router', text: 'Router' }]);
    expect(childLabels(r, 200_000)).toEqual([{ kind: 'router', text: 'Router' }, { kind: 'warn', text: 'utknęło' }]);
    expect(childLabels({ ...r, router_task: { ...task, blocked: true } }, 0)[1]).toEqual({ kind: 'warn', text: 'zablokowane' });
  });
  it('the second line is the current action while working, else the outcome', () => {
    expect([childMark(sub()), childLine(sub())]).toEqual(['run', 'Szukanie: bubble']);
    expect(childLine(sub({ action: null, tool: 'bash' }))).toBe('Uruchamia komendy');
    for (const st of ['done', 'ended'] as State[]) expect([childMark(sub({ state: st })), childLine(sub({ state: st }))]).toEqual(['ok', 'Skończył']);
    expect([childMark(sub({ state: 'error' })), childLine(sub({ state: 'error' }))]).toEqual(['err', 'Błąd']);
    expect(childMark(sub({ state: 'idle' }))).toBe('idle');
  });
  it('running time reads like a clock', () => {
    expect([clock(0), clock(9_999), clock(72_000), clock(3_723_000), clock(-5)]).toEqual(['0:00', '0:09', '1:12', '1:02:03', '0:00']);
  });
});

describe('panel tabs and limits', () => {
  const now = new Date(2026, 8, 24, 12, 0).getTime();
  const lim = (agent: 'claude' | 'codex', window: 'five_hour' | 'weekly', used_pct: number, stale_since?: number) =>
    ({ agent, window, used_pct, resets_at: null, ...(stale_since != null ? { stale_since } : {}) });

  it('counts only top-level sessions that are not inactive', () => {
    const child = { ...s('k', 'working', 1), parent: 'a' };
    expect(activeCount([s('a', 'working', 1), s('b', 'needs_you', 1), s('c', 'idle', 1), s('d', 'done', 1), s('e', 'sleep', 1), s('f', 'ended', 1), child])).toBe(2);
    expect(activeCount([])).toBe(0);
  });
  it('raises the limits alert from 80 % or for a stale reading, never without data', () => {
    expect(limitsAlert([])).toBe(false);
    expect(limitsAlert([lim('claude', 'five_hour', 79)])).toBe(false);
    expect(limitsAlert([lim('claude', 'five_hour', 80)])).toBe(true);
    expect(limitsAlert([lim('codex', 'weekly', 10, now - 3_600_000)])).toBe(true);
  });
  it('collapses more than three children until expanded', () => {
    const c = [1, 2, 3, 4, 5];
    expect(collapseChildren([1, 2, 3], false)).toEqual({ shown: [1, 2, 3], hidden: 0 });
    expect(collapseChildren(c, false)).toEqual({ shown: [1, 2, 3], hidden: 2 });
    expect(collapseChildren(c, true)).toEqual({ shown: c, hidden: 0 });
    expect(collapseChildren([], false)).toEqual({ shown: [], hidden: 0 });
  });
  it('builds one limits card per agent; an agent with no readings is noData and antigravity needs data', () => {
    const cards = limitCards([lim('claude', 'five_hour', 40)], now);
    expect(cards.map(x => x.agent)).toEqual(['claude', 'codex']);
    expect(cards[0].noData).toBe(false);
    expect(cards[1].noData).toBe(true);
    expect(cards[0].rows.length).toBe(2);
  });
  it('marks a card stale when any of its rows is', () => {
    const cards = limitCards([lim('codex', 'weekly', 30, now - 7_200_000)], now);
    expect(cards.find(x => x.agent === 'codex')!.stale).toBe(true);
    expect(cards.find(x => x.agent === 'claude')!.stale).toBe(false);
  });
});

describe('context bar', () => {
  it('gives a clamped percentage, or null without a context window', () => {
    const base = s('a', 'working', 1);
    expect(contextPct(base)).toBeNull();
    expect(contextPct({ ...base, context: { used: 50, max: 200 } })).toBe(25);
    expect(contextPct({ ...base, context: { used: 500, max: 200 } })).toBe(100);
    expect(contextPct({ ...base, context: { used: 5, max: 0 } })).toBeNull();
  });
});
