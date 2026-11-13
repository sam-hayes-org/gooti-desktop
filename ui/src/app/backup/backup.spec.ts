import { TestBed } from '@angular/core/testing';
import { ConfirmService } from '../shared/confirm.service';
import {
  FakeCore,
  PHRASE,
  button,
  provideFakeCore,
  setTextarea,
  settle,
  text,
} from '../testing/fake-core';
import { formatVaultSize } from './relays';
import { BackupPage } from './backup.page';

describe('BackupPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  async function render() {
    const fixture = TestBed.createComponent(BackupPage);
    await settle(fixture);
    return { fixture, root: fixture.nativeElement as HTMLElement };
  }

  it('turns backup on only after the words are written down', async () => {
    const { fixture, root } = await render();
    expect(text(root)).toContain('Backup is off');
    button(root, 'Create passphrase').click();
    await settle(fixture);
    expect(root.querySelector('[data-phrase]')?.textContent).toBe(PHRASE);

    const turnOn = button(root, 'Turn on backup');
    expect(turnOn.disabled).toBe(true);
    const box = root.querySelector<HTMLInputElement>('input[name="savedWords"]')!;
    box.checked = true;
    box.dispatchEvent(new Event('change'));
    await settle(fixture);
    expect(turnOn.disabled).toBe(false);
    turnOn.click();
    await settle(fixture);

    expect(core.enableBackup).toHaveBeenCalledWith(PHRASE, ['wss://relay.getgooti.com']);
    expect(text(root)).toContain('Back up now');
    expect(text(root)).toContain('1.2 KB');
  });

  it('restores with a passphrase and the relays in the box', async () => {
    const { fixture, root } = await render();
    button(root, 'Use an existing backup').click();
    await settle(fixture);
    setTextarea(root, 'textarea[name="restoreRelays"]', 'wss://a.example\n\n wss://b.example ');
    setTextarea(root, 'textarea[name="phrase"]', PHRASE);
    await settle(fixture);
    button(root, 'Restore from backup').click();
    await settle(fixture);
    expect(core.joinBackup).toHaveBeenCalledWith(PHRASE, ['wss://a.example', 'wss://b.example']);
    expect(text(root)).toContain('Merged the backup with this desktop.');
  });

  it('shows each relay and its reason when a restore fails', async () => {
    core.joinBackup.mockRejectedValueOnce({
      code: 'relays_failed',
      message: 'No relay could be reached.',
      failures: [{ url: 'wss://relay.getgooti.com', reason: 'Connection refused' }],
    });
    const { fixture, root } = await render();
    button(root, 'Use an existing backup').click();
    await settle(fixture);
    setTextarea(root, 'textarea[name="phrase"]', PHRASE);
    await settle(fixture);
    button(root, 'Restore from backup').click();
    await settle(fixture);
    expect(text(root)).toContain('No relay could be reached.');
    expect(text(root)).toContain('wss://relay.getgooti.com Connection refused');
  });

  it('backs up now and lists what this desktop took and sent', async () => {
    core.backup = { relays: ['wss://relay.getgooti.com'], autoBackup: false, contentChars: 5000, desktopCurrent: true };
    core.backUpNow.mockResolvedValueOnce({
      result: 'backedUp',
      took: ['Added identity Bob'],
      sent: ['Renamed Alice to Alicia'],
    });
    const { fixture, root } = await render();
    button(root, 'Back up now').click();
    await settle(fixture);
    expect(core.setBackupRelays).toHaveBeenCalledWith(['wss://relay.getgooti.com']);
    expect(core.backUpNow).toHaveBeenCalledOnce();
    const dialog = root.querySelector<HTMLElement>('app-dialog')!;
    expect(text(dialog)).toContain('Backed up');
    const labels = Array.from(dialog.querySelectorAll('.field-label')).map((l) => l.textContent);
    const lines = Array.from(dialog.querySelectorAll('.changes li')).map((l) => l.textContent);
    expect(labels).toEqual(['On this desktop', 'Sent']);
    expect(lines).toEqual(['Added identity Bob', 'Renamed Alice to Alicia']);
  });

  it('turns automatic backup on and backup off after the confirmation', async () => {
    core.backup = { relays: ['wss://relay.getgooti.com'], autoBackup: false, contentChars: 5000, desktopCurrent: true };
    const { fixture, root } = await render();
    const auto = root.querySelector<HTMLInputElement>('input[name="autoBackup"]')!;
    auto.checked = true;
    auto.dispatchEvent(new Event('change'));
    await settle(fixture);
    expect(core.setAutoBackup).toHaveBeenCalledWith(true);

    const confirm = TestBed.inject(ConfirmService);
    button(root, 'Turn off backup').click();
    expect(confirm.pending()?.message).toBe('Turn off backup on this desktop?');
    confirm.answer(true);
    await settle(fixture);
    expect(core.disableBackup).toHaveBeenCalledOnce();
    expect(text(root)).toContain('Backup is off');
  });

  it('formats the vault size like the extension', () => {
    expect(formatVaultSize(999)).toBe('999 B');
    expect(formatVaultSize(1234)).toBe('1.2 KB');
    expect(formatVaultSize(56_789)).toBe('57 KB');
    expect(formatVaultSize(1_500_000)).toBe('1.5 MB');
  });
});
