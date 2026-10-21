import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const html = readFileSync(fileURLToPath(new URL('../../panel.html', import.meta.url)), 'utf8');
const css = (html.match(/<style>([\s\S]*?)<\/style>/)?.[1] ?? '').replace(/\/\*[\s\S]*?\*\//g, '');
const rules = [...css.matchAll(/(?<=^|\})\s*([^{}@]+)\{([^}]*)\}/g)].map(m => ({ sel: m[1].trim(), body: m[2] }));

describe('panel.html styles', () => {
  it('defines every selector once, so a later copy cannot silently override the earlier one', () => {
    const seen = new Map<string, number>();
    for (const r of rules) seen.set(r.sel, (seen.get(r.sel) ?? 0) + 1);
    expect([...seen].filter(([, n]) => n > 1).map(([s]) => s)).toEqual([]);
  });

  it('shows the notification remove button instead of hiding it until a session is hovered', () => {
    // .remove only lives on notifications; they are not .session, so an opacity:0 rule would hide it for good
    const remove = rules.filter(r => r.sel === '.remove');
    expect(remove.map(r => r.body).join(';')).not.toContain('opacity:0');
    expect(css).not.toContain('.session:hover .remove');
  });

  it('keeps the Limits-tab row grid off limit notifications: both carry the .limit class', () => {
    // a bare .limit grid squeezed a limit notification's text into its 96 px label column
    expect(rules.filter(r => /(^|[\s,])\.limit(?=[\s{.:,]|$)/.test(r.sel) && !r.sel.startsWith('.limits ') && !r.sel.startsWith('.note')).map(r => r.sel)).toEqual([]);
    expect(rules.find(r => r.sel === '.note .title')?.body).toContain('white-space:normal');
  });

  it('lets the session menu overflow its card: a clipped group would cut off the menu items', () => {
    const group = rules.find(r => r.sel === '.group')?.body ?? '';
    expect(group).not.toContain('overflow:hidden');
  });
});
