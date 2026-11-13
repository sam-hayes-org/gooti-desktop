import { Component, signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { FormsModule } from '@angular/forms';
import { vi } from 'vitest';
import { choose, openSelect, selectLabel, settle } from '../testing/fake-core';
import { Select, SelectOption } from './select';
import { TextMenu } from './text-menu';
import { Tooltip } from './tooltip';

@Component({
  imports: [FormsModule, Select, Tooltip, TextMenu],
  template: `
    <app-select inputId="pick" [options]="options" [(ngModel)]="value" />
    <span id="badge" appTooltip="The key stays on this desktop">Desktop only</span>
    <input id="name" name="name" [(ngModel)]="name" />
    <input id="secret" type="password" value="hunter2" />
    <p id="plain">Plain text</p>
    <app-text-menu />
  `,
})
class Host {
  readonly options: SelectOption<number>[] = [
    { value: 1, label: 'One' },
    { value: 2, label: 'Two', disabled: true },
    { value: 3, label: 'Three' },
  ];
  readonly value = signal(1);
  readonly name = signal('Alice');
}

async function render() {
  const fixture = TestBed.createComponent(Host);
  document.body.appendChild(fixture.nativeElement);
  await settle(fixture);
  return { fixture, root: fixture.nativeElement as HTMLElement, host: fixture.componentInstance };
}

function menuItems(): string[] {
  return Array.from(document.querySelectorAll<HTMLButtonElement>('.text-menu [cdkMenuItem]')).map(
    (b) => `${b.textContent?.trim()}${b.disabled ? ' (off)' : ''}`,
  );
}

function rightClick(target: Element): MouseEvent {
  const event = new MouseEvent('contextmenu', { bubbles: true, cancelable: true, clientX: 20, clientY: 20 });
  target.dispatchEvent(event);
  return event;
}

describe('Select', () => {
  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('shows the chosen label and reports a new choice', async () => {
    const { fixture, root, host } = await render();
    expect(selectLabel(root, 'pick')).toBe('One');
    await choose(fixture, root, 'pick', 'Three');
    expect(host.value()).toBe(3);
    expect(selectLabel(root, 'pick')).toBe('Three');
    expect(root.querySelector('#pick')!.getAttribute('aria-expanded')).toBe('false');
  });

  it('marks the chosen option and cannot choose a disabled one', async () => {
    const { fixture, root, host } = await render();
    const options = await openSelect(fixture, root, 'pick');
    expect(options.map((o) => o.getAttribute('aria-selected'))).toEqual(['true', 'false', 'false']);
    expect(options[1].getAttribute('aria-disabled')).toBe('true');
    options[1].click();
    await settle(fixture);
    expect(host.value()).toBe(1);
  });

  it('opens with the arrow key and closes with Escape, back on the field', async () => {
    const { fixture, root } = await render();
    const field = root.querySelector<HTMLButtonElement>('#pick')!;
    field.dispatchEvent(new KeyboardEvent('keydown', { key: 'ArrowDown', bubbles: true }));
    await settle(fixture);
    expect(field.getAttribute('aria-expanded')).toBe('true');
    const list = document.getElementById(field.getAttribute('aria-controls')!)!;
    list.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true }));
    await settle(fixture);
    expect(field.getAttribute('aria-expanded')).toBe('false');
    expect(document.activeElement).toBe(field);
  });
});

describe('Tooltip', () => {
  afterEach(() => vi.useRealTimers());

  it('shows the text after a moment on hover and hides it on leave', async () => {
    const { fixture, root } = await render();
    vi.useFakeTimers();
    const badge = root.querySelector<HTMLElement>('#badge')!;
    badge.dispatchEvent(new MouseEvent('mouseenter'));
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
    vi.advanceTimersByTime(500);
    fixture.detectChanges();
    const tip = document.querySelector('[role="tooltip"]')!;
    expect(tip.textContent).toBe('The key stays on this desktop');
    expect(badge.getAttribute('aria-describedby')).toBe(tip.id);
    expect(badge.hasAttribute('title')).toBe(false);

    badge.dispatchEvent(new MouseEvent('mouseleave'));
    fixture.detectChanges();
    expect(document.querySelector('[role="tooltip"]')).toBeNull();
  });
});

describe('TextMenu', () => {
  let clipboard: string;

  beforeEach(() => {
    clipboard = 'Bob';
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        readText: vi.fn(async () => clipboard),
        writeText: vi.fn(async (value: string) => void (clipboard = value)),
      },
    });
  });

  afterEach(() => document.querySelectorAll('.cdk-overlay-container').forEach((c) => (c.innerHTML = '')));

  it('takes no space in the layout (the app root is a grid)', async () => {
    const { root } = await render();
    expect(getComputedStyle(root.querySelector('app-text-menu')!).display).toBe('contents');
  });

  it('never shows the system menu, and shows nothing outside text fields', async () => {
    const { root } = await render();
    expect(rightClick(root.querySelector('#plain')!).defaultPrevented).toBe(true);
    expect(menuItems()).toEqual([]);
  });

  it('offers cut, copy, paste and select all in a text field', async () => {
    const { fixture, root } = await render();
    const input = root.querySelector<HTMLInputElement>('#name')!;
    input.setSelectionRange(0, 5);
    expect(rightClick(input).defaultPrevented).toBe(true);
    fixture.detectChanges();
    expect(menuItems()).toEqual(['Cut', 'Copy', 'Paste', 'Select all']);
  });

  it('pastes over the selection and tells the form', async () => {
    const { fixture, root, host } = await render();
    const input = root.querySelector<HTMLInputElement>('#name')!;
    input.setSelectionRange(0, 5);
    rightClick(input);
    fixture.detectChanges();
    Array.from(document.querySelectorAll<HTMLButtonElement>('.text-menu button'))
      .find((b) => b.textContent?.trim() === 'Paste')!
      .click();
    await settle(fixture);
    expect(input.value).toBe('Bob');
    expect(host.name()).toBe('Bob');
    expect(menuItems()).toEqual([]);
  });

  it('copies the selection', async () => {
    const { fixture, root } = await render();
    const input = root.querySelector<HTMLInputElement>('#name')!;
    input.setSelectionRange(1, 3);
    rightClick(input);
    fixture.detectChanges();
    Array.from(document.querySelectorAll<HTMLButtonElement>('.text-menu button'))
      .find((b) => b.textContent?.trim() === 'Copy')!
      .click();
    await settle(fixture);
    expect(clipboard).toBe('li');
  });

  it('does not copy a password out', async () => {
    const { fixture, root } = await render();
    const input = root.querySelector<HTMLInputElement>('#secret')!;
    input.setSelectionRange(0, 7);
    rightClick(input);
    fixture.detectChanges();
    expect(menuItems()).toEqual(['Cut (off)', 'Copy (off)', 'Paste', 'Select all']);
  });
});
