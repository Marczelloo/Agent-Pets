import { renderToString } from 'react-dom/server';
import { afterEach, describe, expect, it } from 'vitest';
import { setLang } from '../i18n';
import { setLinux } from '../platform';
import { PanelView } from '../panel/App';
import type { AppRow, Diagnostics, Settings, UpdateStatus } from '../types';
import { SLOT } from '../stage/layout';
import { defaultSettings } from './model';
import { AppsTab } from './AppsTab';
import { SettingsView, type Tab } from './SettingsView';
import { resetStage, withBubbles } from './StageTab';
import { Wizard } from './Wizard';

const rows: AppRow[] = [
  { id: 'claude_code', detected: { found: true, path: 'C:/h/.claude', note: null }, status: { installed: false, detail: 'Hooki: brak' }, enabled: true },
  { id: 'codex', detected: { found: false, path: null, note: 'Nie znaleziono ~/.codex.' }, status: { installed: true, detail: 'Nic do instalowania' }, enabled: false },
  { id: 'agent_router', detected: { found: true, path: 'C:/h/.agent-router', note: null }, status: { installed: true, detail: 'Nic do instalowania' }, enabled: true },
  { id: 'opencode', detected: { found: true, path: 'C:/h/.config/opencode', note: null }, status: { installed: false, detail: 'Plugin: brak' }, enabled: false },
  { id: 'copilot', detected: { found: true, path: 'C:/h/.copilot', note: null }, status: { installed: false, detail: 'Hooki: brak' }, enabled: false },
  { id: 'antigravity', detected: { found: false, path: null, note: 'Nie znaleziono ~/.gemini.' }, status: { installed: false, detail: 'Hooki: brak' }, enabled: false },
];
const found = (id: AppRow['id']): AppRow =>
  ({ id, detected: { found: true, path: `C:/h/.${id}`, note: null }, status: { installed: false, detail: 'Hooki: brak' }, enabled: false });
const newRows: AppRow[] = [...rows, found('cursor'), found('grok'), found('zcode')];
const diag: Diagnostics = { version: '0.5.0', endpoint_port: 1, settings_path: 's', settings_error: null, hook_exe: null,
  autostart_registered: false, last_seen: {}, apps: [], stats_files: 0, stats_scanned_bytes: 0, stats_total_bytes: 0,
  log_path: '', log_tail: [] };
const noop = async () => [] as string[];
afterEach(() => setLang('pl'));

describe('Wizard', () => {
  it('a found Cursor, Grok or ZCode stays off until the person turns it on; they, Copilot and Antigravity wear the experimental badge', () => {
    const html = renderToString(<Wizard rows={newRows} initial={defaultSettings()} onFinish={noop} />);
    for (const label of ['Cursor', 'Grok Build', 'ZCode']) expect(html).toMatch(new RegExp(`aria-label="${label}"(?![^>]*checked="")`));
    expect(html).toMatch(/aria-label="Claude Code"[^>]*checked=""/);
    expect(html.match(/class="badge">eksperymentalne</g)?.length).toBe(3 + 2);
    expect(html).toContain('~/.cursor/hooks.json');
  });
  it('the look step offers the style gallery and the motion switch', () => {
    const html = renderToString(<Wizard rows={rows} initial={defaultSettings()} onFinish={noop} initialStep="look" />);
    expect(html).toContain('gallery compact');
    expect(html).toContain('Pixel-art');
    expect(html).toContain('Dynami'); // Dynamiczny / Dynamic
  });
  it('starts with the apps it found; missing ones are greyed out with a hint', () => {
    const html = renderToString(<Wizard rows={rows} initial={defaultSettings()} onFinish={noop} />);
    expect(html).toContain('Claude Code');
    expect(html).toContain('Nie znaleziono ~/.codex.');
    expect(html).toMatch(/<input[^>]*disabled[^>]*aria-label="Codex"|<input[^>]*aria-label="Codex"[^>]*disabled/);
  });
  it('asks for plan usage consent, off by default, and says where the token goes', () => {
    const html = renderToString(<Wizard rows={rows} initial={defaultSettings()} onFinish={noop} initialStep="limits" />);
    expect(html).toContain('api.anthropic.com');
    expect(html).not.toMatch(/aria-label="Limity z Anthropic"[^>]*checked|checked[^>]*aria-label="Limity z Anthropic"/);
  });
});

describe('language', () => {
  it('general tab offers the language, and English renders English', () => {
    const s = { ...defaultSettings(), language: 'en' as const };
    setLang('en');
    const html = renderToString(<SettingsView settings={s} rows={rows} diag={diag} tab="general" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('Język / Language');
    expect(html).toContain('Start with Windows');
    expect(html).not.toContain('Uruchamiaj z Windows');
  });
  it('the wizard shows a language picker on its first step', () => {
    const html = renderToString(<Wizard rows={rows} initial={defaultSettings()} onFinish={noop} />);
    expect(html).toContain('aria-label="Język / Language"');
  });
  it('new settings follow the system language', () => {
    expect(defaultSettings().language).toBe('auto');
  });
});

describe('settings shell and appearance', () => {
  const render = (tab: Tab, s = defaultSettings()) => renderToString(<SettingsView settings={s} rows={rows} diag={diag} tab={tab} onTab={() => {}}
    onChange={() => {}} onIntegration={async () => ''} message={null} />);
  it('the sidebar groups the tabs under Ustawienia and Aplikacja', () => {
    const html = render('general');
    expect(html).toContain('Ustawienia');
    expect(html).toContain('Aplikacja');
    for (const tab of ['Aplikacje', 'Wygląd', 'Pasek', 'Powiadomienia', 'Limity', 'Ogólne', 'Diagnostyka']) expect(html).toContain(tab);
    expect(html).toMatch(/aria-current="page"[^>]*>(<svg[^>]*>.*?<\/svg>)?<span>Ogólne</);
  });
  it('apps tab: the Claude Code mod switch sits under Claude Code and follows the language', () => {
    try {
      setLang('en');
      const html = render('apps');
      expect(html).toContain('aria-label="Claude Code mod"');
      expect(html).toContain('Takes effect in new Claude Code sessions.');
      expect(html.indexOf('Claude Code mod')).toBeGreaterThan(html.indexOf('C:/h/.claude'));
      expect(html).toContain('aria-label="Claude Code mod" checked=""');
      expect(render('apps', { ...defaultSettings(), claude_mod: false })).toContain('aria-label="Claude Code mod"/>');
      setLang('pl');
      expect(render('apps')).toContain('aria-label="Mod do Claude Code"');
      expect(render('apps', { ...defaultSettings(), apps: { ...defaultSettings().apps, claude_code: false } })).not.toContain('Mod do Claude Code');
    } finally { setLang('pl'); }
  });
  it('apps tab: the pet and nudges switches show only while Claude Code and its mod are on, and flip their own field', () => {
    try {
      setLang('en');
      const pet = 'aria-label="Pixel pet in the terminal"', nudges = 'aria-label="Nudges about other agents"';
      const html = render('apps');
      expect(html).toContain(pet);
      expect(html).toContain(nudges);
      expect(html.indexOf(pet)).toBeGreaterThan(html.indexOf('aria-label="Claude Code mod"'));
      expect(html).toMatch(/aria-label="Nudges about other agents" checked=""/);
      expect(html).not.toMatch(/aria-label="Pixel pet in the terminal" checked=""/);
      const on = { ...defaultSettings(), claude_mod_pet: true, claude_mod_nudges: false };
      expect(render('apps', on)).toMatch(/aria-label="Pixel pet in the terminal" checked=""/);
      expect(render('apps', on)).not.toMatch(/aria-label="Nudges about other agents" checked=""/);
      for (const s of [{ ...defaultSettings(), claude_mod: false },
        { ...defaultSettings(), apps: { ...defaultSettings().apps, claude_code: false } }]) {
        expect(render('apps', s)).not.toContain(pet);
        expect(render('apps', s)).not.toContain(nudges);
      }
      const seen: Settings[] = [];
      const toggles = (n: unknown, out: { label: string; onChange: (on: boolean) => void }[] = []): typeof out => {
        if (Array.isArray(n)) n.forEach(c => toggles(c, out));
        else if (n && typeof n === 'object' && 'props' in n) {
          const p = (n as { props: Record<string, unknown> }).props;
          if (typeof p.label === 'string' && typeof p.onChange === 'function') out.push({ label: p.label, onChange: p.onChange as (on: boolean) => void });
          toggles(p.children, out);
          toggles(p.control, out);
        }
        return out;
      };
      const tree = AppsTab({ settings: defaultSettings(), rows, onChange: s => seen.push(s), onIntegration: async () => '', onClaudeMod: async () => '' });
      const by = (label: string) => toggles(tree).find(x => x.label === label)!;
      by('Pixel pet in the terminal').onChange(true);
      by('Nudges about other agents').onChange(false);
      expect(seen[0]).toEqual({ ...defaultSettings(), claude_mod_pet: true });
      expect(seen[1]).toEqual({ ...defaultSettings(), claude_mod_nudges: false });
    } finally { setLang('pl'); }
  });
  it('general tab: theme is an icon-only radiogroup of system, light and dark with the current one checked', () => {
    const html = render('general', { ...defaultSettings(), theme: 'dark' });
    expect(html).toContain('Motyw');
    expect(html).toMatch(/role="radiogroup" aria-label="Motyw"/);
    expect(html.match(/role="radio" aria-checked="(true|false)"[^>]*title="(Systemowy|Jasny|Ciemny)"/g)?.length).toBe(3);
    expect(html).toMatch(/aria-checked="true"[^>]*title="Ciemny"/);
  });
  it('general tab: power saving is a segmented control and the language stays a labelled select', () => {
    const html = render('general', { ...defaultSettings(), power_saving: 'always' });
    expect(html).toMatch(/role="radiogroup" aria-label="Tryb oszczędny"/);
    expect(html).toMatch(/role="radio" aria-checked="true"[^>]*>Zawsze</);
    expect(html).toContain('aria-label="Język / Language"');
  });
  it('the theme labels are English after switching', () => {
    setLang('en');
    const html = render('general');
    expect(html).toContain('Theme');
    expect(html).toContain('title="Dark"');
  });
});

describe('SettingsView', () => {
  it('general tab: update mode select, check button with its result and the installed version', () => {
    const s = { ...defaultSettings(), updates: 'auto' as const };
    const view = (update?: UpdateStatus) => renderToString(<SettingsView settings={s} rows={rows} diag={{ ...diag, version: '0.7.0' }} tab="general"
      onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} update={update} onCheck={() => {}} onInstall={() => {}} />);
    const html = view();
    expect(html).toContain('Aktualizacje');
    expect(html).toMatch(/role="radio" aria-checked="true"[^>]*>Instaluj automatycznie</);
    expect(html).toContain('Powiadamiaj');
    expect(html).toContain('Instaluj automatycznie');
    expect(html).toContain('Sprawdź teraz');
    expect(html).toContain('Wersja 0.7.0');
    expect(view({ state: 'latest' })).toContain('Masz najnowszą wersję');
    expect(view({ state: 'available', version: '0.7.1', notes: null })).toContain('Dostępna wersja 0.7.1');
    expect(html).not.toContain('Zainstaluj teraz');
    expect(view({ state: 'available', version: '0.7.1', notes: null })).toMatch(/<button[^>]*>Zainstaluj teraz</);
    expect(view({ state: 'downloading', version: '0.7.1', pct: 40 })).toMatch(/Pobieram wersję 0.7.1… 40%[\s\S]*<button[^>]*disabled[^>]*>Zainstaluj teraz</);
    expect(view({ state: 'ready', version: '0.7.1' })).toMatch(/<button[^>]*>Zainstaluj teraz</);
    expect(view({ state: 'error', message: 'Błąd sprawdzania aktualizacji', verify: false })).toContain('Błąd sprawdzania aktualizacji');
  });
  it('renders the look tab in English after switching', () => {
    setLang('en');
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('Same as default');
    expect(html).not.toContain('Jak domyślny');
  });
  it('look tab: seven style cards, the chosen one checked, motion switch and per-agent overrides', () => {
    const s = defaultSettings();
    const html = renderToString(<SettingsView settings={s} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    for (const name of ['Naklejka', 'Szkic', 'Czysty', 'Pixel-art', 'Neon', 'Tusz', 'Pastel']) expect(html).toContain(name);
    expect(html).toMatch(/<button[^>]*aria-checked="true"[^>]*look-card[^>]*><canvas[^>]*><\/canvas><span>Naklejka<\/span>/);
    expect(html).toContain('Spokojny');
    expect(html).toContain('Dynami'); // Dynamiczny / Dynamic
    expect(html).toContain('Osobno dla agentów');
    expect(html).toContain('Jak domyślny');
    expect(html).toContain('Tak wygląda w pasku');
    expect(html).not.toContain('Najwięcej zwierzaków w pasku'); // moved to the Taskbar tab
  });
  it('taskbar tab: position, monitor, background, size, spacing, pet limit, alignment, order, elements and reset', () => {
    const monitors = [{ id: 'primary-dev', primary: true, width: 2560, height: 1440, index: 1, has_bar: true }, { id: 'second', primary: false, width: 1920, height: 1080, index: 2, has_bar: true }];
    const view = (stage: Partial<Settings['stage']>, leftFallback = false) => renderToString(<SettingsView
      settings={{ ...defaultSettings(), stage: { ...defaultSettings().stage, ...stage } }} rows={rows} diag={diag} tab="stage" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} monitors={monitors} leftFallback={leftFallback} onMove={() => {}} />);
    const html = view({});
    for (const s of ['Pozycja', 'Przy zasobniku', 'Po lewej', 'Własna', 'Pływające', 'Monitor', 'Ekran 2 · 1920×1080', '(główny)',
      'Tło', 'Brak', 'Szkło', 'Pełny kolor', 'Rozmiar zwierzaków', 'Odstęp', 'Margines', 'Najwięcej zwierzaków w pasku', 'Wyrównanie',
      'Kolejność', 'Paski postępu', 'Paski limitów', 'Plakietka „+N”', 'Przywróć domyślne']) expect(html, s).toContain(s);
    expect(html).toMatch(/aria-label="Rozmiar zwierzaków"[^>]*max="100"|max="100"[^>]*aria-label="Rozmiar zwierzaków"/);
    expect(html).toMatch(/role="radiogroup" aria-label="Wyrównanie"[^>]*aria-disabled="true"/);
    expect(html).not.toContain('Przesuń');
    const floating = view({ position: 'floating', size: 250 });
    expect(floating).toMatch(/max="300"/);
    expect(floating).not.toMatch(/role="radiogroup" aria-label="Wyrównanie"[^>]*aria-disabled="true"/);
    expect(view({ position: 'custom', custom_at: 0.3 })).toContain('Przesuń');
    expect(view({ size: 250 })).toMatch(/aria-valuetext="100%"/);
    expect(view({ size: 250 })).toContain('Większe tylko w oknie pływającym');
    expect(view({ position: 'left' }, true)).toContain('Ikony paska są wyrównane do lewej');
    expect(view({ background: { kind: 'glass', radius: 12 } })).toContain('Przezroczystość');
    expect(html).not.toContain('Przezroczystość');
  });
  it('taskbar tab: position is a set of cards and the Move row sits in that same section only for a custom position', () => {
    const view = (stage: Partial<Settings['stage']>) => renderToString(<SettingsView
      settings={{ ...defaultSettings(), stage: { ...defaultSettings().stage, ...stage } }} rows={rows} diag={diag} tab="stage" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} monitors={[]} onMove={() => {}} />);
    const first = (h: string) => { const a = h.indexOf('<h3>Położenie</h3>'); return h.slice(a, h.indexOf('<h3>', a + 4)); };
    const custom = first(view({ position: 'custom', custom_at: 0.3 }));
    expect(custom).toMatch(/class="ui-card on"/);
    expect(custom).toContain('<svg');
    expect(custom).toContain('Przesuń');
    expect(first(view({ position: 'right' }))).not.toContain('Przesuń');
    expect(view({}).match(/<h3>[^<]*<\/h3>/g)).toEqual(['<h3>Położenie</h3>', '<h3>Okno</h3>', '<h3>Zwierzaki</h3>', '<h3>Elementy</h3>', '<h3>Dymki i subagenci</h3>']);
  });
  it('taskbar tab: the pet limit is a stepper with a name and no native number input', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="stage" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} monitors={[]} onMove={() => {}} />);
    expect(html).toContain('class="ui-stepper"');
    expect(html).toContain('aria-label="Najwięcej zwierzaków w pasku"');
    expect(html).not.toContain('type="number"');
    expect(html).not.toContain('class="segmented"');
    expect(html).not.toContain('class="card"');
  });
  it('taskbar tab: an unplugged saved monitor stays selected, a monitor without a taskbar gets a hint', () => {
    const monitors = [{ id: 'one', primary: true, width: 2560, height: 1440, index: 1, has_bar: true },
      { id: 'two', primary: false, width: 1920, height: 1080, index: 2, has_bar: false }];
    const view = (stage: Partial<Settings['stage']>) => renderToString(<SettingsView
      settings={{ ...defaultSettings(), stage: { ...defaultSettings().stage, ...stage } }} rows={rows} diag={diag} tab="stage" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} monitors={monitors} />);
    expect(view({ monitor: 'gone' })).toMatch(/<option value="gone" selected="">[^<]*odłączony/);
    expect(view({ monitor: 'two' })).toContain('Na tym monitorze nie ma paska zadań');
    expect(view({ monitor: 'two', position: 'floating' })).not.toContain('Na tym monitorze nie ma paska zadań');
    expect(view({ monitor: 'one' })).not.toContain('odłączony');
  });
  it('taskbar tab reset keeps the pet limit and everything outside the stage', () => {
    const s = { ...defaultSettings(), pets: { ...defaultSettings().pets, max_visible: 3 }, stage: { ...defaultSettings().stage, gap: 20, position: 'floating' as const } };
    const r = resetStage(s);
    expect(r.stage).toEqual(defaultSettings().stage);
    expect(r.pets.max_visible).toBe(3);
  });
  it('look tab: a big preview and every animation to pick, grouped, plus all in order', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('class="preview-stage"');
    for (const name of ['Wszystkie po kolei', 'Praca', 'Stany', 'Reakcje', 'Komendy', 'Subagent', 'Kompaktuje', 'Pożegnanie', 'Śpi']) expect(html).toContain(name);
  });
  it('look tab: sections run Podgląd, Styl, Ruch, Dymki, the taskbar, Osobno dla agentów and no old button rows remain', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    const at = ['Podgląd', 'Styl', 'Ruch', 'Dymki', 'Tak wygląda w pasku', 'Otoczenie', 'Osobno dla agentów'].map(x => html.indexOf(`<h3>${x}</h3>`));
    expect(at.every(i => i > -1)).toBe(true);
    expect([...at].sort((a, b) => a - b)).toEqual(at);
    expect(html).not.toContain('class="segmented"');
    expect(html).not.toContain('class="chips"');
    expect(html).toMatch(/aria-label="Słuchają muzyki"/);
  });
  it('look tab: all-in-order is a toggle button, not a switch, and music sits under Otoczenie rather than the taskbar preview', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toMatch(/<button[^>]*aria-pressed="false"[^>]*>.*?Wszystkie po kolei<\/button>/);
    expect(html.match(/class="ui-switch"/g)?.length).toBe(1);
    expect(html.indexOf('Słuchają muzyki')).toBeGreaterThan(html.indexOf('<h3>Otoczenie</h3>'));
    expect(html.indexOf('<h3>Tak wygląda w pasku</h3>')).toBeLessThan(html.indexOf('<h3>Otoczenie</h3>'));
    expect(html.indexOf('Słuchają muzyki')).toBeLessThan(html.indexOf('<h3>Osobno dla agentów</h3>'));
  });
  it('look tab: the taskbar preview gives every pet a full slot, so none overlap', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toMatch(new RegExp(`class="taskbar"[^>]*width="${SLOT * 9}"`));
  });
  it('the notifications tab offers to mute, and says until when a mute lasts', () => {
    const view = (muted_until: number | null, now = 1_000) => renderToString(<SettingsView settings={{ ...defaultSettings(), notifications: { ...defaultSettings().notifications, muted_until } }}
      rows={rows} diag={diag} tab="notify" onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} now={now} />);
    const off = view(null);
    expect(off).toContain('Wyciszenie');
    for (const l of ['Wyłączone', '1 godz.', 'Do 8:00', 'Do odwołania']) expect(off).toContain(l);
    expect(off).toMatch(/aria-checked="true"[^>]*>Wyłączone/);
    expect(off).not.toContain('Wyciszone do');
    const hhmm = (ts: number) => `${String(new Date(ts).getHours()).padStart(2, '0')}:${String(new Date(ts).getMinutes()).padStart(2, '0')}`;
    const until = 1_000 + 90 * 60_000;
    expect(view(until)).toContain(`Wyciszone do ${hhmm(until)}`);
    expect(view(until)).not.toMatch(/aria-checked="true"[^>]*>Wyłączone/);
    expect(view(1_000 - 1, 1_000), 'an expired mute is no mute').toMatch(/aria-checked="true"[^>]*>Wyłączone/);
    const forever = view(9223372036854776000);
    expect(forever).toContain('Wyciszone do odwołania');
    expect(forever).toMatch(/aria-checked="true"[^>]*>Do odwołania/);
    setLang('en');
    expect(view(until)).toContain(`Muted until ${hhmm(until)}`);
    expect(view(null)).toContain('Until 8:00');
  });
  it('offers the weekly recap switch in both languages', () => {
    const view = () => renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="notify" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    setLang('pl');
    expect(view()).toContain('Podsumowanie tygodnia');
    expect(view()).toContain('W poniedziałek rano');
    setLang('en');
    expect(view()).toContain('Weekly recap');
    expect(view()).toContain('On Monday morning');
    setLang('pl');
  });
  it('the diagnostics tab offers to report a problem on GitHub, only when it can open it', () => {
    const view = (onReport?: () => void) => renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="diag"
      onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} onReport={onReport} />);
    expect(view(() => {})).toContain('Zgłoś problem</button>');
    expect(view(() => {})).toContain('wklej go do formularza');
    expect(view()).not.toContain('Zgłoś problem');
  });
  it('notifications, limits and diagnostics sit on the shared kit: no card chrome, Claude heading with the CLI note, actions above the report', () => {
    const view = (tab: Tab, onReport?: () => void) => renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab={tab}
      onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} onReport={onReport} />);
    for (const tab of ['notify', 'limits', 'diag'] as Tab[]) expect(view(tab), tab).not.toContain('class="card"');
    expect(view('notify')).toContain('class="ui-note"');
    const limits = view('limits');
    expect(limits).toContain('<h3>Claude</h3>');
    expect(limits).toContain('claude auth login');
    const diagHtml = view('diag', () => {});
    expect(diagHtml.indexOf('Skopiuj raport')).toBeGreaterThan(-1);
    expect(diagHtml.indexOf('Skopiuj raport')).toBeLessThan(diagHtml.indexOf('class="report"'));
    expect(diagHtml.indexOf('Zgłoś problem</button>')).toBeLessThan(diagHtml.indexOf('class="report"'));
  });
  it('the wizard groups apps like the Apps tab and drops the card chrome', () => {
    const html = renderToString(<Wizard rows={newRows} initial={defaultSettings()} onFinish={noop} />);
    const at = ['Główne agenty', 'Eksperymentalne'].map(x => html.indexOf(`<h3>${x}</h3>`));
    expect(at[0]).toBeGreaterThan(-1);
    expect(at[1]).toBeGreaterThan(at[0]);
    for (const step of ['apps', 'limits', 'notify', 'look'] as const)
      expect(renderToString(<Wizard rows={rows} initial={defaultSettings()} onFinish={noop} initialStep={step} />), step).not.toContain('class="card');
  });
  it('lists the apps with their integration state', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('Hooki: brak');
    expect(html).toContain('Agent Router');
    expect(html).toContain('opencode');
    expect(html).toContain('Plugin: brak');
  });
  it('Copilot, Antigravity, Cursor, Grok and ZCode sit under Eksperymentalne, after the main agents; the new three say what they write and what they cannot show', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={newRows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    const at = (x: string) => html.indexOf(x);
    expect(at('Główne agenty')).toBeGreaterThan(-1);
    expect(at('Claude Code')).toBeLessThan(at('Eksperymentalne'));
    expect(at('opencode')).toBeLessThan(at('Eksperymentalne'));
    for (const name of ['GitHub Copilot', 'Antigravity', 'Cursor', 'Grok Build', 'ZCode']) expect(at(name)).toBeGreaterThan(at('Eksperymentalne'));
    expect(at('Eksperymentalne')).toBeLessThan(at('Furtka dla innych agentów'));
    expect(html).not.toContain('class="badge"');
    for (const path of ['~/.cursor/hooks.json', '~/.grok/hooks/agent-pets.json', '~/.zcode/cli/config.json']) expect(html).toContain(path);
    const only = renderToString(<SettingsView settings={defaultSettings()} rows={[found('cursor'), found('zcode')]} diag={diag} tab="apps"
      onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(only).toContain('czeka na Ciebie');
    expect(only).toContain('nie zgłasza błędów');
    setLang('en');
    const en = renderToString(<SettingsView settings={defaultSettings()} rows={newRows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(en).toContain('Experimental');
    expect(en).toContain('Main agents');
  });
  it('an app row shows one status line, keeps its path and hint under Details, dims when not detected and offers Reinstall when hooks are missing', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('Wykryto · Hooki: brak');
    expect(html).toContain('Szczegóły');
    expect(html).toContain('C:/h/.claude');
    expect(html).toContain('Zainstaluj ponownie');
    expect(html).toMatch(/class="ui-row dim"/);
    expect(html).toContain('Nie znaleziono ~/.codex.');
  });
  it('Copilot and Antigravity say what they write and where; Antigravity says what it cannot show', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('>GitHub Copilot<');
    expect(html).toContain('~/.copilot/hooks/agent-pets.json');
    expect(html).toContain('~/.gemini/config/hooks.json');
    expect(html).toContain('czeka na Ciebie');
    expect(html).toMatch(/aria-label="GitHub Copilot"(?![^>]*checked="")/);
  });
  it('the apps tab has the door switch, on for a 0.9.1 file', () => {
    const old = { ...defaultSettings(), apps: { claude_code: true, codex: true, agent_router: true } } as unknown as Settings;
    const html = renderToString(<SettingsView settings={old} rows={rows} diag={diag} tab="apps" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toMatch(/aria-label="Furtka dla innych agentów"[^>]*checked=""/);
    expect(html).toContain('hook.exe report');
  });
  it('offers a copyable report in diagnostics', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="diag" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('Skopiuj raport');
  });
});

describe('panel', () => {
  it('has a settings button', () => {
    const html = renderToString(<PanelView snap={{ sessions: [], limits: [], now: 0 }} nowMs={0} status={null} focusId={null}
      onJump={() => {}} onSettings={() => {}} />);
    expect(html).toContain('aria-label="Ustawienia"');
  });
  it('panel: a door agent name is text, never markup, and a program gets its icon', () => {
    const evil = '<img src=x onerror=alert(1)>';
    const sess = { id: 'generic:kilo:a', agent: 'other' as const, agent_name: evil, origin: 'cli' as const, title: evil, cwd: '', state: 'working' as const,
      tool: 'bash' as const, progress: null, context: null, started_at: 0, last_activity: 0, state_since: 0, turn_started_at: null,
      jump: { pid: null, session_id: 'generic:kilo:a', cwd: '', app: 'vscode' as const } };
    const html = renderToString(<PanelView snap={{ sessions: [sess], limits: [], now: 0 }} nowMs={0} status={null} focusId={null}
      onJump={() => {}} onSettings={() => {}} />);
    expect(html).not.toContain('<img');
    expect(html).toContain('&lt;img');
    expect(html).toMatch(/<svg[^>]*class="host-icon"/);
  });
  it('look tab: pick which pet the previews show', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toContain('aria-label="Zwierzak w podglądzie"');
    for (const name of ['Claude Code', 'Codex', 'opencode', 'Inny agent']) expect(html).toContain(`>${name}</button>`);
    expect(html).toMatch(/aria-checked="true"[^>]*>Claude Code<\/button>/);
  });
  it('look tab: previews the bubbles in the chosen style', () => {
    const html = renderToString(<SettingsView settings={defaultSettings()} rows={rows} diag={diag} tab="look" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} />);
    expect(html).toMatch(/<canvas[^>]*class="bubble-preview"/);
    expect(html).toContain('Dymki');
  });
  it('taskbar tab: bubbles and subagents section with three switches', () => {
    const s = defaultSettings();
    const html = renderToString(<SettingsView settings={s} rows={rows} diag={diag} tab="stage" onTab={() => {}}
      onChange={() => {}} onIntegration={async () => ''} message={null} monitors={[]} leftFallback={false} onMove={() => {}} />);
    for (const t of ['Dymki i subagenci', 'Dymki z pytaniami', 'Dymki z akcją', 'Mini-zwierzaki subagentów']) expect(html, t).toContain(t);
    const off = withBubbles(withBubbles(withBubbles(s, 'questions', false), 'actions', false), 'minis', false);
    expect([off.stage.bubbles, off.stage.minis]).toEqual([{ questions: false, actions: false }, false]);
    expect(off.pets).toEqual(s.pets);
  });
  it('panel: subagents under their parent with descriptions, router health and a parent-only count', () => {
    const base = { agent: 'claude' as const, origin: 'cli' as const, cwd: '', tool: 'bash' as const, progress: null, context: null,
      last_activity: 50_000, state_since: 0, turn_started_at: null, question: null };
    const parent = { ...base, id: 'p', title: 'Główna', state: 'working' as const, started_at: 0, jump: { pid: null, session_id: 'p', cwd: '', app: null } };
    const kid = (id: string, kind: 'claude' | 'codex' | 'router', description: string, started: number) => ({
      ...base, id, title: description, state: 'working' as const, started_at: started, parent: 'p', action: kind === 'claude' ? 'Czyta a.rs' : null,
      agent: kind === 'claude' ? 'claude' as const : 'codex' as const, origin: kind === 'router' ? 'router' as const : 'cli' as const,
      sub: { kind, agent_type: null, description, background: false }, jump: { pid: null, session_id: id, cwd: '', app: null },
      router_task: kind === 'router' ? { task_id: 't', status: 'running', last_activity_at: 100_000, blocked: false, stall_ms: 180_000 } : null });
    const snap = { sessions: [parent, kid('p/a', 'claude', 'Znajdź testy', 10_000), kid('c1', 'codex', 'Newton', 20_000), kid('th', 'router', 'Policz pliki', 30_000)], limits: [], now: 0 };
    const html = renderToString(<PanelView snap={snap} nowMs={300_000} status={null} focusId="c1" onJump={() => {}} />);
    expect(html).toMatch(/>Sesje<span class="n">1</);
    for (const t of ['Znajdź testy', 'Newton', 'Policz pliki', 'Czyta a.rs', 'utknęło']) expect(html, t).toContain(t);
    expect(html.match(/class="kid[ "]/g)?.length).toBe(3);
    expect(html).toMatch(/class="kid[^"]*focus/);
    expect(html.indexOf('Znajdź testy')).toBeLessThan(html.indexOf('Newton'));
  });
});

describe('keyboard shortcuts in the general tab', () => {
  const render = (s = defaultSettings(), hotkeyStatus?: { jump: string | null; panel: string | null }) => renderToString(<SettingsView settings={s} rows={rows} diag={diag}
    tab="general" onTab={() => {}} onChange={() => {}} onIntegration={async () => ''} message={null} hotkeyStatus={hotkeyStatus} />);
  it('shows both shortcuts with their combos, in Polish and in English', () => {
    const pl = render();
    expect(pl).toContain('Skróty klawiszowe');
    expect(pl).toContain('Przejdź do agenta, który czeka');
    expect(pl).toContain('Pokaż/ukryj panel');
    expect(pl).toContain('Ctrl + Alt + Shift + J');
    expect(pl).toContain('Ctrl + Alt + Shift + K');
    setLang('en');
    const en = render();
    expect(en).toContain('Keyboard shortcuts');
    expect(en).toContain('Jump to the waiting agent');
    expect(en).toContain('Show/hide the panel');
  });
  it('offers Default only for a changed shortcut and Off only for one that is on', () => {
    setLang('en');
    expect(render()).not.toContain('>Default<');
    const changed = render({ ...defaultSettings(), hotkeys: { jump: 'Ctrl+Shift+F2', panel: null } });
    expect(changed).toContain('Ctrl + Shift + F2');
    expect(changed.match(/>Default</g)?.length).toBe(2);
    const section = changed.slice(changed.indexOf('Keyboard shortcuts'), changed.indexOf('Checks GitHub'));
    expect(section.match(/>Off</g)?.length).toBe(2); // the "Off" button of jump, the "Off" label of panel
    expect(changed).toContain('aria-label="Show/hide the panel: Off"');
  });
  it('a settings file from before the shortcuts still renders the defaults', () => {
    const { hotkeys: _gone, ...old } = defaultSettings();
    expect(render(old as Settings)).toContain('Ctrl + Alt + Shift + J');
  });
  it('shows a registration error under the row that failed, and nothing when all is well', () => {
    setLang('en');
    const ok = render(defaultSettings(), { jump: null, panel: null });
    expect(ok).not.toContain('Couldn&#x27;t register');
    const bad = render(defaultSettings(), { jump: null, panel: 'in use' });
    expect(bad.match(/Couldn&#x27;t register/g)?.length).toBe(1);
    expect(bad.indexOf('Couldn&#x27;t register')).toBeGreaterThan(bad.indexOf('Show/hide the panel'));
    setLang('pl');
    expect(render(defaultSettings(), { jump: 'taken', panel: null })).toContain('Nie udało się zarejestrować — skrót zajęty przez inny program?');
  });
});

describe('Linux', () => {
  afterEach(() => setLinux(false));
  const monitors = [{ id: 'one', primary: true, width: 2560, height: 1440, index: 1, has_bar: true },
    { id: 'two', primary: false, width: 1920, height: 1080, index: 2, has_bar: false }];
  const view = (tab: Tab, stage: Partial<Settings['stage']> = {}, props: { leftFallback?: boolean; verticalBar?: boolean } = {}) => renderToString(<SettingsView
    settings={{ ...defaultSettings(), stage: { ...defaultSettings().stage, ...stage } }} rows={rows} diag={diag} tab={tab} onTab={() => {}}
    onChange={() => {}} onIntegration={async () => ''} message={null} monitors={monitors} onMove={() => {}} {...props} />);
  const cards = (h: string) => h.match(/role="radio"[^>]*class="ui-card/g)?.length ?? 0;

  it('offers three positions, and Custom only while it is the saved one', () => {
    setLinux(true);
    const html = view('stage');
    expect(cards(html)).toBe(3);
    for (const s of ['Na dole po prawej', 'Na dole po lewej', 'Pływające']) expect(html, s).toContain(s);
    expect(html).not.toContain('Na dole, własna');
    const custom = view('stage', { position: 'custom', custom_at: 0.3 });
    expect(cards(custom)).toBe(4);
    expect(custom).toContain('Na dole, własna');
    expect(custom).toContain('Przesuń');
  });
  it('Windows keeps the four positions', () => {
    setLinux(false);
    expect(cards(view('stage'))).toBe(4);
  });
  it('the preview has no taskbar band on Linux, Windows has one', () => {
    const band = /<rect x="2" y="27" width="92" height="11"/;
    setLinux(true);
    expect(view('stage')).not.toMatch(band);
    setLinux(false);
    expect(view('stage')).toMatch(band);
  });
  it('never shows the taskbar notes on Linux, even when the props and monitor would trigger them', () => {
    setLinux(true);
    const html = view('stage', { position: 'left', monitor: 'two' }, { leftFallback: true, verticalBar: true });
    for (const s of ['Ikony paska są wyrównane do lewej', 'Pasek zadań jest przypięty', 'nie ma paska zadań']) expect(html, s).not.toContain(s);
    setLang('en');
    const en = view('stage', { position: 'left', monitor: 'two' }, { leftFallback: true, verticalBar: true });
    for (const s of ['Taskbar icons are aligned left', 'The taskbar is docked', 'has no taskbar']) expect(en, s).not.toContain(s);
    expect(en).toContain('Bottom right');
    setLinux(false);
    const win = view('stage', { position: 'left', monitor: 'two' }, { leftFallback: true, verticalBar: true });
    for (const s of ['Taskbar icons are aligned left', 'The taskbar is docked', 'has no taskbar']) expect(win, s).toContain(s);
  });
  it('hides the Windows notification settings note on Linux', () => {
    setLinux(true);
    expect(view('notify')).not.toContain('Ustawieniach Windows');
    setLinux(false);
    expect(view('notify')).toContain('Ustawieniach Windows');
  });
  it('uses the Linux wording in the sidebar and the general tab', () => {
    setLinux(true);
    expect(view('general')).toContain('Uruchamiaj po zalogowaniu');
    expect(view('general')).not.toContain('Windows');
    expect(view('general')).toContain('<span>Ekran</span>');
  });
});
