import { TestBed } from '@angular/core/testing';
import { provideRouter, Router } from '@angular/router';
import { ConfirmService } from '../shared/confirm.service';
import { ALICE, FakeCore, button, provideFakeCore, setInput, setTextarea, settle, text } from '../testing/fake-core';
import { IdentitiesPage } from './identities.page';
import { IdentityDetailPage } from './identity-detail.page';
import { IdentityNewPage } from './identity-new.page';

describe('Identities', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    TestBed.configureTestingModule({
      providers: [provideRouter([]), provideFakeCore(core)],
    });
  });

  it('shows an empty card, then the identities', async () => {
    const fixture = TestBed.createComponent(IdentitiesPage);
    await settle(fixture);
    expect(text(fixture.nativeElement)).toContain('No identity yet');

    core.identities.set([ALICE]);
    await settle(fixture);
    expect(text(fixture.nativeElement)).toContain('Alice');
    expect(text(fixture.nativeElement)).not.toContain('No identity yet');
  });

  it('marks an identity whose key no Gooti Desktop holds', async () => {
    core.identities.set([
      { ...ALICE, hasPrivateKey: false, isBunker: true, keyMissing: true },
      { ...ALICE, id: 'bob', nick: 'Bob', hasPrivateKey: false, isBunker: true },
    ]);
    const list = TestBed.createComponent(IdentitiesPage);
    await settle(list);
    const badges = Array.from((list.nativeElement as HTMLElement).querySelectorAll('.badge-bunker'));
    expect(badges.map((b) => b.textContent?.trim())).toEqual(['Key missing', 'Bunker']);

    const detail = TestBed.createComponent(IdentityDetailPage);
    detail.componentRef.setInput('id', 'alice');
    await settle(detail);
    const note = text(detail.nativeElement as HTMLElement);
    expect(note).toContain('No Gooti Desktop with your passphrase holds the private key');
    expect(note).toContain('The key is lost, unless you saved its nsec.');
    expect(note).not.toContain('A browser links this identity to a bunker.');
  });

  it('offers to sign here for identities another Gooti Desktop answers for', async () => {
    core.identities.set([
      { ...ALICE, custody: 'desktop', servedElsewhere: true },
      { ...ALICE, id: 'bob', nick: 'Bob', servedElsewhere: true },
      { ...ALICE, id: 'carol', nick: 'Carol' },
    ]);
    const list = TestBed.createComponent(IdentitiesPage);
    await settle(list);
    const root = list.nativeElement as HTMLElement;
    expect(text(root)).toContain('Alice and Bob sign on another Gooti Desktop');
    button(root, 'Sign here').click();
    await settle(list);
    expect(core.serveHere).toHaveBeenCalledExactlyOnceWith(['alice', 'bob']);
    expect(root.querySelector('.served-card')).toBeNull();

    core.identities.update((all) => all.map((i) => (i.id === 'bob' ? { ...i, servedElsewhere: true } : i)));
    const detail = TestBed.createComponent(IdentityDetailPage);
    detail.componentRef.setInput('id', 'bob');
    await settle(detail);
    const page = detail.nativeElement as HTMLElement;
    expect(text(page)).toContain('Another Gooti Desktop answers apps and browsers for this identity.');
    button(page, 'Sign here instead').click();
    await settle(detail);
    expect(core.serveHere).toHaveBeenLastCalledWith(['bob']);
    expect(page.querySelector('.served-elsewhere')).toBeNull();
  });

  it('generates a key unless one is imported', async () => {
    const fixture = TestBed.createComponent(IdentityNewPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const save = root.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    expect(save.disabled).toBe(true);

    setInput(root, 'input[name="nick"]', 'Sam');
    await settle(fixture);
    expect(save.disabled).toBe(false);
    save.click();
    await settle(fixture);
    expect(core.createIdentity).toHaveBeenCalledWith('Sam', undefined, undefined, 'desktop', ['wss://relay.getgooti.com']);
  });

  it('asks for the password of an ncryptsec key', async () => {
    const fixture = TestBed.createComponent(IdentityNewPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    setInput(root, 'input[name="nick"]', 'Sam');
    const importChoice = root.querySelector<HTMLInputElement>('input[value="import"]')!;
    importChoice.click();
    await settle(fixture);

    const save = root.querySelector<HTMLButtonElement>('button[type="submit"]')!;
    setInput(root, 'input[name="key"]', 'ncryptsec1abc');
    await settle(fixture);
    expect(root.querySelector('input[name="keyPassword"]')).not.toBeNull();
    expect(save.disabled).toBe(true);

    setInput(root, 'input[name="keyPassword"]', 'secret');
    await settle(fixture);
    save.click();
    await settle(fixture);
    expect(core.createIdentity).toHaveBeenCalledWith('Sam', 'ncryptsec1abc', 'secret', 'desktop', ['wss://relay.getgooti.com']);
  });

  it('shows the core error, such as a duplicate key', async () => {
    core.createIdentity.mockRejectedValueOnce({
      code: 'duplicate_identity',
      message: 'An identity with this public key already exists: Alice',
    });
    const fixture = TestBed.createComponent(IdentityNewPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    setInput(root, 'input[name="nick"]', 'Sam');
    await settle(fixture);
    root.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(fixture);
    expect(root.querySelector('[role="alert"]')?.textContent).toBe(
      'An identity with this public key already exists: Alice',
    );
  });

  it('shows the private key only after Show, and deletes after the confirmation', async () => {
    core.identities.set([ALICE]);
    const fixture = TestBed.createComponent(IdentityDetailPage);
    fixture.componentRef.setInput('id', 'alice');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(root.innerHTML).not.toContain('nsec1test');

    root.querySelector<HTMLButtonElement>('[aria-label="Show private key"]')!.click();
    await settle(fixture);
    expect(core.revealNsec).toHaveBeenCalledWith('alice');
    expect(root.querySelector<HTMLInputElement>('[aria-label="Private key (nsec)"]')!.value).toBe(
      'nsec1test',
    );

    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    const confirm = TestBed.inject(ConfirmService);
    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((button) => button.textContent?.trim() === 'Delete identity')!
      .click();
    expect(confirm.pending()?.message).toBe('Delete this identity?');
    confirm.answer(true);
    await settle(fixture);
    expect(core.deleteIdentity).toHaveBeenCalledWith('alice');
    expect(navigate).toHaveBeenCalledWith('/identities');
  });

  it('keeps the key on this desktop unless that is unticked', async () => {
    const fixture = TestBed.createComponent(IdentityNewPage);
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const box = root.querySelector<HTMLInputElement>('input[name="desktopOnly"]')!;
    expect(box.checked).toBe(true);
    box.checked = false;
    box.dispatchEvent(new Event('change'));
    setInput(root, 'input[name="nick"]', 'Sam');
    await settle(fixture);
    root.querySelector<HTMLButtonElement>('button[type="submit"]')!.click();
    await settle(fixture);
    expect(root.querySelector('textarea[name="browserRelays"]')).toBeNull();
    expect(core.createIdentity).toHaveBeenCalledWith('Sam', undefined, undefined, 'shared', undefined);
  });

  it('moves a key to the desktop on chosen relays, and back after a confirmation', async () => {
    core.identities.set([ALICE]);
    const fixture = TestBed.createComponent(IdentityDetailPage);
    fixture.componentRef.setInput('id', 'alice');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const confirm = TestBed.inject(ConfirmService);
    expect(text(root)).toContain('Browsers and desktops');

    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.trim() === 'Move key to desktop')!
      .click();
    await settle(fixture);
    expect(text(root)).toContain('Copies made before, such as exports or old backups, are not deleted.');
    const box = root.querySelector<HTMLTextAreaElement>('textarea[name="browserRelays"]')!;
    expect(box.value).toBe('wss://relay.getgooti.com');
    setTextarea(root, 'textarea[name="browserRelays"]', 'wss://mine.example');
    await settle(fixture);
    button(root, 'Move key').click();
    await settle(fixture);
    expect(core.setCustody).toHaveBeenCalledWith('alice', 'desktop', ['wss://mine.example']);
    expect(text(root)).toContain('Desktop only');
    expect(text(root)).toContain('Sites send their requests straight to the desktop');
    expect(root.querySelector('input[name="askSite"]')).toBeNull();

    Array.from(root.querySelectorAll<HTMLButtonElement>('button'))
      .find((b) => b.textContent?.trim() === 'Use in browsers again')!
      .click();
    expect(confirm.pending()?.message).toBe(
      'Use this key in browsers again? This puts the private key back on every browser and desktop with your passphrase.',
    );
    confirm.answer(true);
    await settle(fixture);
    expect(core.setCustody).toHaveBeenLastCalledWith('alice', 'shared');
  });
});
