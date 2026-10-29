import { useState } from 'react';
import type { AppId, AppRow, Diagnostics, HotkeyStatus, MonitorInfo, Settings, UpdateStatus } from '../types';
import { LookTab } from './look/LookTab';
import { StageTab } from './StageTab';
import { AppsTab } from './AppsTab';
import { defaultHotkeys, FOREVER, mutedText, mutedUntil, reportText } from './model';
import { HotkeyRow } from './HotkeyRow';
import { BackupSection } from './BackupSection';
import { Toggle } from './Toggle';
import { t } from '../i18n';
import { onLinux } from '../platform';
import { LANGUAGE_LABEL } from '../i18n/pl';
import { AppsIcon, BellIcon, DiagIcon, GeneralIcon, LimitsIcon, LookIcon, MonitorIcon, MoonIcon, SunIcon, TaskbarIcon } from '../ui/icons';
import { LanguageSelect } from './LanguageSelect';
import { Row, Section, Segmented, Switch } from './ui';

/** Same words as `pets_core::mute::MuteChoice::parse`. */
export type MuteChoice = 'off' | 'hour' | 'morning' | 'forever';
export type Tab = 'apps' | 'look' | 'stage' | 'notify' | 'limits' | 'general' | 'diag';
/** Sidebar groups, top to bottom. */
export const TAB_GROUPS: { key: 'settings' | 'app'; tabs: Tab[] }[] = [
  { key: 'settings', tabs: ['apps', 'look', 'stage', 'notify', 'limits'] },
  { key: 'app', tabs: ['general', 'diag'] },
];
export const TABS: Tab[] = TAB_GROUPS.flatMap(g => g.tabs);
const TAB_ICON: Record<Tab, () => React.JSX.Element> = {
  apps: AppsIcon, look: LookIcon, stage: TaskbarIcon, notify: BellIcon, limits: LimitsIcon, general: GeneralIcon, diag: DiagIcon,
};

interface Props {
  settings: Settings;
  rows: AppRow[];
  diag: Diagnostics | null;
  tab: Tab;
  onTab: (t: Tab) => void;
  onChange: (s: Settings) => void;
  onIntegration: (id: AppId, on: boolean) => Promise<string>;
  onClaudeMod?: (on: boolean) => Promise<string>;
  message: string | null;
  /** update state (result of "Check now") */
  update?: UpdateStatus;
  onCheck?: () => void;
  /** download (if needed) and install the found version now; the app restarts */
  onInstall?: () => void;
  /** Diagnostics tab: open the bug form on GitHub */
  onReport?: () => void;
  onExport?: () => Promise<string>;
  onImport?: (text: string) => Promise<void>;
  onReveal?: (path: string) => Promise<void>;
  /** Taskbar tab */
  monitors?: MonitorInfo[];
  leftFallback?: boolean;
  verticalBar?: boolean;
  onMove?: () => void;
  /** Notifications tab: mute for a while (the deadline comes back through the settings) */
  onMute?: (choice: MuteChoice) => void;
  /** General tab: why a shortcut could not be registered (from the core) */
  hotkeyStatus?: HotkeyStatus;
  /** the clock, injectable for tests */
  now?: number;
}

/** Manual check result beside the button. */
function checkResult(u: UpdateStatus | undefined): string | null {
  switch (u?.state) {
    case 'checking': return t().settings.checking;
    case 'latest': return t().settings.latest;
    case 'available': return t().panel.update.available(u.version);
    case 'downloading': return t().panel.update.downloading(u.version) + (u.pct != null ? ` ${u.pct}%` : '');
    case 'ready': return t().panel.update.ready(u.version);
    case 'error': return u.message;
    default: return null;
  }
}

/** Settings window: tabs on the left like Windows 11 Settings; changes apply immediately. */
export function SettingsView({ settings: s, rows, diag, tab, onTab, onChange, onIntegration, onClaudeMod, message, update, onCheck, onInstall, onReport, onExport, onImport, onReveal, monitors = [], leftFallback = false, verticalBar = false, onMove = () => {}, onMute = () => {}, hotkeyStatus, now = Date.now() }: Props) {
  const [copied, setCopied] = useState(false);
  const set = (patch: Partial<Settings>) => onChange({ ...s, ...patch });
  const muteUntil = mutedUntil(s.notifications, now);
  const hotkeys = s.hotkeys ?? defaultHotkeys();
  const setHotkey = (k: 'jump' | 'panel', v: string | null) => set({ hotkeys: { ...hotkeys, [k]: v } });

  return (
    <div className="settings">
      <nav aria-label={t().panel.settings}>
        <h1>Agent Pets</h1>
        {TAB_GROUPS.map(g => (
          <div key={g.key} className="nav-group" role="group" aria-label={t().settings.groups[g.key]}>
            <h4>{t().settings.groups[g.key]}</h4>
            {g.tabs.map(id => {
              const Icon = TAB_ICON[id];
              return (
                <button type="button" key={id} className={tab === id ? 'on' : ''} aria-current={tab === id ? 'page' : undefined}
                  onClick={() => onTab(id)}><Icon /><span>{t().settings.tabs[id]}</span></button>
              );
            })}
          </div>
        ))}
      </nav>
      <main>
        <h2>{t().settings.tabs[tab]}</h2>
        {message && <p className="notice" role="status">{message}</p>}

        {tab === 'apps' && <AppsTab settings={s} rows={rows} onChange={onChange} onIntegration={onIntegration}
          onClaudeMod={onClaudeMod ?? (async on => { onChange({ ...s, claude_mod: on }); return ''; })} />}

        {tab === 'look' && <LookTab pets={s.pets} onChange={p => set({ pets: p })} />}
        {tab === 'stage' && <StageTab settings={s} monitors={monitors} leftFallback={leftFallback} verticalBar={verticalBar} onChange={onChange} onMove={onMove} />}

        {tab === 'notify' && <>
          <Section>
            <Row label={t().settings.mute} hint={muteUntil != null ? mutedText(muteUntil) : t().settings.muteDesc} control={
              <Segmented aria-label={t().settings.mute} value={(muteUntil == null ? 'off' : muteUntil >= FOREVER ? 'forever' : '') as MuteChoice} onChange={onMute} options={[
                { value: 'off', label: t().settings.muteOff }, { value: 'hour', label: t().settings.muteHour },
                { value: 'morning', label: t().settings.muteMorning }, { value: 'forever', label: t().settings.muteForever },
              ]} />} />
          </Section>
          <Section>
            <Toggle label={t().state.needs_you} checked={s.notifications.needs_you}
              onChange={on => set({ notifications: { ...s.notifications, needs_you: on } })}>{t().settings.notifyNeeds}</Toggle>
            <Toggle label={t().state.done} checked={s.notifications.done}
              onChange={on => set({ notifications: { ...s.notifications, done: on } })}>{t().settings.notifyDone}</Toggle>
            <Toggle label={t().limits.label} checked={s.notifications.limits}
              onChange={on => set({ notifications: { ...s.notifications, limits: on } })}>{t().limits.notification}</Toggle>
            <Toggle label={t().settings.notifyWeekly} checked={s.notifications.weekly ?? true}
              onChange={on => set({ notifications: { ...s.notifications, weekly: on } })}>{t().settings.notifyWeeklyDesc}</Toggle>
          </Section>
          <Section>
            <Toggle label={t().settings.notifySound} checked={s.notifications.sound ?? true}
              onChange={on => set({ notifications: { ...s.notifications, sound: on } })}>{t().settings.notifySoundDesc}</Toggle>
          </Section>
          {!onLinux() && <p className="ui-note">{t().settings.notifyWindows}</p>}
        </>}

        {tab === 'limits' && <Section title="Claude" note={t().limits.cliNote}>
          <Toggle label={t().limits.fromAnthropic} checked={s.claude_plan_usage} onChange={on => set({ claude_plan_usage: on })}>
            {t().limits.usage}
          </Toggle>
        </Section>}

        {tab === 'general' && <>
          <Section title={t().settings.appearance}>
            <Row label={t().settings.theme} hint={t().settings.themeDesc} control={
              <Segmented aria-label={t().settings.theme} value={s.theme ?? 'system'} onChange={v => set({ theme: v })} options={[
                { value: 'system', icon: <MonitorIcon />, title: t().settings.themes.system },
                { value: 'light', icon: <SunIcon />, title: t().settings.themes.light },
                { value: 'dark', icon: <MoonIcon />, title: t().settings.themes.dark },
              ]} />} />
            <Row label={LANGUAGE_LABEL} control={
              <LanguageSelect value={s.language ?? 'auto'} onChange={l => set({ language: l })} />} />
          </Section>
          <Section>
            <Row label={t().settings.autostart} hint={t().settings.autostartDesc}
              control={<Switch checked={s.autostart} onChange={on => set({ autostart: on })} aria-label={t().settings.autostart} />} />
            <Row label={t().settings.powerSaving} hint={t().settings.powerSavingDesc} control={
              <Segmented aria-label={t().settings.powerSaving} value={s.power_saving} onChange={v => set({ power_saving: v })} options={[
                { value: 'auto', label: t().settings.power.auto }, { value: 'always', label: t().settings.power.always }, { value: 'never', label: t().settings.power.never },
              ]} />} />
          </Section>
          <Section title={t().settings.hotkeys.title} note={t().settings.hotkeys.note}>
            <HotkeyRow label={t().settings.hotkeys.jump} hint={t().settings.hotkeys.jumpDesc} value={hotkeys.jump} fallback={defaultHotkeys().jump!}
              error={hotkeyStatus?.jump} onChange={v => setHotkey('jump', v)} />
            <HotkeyRow label={t().settings.hotkeys.panel} value={hotkeys.panel} fallback={defaultHotkeys().panel!}
              error={hotkeyStatus?.panel} onChange={v => setHotkey('panel', v)} />
          </Section>
          <BackupSection onExport={onExport} onImport={onImport} onReveal={onReveal} />
          <Section>
            <Row label={t().settings.updates} hint={t().settings.updatesDesc} control={
              <Segmented aria-label={t().settings.updates} value={s.updates ?? 'notify'} onChange={v => set({ updates: v })} options={[
                { value: 'notify', label: t().settings.updateMode.notify }, { value: 'auto', label: t().settings.updateMode.auto }, { value: 'off', label: t().settings.updateMode.off },
              ]} />} />
            <Row label={t().settings.latestVersion} control={<span className="check-ctl">
              <span className="ui-hint" role="status">{checkResult(update)}</span>
              {onInstall && (update?.state === 'available' || update?.state === 'downloading' || update?.state === 'ready') &&
                <button type="button" className="accent" disabled={update.state === 'downloading'} onClick={onInstall}>{t().panel.update.installNow}</button>}
              <button type="button" disabled={update?.state === 'checking' || !onCheck} onClick={onCheck}>{t().settings.checkNow}</button></span>} />
            <p className="ui-note">{t().settings.version(diag?.version ?? '–')}</p>
          </Section>
        </>}

        {tab === 'diag' && <Section>
          {onLinux() && <p className="ui-note" role="status">{t().settings.linuxBeta}</p>}
          <div className="diag-actions">
            <button type="button" disabled={!diag} onClick={() => {
              if (!diag) return;
              void navigator.clipboard.writeText(reportText(diag, Date.now())).then(() => setCopied(true));
            }}>{copied ? t().settings.copied : t().settings.copyReport}</button>
            {onReport && <button type="button" onClick={() => {
              // the form opens even when the clipboard refuses
              const copy = diag ? navigator.clipboard.writeText(reportText(diag, Date.now())).then(() => setCopied(true)) : Promise.resolve();
              void copy.catch(() => {}).finally(onReport);
            }}>{t().settings.reportProblem}</button>}
          </div>
          {onReport && <p className="ui-note">{t().settings.reportProblemDesc}</p>}
          {diag ? <pre className="report">{reportText(diag, Date.now())}</pre> : <p className="ui-note">{t().settings.loading}</p>}
        </Section>}
      </main>
    </div>
  );
}
