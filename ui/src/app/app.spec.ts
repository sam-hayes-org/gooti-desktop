import { vi } from 'vitest';
import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { App, BACKUP_BAR_MIN_MS } from './app';
import { routes } from './app.routes';
import { NAV_ITEMS } from './nav';
import { ConfirmService } from './shared/confirm.service';
import { BunkerAsk, BunkerConnection, BunkerInfo, RelayStatus } from './core/core.service';
import { ALICE, FakeCore, provideFakeCore, settle, text } from './testing/fake-core';

describe('App', () => {
  let core: FakeCore;

  beforeEach(async () => {
    core = new FakeCore();
    await TestBed.configureTestingModule({
      imports: [App],
      providers: [provideRouter(routes), provideFakeCore(core)],
    }).compileComponents();
  });

  async function render() {
    const fixture = TestBed.createComponent(App);
    await settle(fixture);
    return fixture;
  }

  it('shows the first vault screen when there is no vault', async () => {
    core.status.set('noVault');
    const fixture = await render();
    expect(text(fixture.nativeElement)).toContain('Your identities start here.');
    expect(fixture.nativeElement.querySelector('.sidebar')).toBeNull();
  });

  it('asks for the password when the vault is locked', async () => {
    core.status.set('locked');
    const fixture = await render();
    expect(text(fixture.nativeElement)).toContain('Unlock Gooti');
    expect(fixture.nativeElement.querySelector('.sidebar')).toBeNull();
  });

  it('shows the backup bar while a backup runs, also before the vault is open', async () => {
    const fixture = await render();
    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector('.backup-bar')).toBeNull();

    core.backingUp.set(true);
    await settle(fixture);
    const bar = root.querySelector('.backup-bar')!;
    expect(bar.getAttribute('role')).toBe('progressbar');
    // Out of the layout grid, like the right-click menu.
    expect(getComputedStyle(bar).position).toBe('fixed');

    core.status.set('noVault');
    await settle(fixture);
    expect(root.querySelector('.backup-bar')).not.toBeNull();

    core.backingUp.set(false);
    await settle(fixture);
  });

  it('keeps the backup bar for at least 800 ms, and no longer after a long backup', async () => {
    const fixture = await render();
    const root = fixture.nativeElement as HTMLElement;
    vi.useFakeTimers();
    try {
      const shown = () => {
        fixture.detectChanges();
        return root.querySelector('.backup-bar') !== null;
      };

      // A fast backup.
      core.backingUp.set(true);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(100);
      core.backingUp.set(false);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(BACKUP_BAR_MIN_MS - 101);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(1);
      expect(shown()).toBe(false);

      // Another backup during the extra time keeps it on, without a gap.
      core.backingUp.set(true);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(50);
      core.backingUp.set(false);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(100);
      core.backingUp.set(true);
      expect(shown()).toBe(true);
      vi.advanceTimersByTime(BACKUP_BAR_MIN_MS);
      expect(shown()).toBe(true);

      // It has run longer than the minimum: off at once when it ends.
      core.backingUp.set(false);
      expect(shown()).toBe(false);
    } finally {
      vi.useRealTimers();
    }
  });

  it('shows no backup bar in the approval window', async () => {
    core.isApprovalWindow.set(true);
    core.backingUp.set(true);
    const fixture = await render();
    expect((fixture.nativeElement as HTMLElement).querySelector('.backup-bar')).toBeNull();
  });

  it('lists every section in the sidebar once unlocked', async () => {
    const fixture = await render();
    const links = Array.from(
      (fixture.nativeElement as HTMLElement).querySelectorAll('nav .nav-link .label'),
    ).map((link) => link.textContent?.trim());
    expect(links).toEqual(NAV_ITEMS.map((item) => item.label));
  });

  describe('sidebar numbers', () => {
    const nav = (root: HTMLElement, path: string) => root.querySelector<HTMLElement>(`[data-nav="${path}"]`)!;
    const app = (id: string) => ({ id }) as BunkerConnection;
    const relay = (url: string, state: RelayStatus['state']): RelayStatus => ({ url, state });

    it('counts identities and follows changes, such as a backup bringing one in', async () => {
      core.identities.set([{ ...ALICE }]);
      const fixture = await render();
      const root = fixture.nativeElement as HTMLElement;
      expect(nav(root, 'identities').querySelector('.total')?.textContent?.trim()).toBe('1');

      core.identities.set([{ ...ALICE }, { ...ALICE, id: 'bob', nick: 'Bob' }]);
      await settle(fixture);
      expect(nav(root, 'identities').querySelector('.total')?.textContent?.trim()).toBe('2');
    });

    it('shows the apps and how many relays are connected, with a color', async () => {
      const fixture = await render();
      const root = fixture.nativeElement as HTMLElement;
      const bunker = (state: Partial<BunkerInfo>) =>
        core.bunkerState.set({ relays: [], linkRelays: [], paused: false, connections: [], relayStatus: [], ...state });

      bunker({
        connections: [app('a'), app('b'), app('c')],
        relayStatus: [relay('wss://a', 'connected'), relay('wss://b', 'connecting')],
      });
      await settle(fixture);
      const link = nav(root, 'connections');
      expect(link.querySelector('.total')?.textContent?.trim()).toBe('3');
      const pill = link.querySelector<HTMLElement>('.relays')!;
      expect(pill.textContent?.trim()).toBe('1/2');
      expect(pill.dataset['tone']).toBe('partial');

      bunker({ connections: [app('a')], relayStatus: [relay('wss://a', 'connected'), relay('wss://b', 'connected')] });
      await settle(fixture);
      expect(link.querySelector('.total')?.textContent?.trim()).toBe('1');
      expect(link.querySelector<HTMLElement>('.relays')!.dataset['tone']).toBe('up');

      bunker({ relayStatus: [relay('wss://a', 'failed')] });
      await settle(fixture);
      expect(link.querySelector('.relays')?.textContent?.trim()).toBe('0/1');
      expect(link.querySelector<HTMLElement>('.relays')!.dataset['tone']).toBe('down');

      bunker({ paused: true, relayStatus: [] });
      await settle(fixture);
      expect(link.querySelector('.relays')?.textContent?.trim()).toBe('Paused');

      bunker({ relayStatus: [] });
      await settle(fixture);
      expect(link.querySelector('.relays')).toBeNull();
    });

    it('shows waiting requests in place of the number of apps', async () => {
      const fixture = await render();
      const root = fixture.nativeElement as HTMLElement;
      core.bunkerState.set({ relays: [], linkRelays: [], paused: false, connections: [app('a'), app('b')], relayStatus: [] });
      core.pendingAsks.set([{ key: 'k1' } as BunkerAsk]);
      await settle(fixture);
      const link = nav(root, 'connections');
      expect(link.querySelector('.count')?.textContent?.trim()).toBe('1');
      expect(link.querySelector('.total')).toBeNull();
    });
  });

  it('opens Identities first and marks it active', async () => {
    const fixture = await render();
    await TestBed.inject(Router).navigateByUrl('/');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector('.page-title')?.textContent).toBe('Identities');
    expect(root.querySelector('.nav-link.active')?.getAttribute('data-nav')).toBe('identities');
  });

  it('locks only after the confirmation', async () => {
    const fixture = await render();
    const confirm = TestBed.inject(ConfirmService);
    fixture.nativeElement.querySelector('[data-action="lock"]').click();
    await settle(fixture);
    expect(confirm.pending()?.message).toBe("Lock Gooti? Apps can't sign while Gooti is locked.");
    confirm.answer(false);
    await settle(fixture);
    expect(core.lock).not.toHaveBeenCalled();

    fixture.nativeElement.querySelector('[data-action="lock"]').click();
    await settle(fixture);
    confirm.answer(true);
    await settle(fixture);
    expect(core.lock).toHaveBeenCalledOnce();
    expect(text(fixture.nativeElement)).toContain('Unlock Gooti');
  });

  it('opens Connect new app when a nostrconnect link arrives', async () => {
    const fixture = await render();
    const router = TestBed.inject(Router);
    core.connectLink.set('nostrconnect://abc?secret=x');
    await settle(fixture);
    expect(router.url).toBe('/connections/new');
  });

  it('opens pages with Ctrl+1 … Ctrl+6 and asks to lock with Ctrl+L', async () => {
    const fixture = await render();
    const router = TestBed.inject(Router);
    const key = (code: string) =>
      document.dispatchEvent(new KeyboardEvent('keydown', { code, ctrlKey: true, bubbles: true }));
    key('Digit2');
    await settle(fixture);
    expect(router.url).toBe('/connections');
    key('Digit6');
    await settle(fixture);
    expect(router.url).toBe('/info');
    document.dispatchEvent(new KeyboardEvent('keydown', { code: 'Digit3', bubbles: true }));
    await settle(fixture);
    // Without Ctrl nothing happens.
    expect(router.url).toBe('/info');

    key('KeyL');
    await settle(fixture);
    const confirm = TestBed.inject(ConfirmService);
    expect(confirm.pending()?.message).toContain('Lock Gooti?');
    // While the question is open, shortcuts wait.
    key('Digit1');
    await settle(fixture);
    expect(router.url).toBe('/info');
    confirm.answer(false);
    await settle(fixture);
    expect(core.lock).not.toHaveBeenCalled();
  });

  it('reports input for the idle lock, at most every 20 seconds', async () => {
    const fixture = await render();
    document.dispatchEvent(new Event('keydown'));
    document.dispatchEvent(new Event('pointerdown'));
    expect(core.touch).toHaveBeenCalledOnce();
    fixture.destroy();
  });

  it('offers an update: installs an AppImage, points other installs to their package manager', async () => {
    core.update.set({
      version: '0.2.0',
      currentVersion: '0.1.0',
      notes: null,
      canInstall: false,
      releasesUrl: 'https://github.com/sam-hayes-org/gooti-desktop/releases/latest',
    });
    const fixture = await render();
    const root = fixture.nativeElement as HTMLElement;
    expect(text(root)).toContain('Gooti 0.2.0 is available. You have 0.1.0.');
    expect(text(root)).toContain('Update it with your package manager.');
    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.trim() === 'Release page')!
      .click();
    expect(core.openReleases).toHaveBeenCalledOnce();

    core.update.update((u) => u && { ...u, canInstall: true });
    await settle(fixture);
    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.trim() === 'Install and restart')!
      .click();
    await settle(fixture);
    expect(core.installUpdate).toHaveBeenCalledOnce();
  });
});
