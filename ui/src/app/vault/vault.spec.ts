import { TestBed } from '@angular/core/testing';
import { ConfirmService } from '../shared/confirm.service';
import {
  FakeCore,
  PHRASE,
  button,
  provideFakeCore,
  setInput,
  setTextarea,
  settle,
  text,
} from '../testing/fake-core';
import { VaultStart } from './vault-start';
import { VaultUnlock } from './vault-unlock';

describe('Vault screens', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  it('creates a vault only with at least 4 characters', async () => {
    const fixture = TestBed.createComponent(VaultStart);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    root.querySelector<HTMLButtonElement>('[data-choice="create"]')!.click();
    await settle(fixture);
    expect(text(root)).toContain('Choose a password');

    const submit = root.querySelector<HTMLButtonElement>('button.submit')!;
    setInput(root, 'input[type="password"]', '123');
    await settle(fixture);
    expect(submit.disabled).toBe(true);

    setInput(root, 'input[type="password"]', '1234');
    await settle(fixture);
    expect(submit.disabled).toBe(false);
    submit.click();
    await settle(fixture);
    expect(core.createVault).toHaveBeenCalledWith('1234');
  });

  it('shows "Invalid password" after a wrong password', async () => {
    core.unlock.mockRejectedValueOnce({ code: 'invalid_password', message: 'Invalid password' });
    const fixture = TestBed.createComponent(VaultUnlock);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    setInput(root, 'input[type="password"]', 'wrong');
    await settle(fixture);
    root.querySelector<HTMLButtonElement>('button.submit')!.click();
    await settle(fixture);
    expect(core.unlock).toHaveBeenCalledWith('wrong');
    expect(root.querySelector('[role="alert"]')?.textContent).toBe('Invalid password');
  });

  it('resets only after the confirmation', async () => {
    const fixture = TestBed.createComponent(VaultUnlock);
    await settle(fixture);
    const confirm = TestBed.inject(ConfirmService);
    (fixture.nativeElement as HTMLElement).querySelector<HTMLButtonElement>('button.reset')!.click();
    expect(confirm.pending()?.message).toBe('Reset Gooti? All data on this desktop will be deleted.');
    confirm.answer(true);
    await settle(fixture);
    expect(core.reset).toHaveBeenCalledOnce();
  });

  it('restores a vault from the relays with a passphrase', async () => {
    const fixture = TestBed.createComponent(VaultStart);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    root.querySelector<HTMLButtonElement>('[data-choice="restore"]')!.click();
    await settle(fixture);
    expect(text(root)).toContain('Restore from backup');
    expect(root.querySelector<HTMLTextAreaElement>('textarea[name="relays"]')!.value).toBe(
      'wss://relay.getgooti.com',
    );

    const submit = button(root, 'Restore from backup');
    setInput(root, 'input[type="password"]', '1234');
    await settle(fixture);
    expect(submit.disabled).toBe(true);
    setTextarea(root, 'textarea[name="phrase"]', PHRASE);
    await settle(fixture);
    submit.click();
    await settle(fixture);
    expect(core.restoreVault).toHaveBeenCalledWith('1234', PHRASE, ['wss://relay.getgooti.com']);
  });

  it('says when the relays have no vault for the words', async () => {
    core.restoreVault.mockRejectedValueOnce({
      code: 'no_remote_vault',
      message: 'No vault found for this passphrase on these relays.',
    });
    const fixture = TestBed.createComponent(VaultStart);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    root.querySelector<HTMLButtonElement>('[data-choice="restore"]')!.click();
    await settle(fixture);
    setInput(root, 'input[type="password"]', '1234');
    setTextarea(root, 'textarea[name="phrase"]', PHRASE);
    await settle(fixture);
    button(root, 'Restore from backup').click();
    await settle(fixture);
    expect(root.querySelector('[role="alert"]')?.textContent).toBe(
      'No vault found for this passphrase on these relays.',
    );
  });
});
