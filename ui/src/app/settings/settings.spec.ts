import { TestBed } from '@angular/core/testing';
import {
  ALICE,
  FakeCore,
  button,
  choose,
  provideFakeCore,
  selectLabel,
  setTextarea,
  settle,
  text,
} from '../testing/fake-core';
import { ConfirmService } from '../shared/confirm.service';
import { SettingsPage } from './settings.page';

describe('SettingsPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  async function render() {
    const fixture = TestBed.createComponent(SettingsPage);
    await settle(fixture);
    return { fixture, root: fixture.nativeElement as HTMLElement };
  }

  function check(root: HTMLElement, name: string, checked: boolean) {
    const box = root.querySelector<HTMLInputElement>(`input[name="${name}"]`)!;
    box.checked = checked;
    box.dispatchEvent(new Event('change'));
  }

  it('shows the defaults: lock on screen lock and sleep, never after idle', async () => {
    const { root } = await render();
    expect(root.querySelector<HTMLInputElement>('input[name="unlockAtLogin"]')!.checked).toBe(false);
    expect(root.querySelector<HTMLInputElement>('input[name="lockOnScreenLock"]')!.checked).toBe(true);
    expect(root.querySelector<HTMLInputElement>('input[name="lockOnSleep"]')!.checked).toBe(true);
    expect(selectLabel(root, 'autoLock')).toBe('Never');
  });

  it('keeps the checkbox off when the keychain fails', async () => {
    core.setUnlockAtLogin.mockRejectedValueOnce({
      code: 'keychain',
      message: 'The keychain is not available: no Secret Service',
    });
    const { fixture, root } = await render();
    check(root, 'unlockAtLogin', true);
    await settle(fixture);
    expect(core.setUnlockAtLogin).toHaveBeenCalledWith(true);
    expect(text(root)).toContain('The keychain is not available');
    expect(root.querySelector<HTMLInputElement>('input[name="unlockAtLogin"]')!.checked).toBe(false);
  });

  it('saves the default relays for apps', async () => {
    const { fixture, root } = await render();
    const box = root.querySelector<HTMLTextAreaElement>('textarea[name="defaultRelays"]')!;
    expect(box.value).toBe('wss://relay.getgooti.com');
    setTextarea(root, 'textarea[name="defaultRelays"]', 'wss://a.example');
    await settle(fixture);
    button(root, 'Save relays').click();
    await settle(fixture);
    expect(core.bunkerSetRelays).toHaveBeenCalledWith(['wss://a.example']);
    expect(text(root)).toContain('Updated the relays');
  });

  it('sends only the lock setting that changed', async () => {
    const { fixture, root } = await render();
    check(root, 'lockOnSleep', false);
    await settle(fixture);
    expect(core.changeLockSettings).toHaveBeenCalledWith({ lockOnSleep: false });

    await choose(fixture, root, 'autoLock', '15 minutes');
    expect(core.changeLockSettings).toHaveBeenLastCalledWith({ autoLockMinutes: 15 });
  });

  it('changes the password with at least 4 characters', async () => {
    const { fixture, root } = await render();
    const button = Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
      (b) => b.textContent?.trim() === 'Change password',
    )!;
    const current = root.querySelector<HTMLInputElement>('input[name="currentPassword"]')!;
    const next = root.querySelector<HTMLInputElement>('input[name="newPassword"]')!;
    current.value = 'old1';
    current.dispatchEvent(new Event('input'));
    next.value = 'new';
    next.dispatchEvent(new Event('input'));
    await settle(fixture);
    expect(button.disabled).toBe(true);
    next.value = 'new1';
    next.dispatchEvent(new Event('input'));
    await settle(fixture);
    button.click();
    await settle(fixture);
    expect(core.changePassword).toHaveBeenCalledWith('old1', 'new1');
    expect(text(root)).toContain('Your password is changed.');
  });

  it('changes the app settings and looks for updates', async () => {
    const { fixture, root } = await render();
    check(root, 'keepRunning', false);
    await settle(fixture);
    expect(core.changeAppSettings).toHaveBeenCalledWith({ keepRunning: false });
    check(root, 'autostart', true);
    await settle(fixture);
    expect(core.setAutostart).toHaveBeenCalledWith(true);
    expect(root.querySelector<HTMLInputElement>('input[name="autostart"]')!.checked).toBe(true);

    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.trim() === 'Check now')!
      .click();
    await settle(fixture);
    expect(text(root)).toContain('Gooti is up to date.');
  });

  it('explains that without a tray closing the window quits', async () => {
    core.autostartInfo = { enabled: false, hasTray: false };
    const { root } = await render();
    expect(root.querySelector<HTMLInputElement>('input[name="keepRunning"]')!.disabled).toBe(true);
    expect(text(root)).toContain('closing the window quits Gooti');
  });

  describe('Reset with Desktop only identities', () => {
    beforeEach(() => {
      core.identities.set([
        { ...ALICE, id: 'alice', nick: 'Alice', custody: 'desktop' },
        { ...ALICE, id: 'bob', nick: 'Bob', custody: 'shared' },
      ]);
    });

    function dialog(): HTMLElement {
      return document.querySelector<HTMLElement>('[role="dialog"]')!;
    }

    it('asks only the short question when the backup has their keys', async () => {
      core.backup = { relays: ['wss://relay.getgooti.com'], autoBackup: true, contentChars: 1, desktopCurrent: true };
      const { fixture, root } = await render();
      button(root, 'Reset Gooti').click();
      await settle(fixture);
      expect(TestBed.inject(ConfirmService).pending()?.message).toBe(
        'Reset Gooti? All data on this desktop will be deleted.',
      );
      expect(document.querySelector('.reset-list')).toBeNull();
    });

    it('names them, backs up first, then resets', async () => {
      core.backup = { relays: ['wss://relay.getgooti.com'], autoBackup: true, contentChars: 1, desktopCurrent: false };
      core.backUpNow.mockImplementationOnce(async () => {
        core.backup = { ...core.backup!, desktopCurrent: true };
        return { result: 'backedUp', took: [], sent: [] };
      });
      const { fixture, root } = await render();
      button(root, 'Reset Gooti').click();
      await settle(fixture);
      expect(text(dialog())).toContain('Your backup does not have everything for these identities yet.');
      expect(Array.from(dialog().querySelectorAll('.reset-list .nick')).map((n) => n.textContent)).toEqual([
        'Alice',
      ]);

      button(dialog(), 'Back up now').click();
      await settle(fixture);
      expect(core.backUpNow).toHaveBeenCalled();
      // The keys stay Desktop only: the backup keeps them for Gooti Desktops.
      expect(core.setCustody).not.toHaveBeenCalled();
      expect(text(dialog())).toContain('Their keys are in your backup now.');
      expect(core.reset).not.toHaveBeenCalled();

      button(dialog(), 'Reset').click();
      await settle(fixture);
      expect(core.reset).toHaveBeenCalled();
    });

    it('shows their keys, and offers no backup step while backup is off', async () => {
      const { fixture, root } = await render();
      button(root, 'Reset Gooti').click();
      await settle(fixture);
      expect(text(dialog())).toContain('Backup is off, so only saving each nsec keeps these keys.');
      expect(Array.from(dialog().querySelectorAll('button')).map((b) => b.textContent?.trim())).not.toContain(
        'Back up now',
      );

      button(dialog(), 'Show private keys').click();
      await settle(fixture);
      expect(core.revealNsec).toHaveBeenCalledExactlyOnceWith('alice');
      expect(dialog().querySelector<HTMLInputElement>('.reset-list input')!.value).toBe('nsec1test');

      button(dialog(), 'Cancel').click();
      await settle(fixture);
      expect(document.querySelector('[role="dialog"]')).toBeNull();
      expect(core.reset).not.toHaveBeenCalled();
    });

    it('keeps the list when the backup fails or is still behind', async () => {
      core.backup = { relays: ['wss://relay.getgooti.com'], autoBackup: true, contentChars: 1, desktopCurrent: false };
      core.backUpNow.mockRejectedValueOnce({ code: 'relays', message: 'No relay answered.' });
      const { fixture, root } = await render();
      button(root, 'Reset Gooti').click();
      await settle(fixture);
      button(dialog(), 'Back up now').click();
      await settle(fixture);
      expect(text(dialog())).toContain('No relay answered.');
      expect(text(dialog())).toContain('Your backup does not have everything for these identities yet.');
      expect(button(dialog(), 'Reset anyway')).toBeTruthy();

      button(dialog(), 'Back up now').click();
      await settle(fixture);
      expect(text(dialog())).toContain('Some changes are not in your backup yet. Try again.');
      expect(button(dialog(), 'Reset anyway')).toBeTruthy();
    });
  });
});
