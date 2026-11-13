import { TestBed } from '@angular/core/testing';
import { ConfirmService } from '../shared/confirm.service';
import { Router, provideRouter } from '@angular/router';
import {
  ALICE,
  ASK,
  CONNECTION,
  FakeCore,
  button,
  choose,
  openSelect,
  provideFakeCore,
  selectLabel,
  setTextarea,
  settle,
  text,
} from '../testing/fake-core';
import { ApprovalPanel } from './approval-panel';
import { ConnectionPage } from './connection.page';
import { ConnectionsPage } from './connections.page';
import { ConnectionNewPage } from './connection-new.page';

describe('ConnectionsPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    core.identities.set([ALICE]);
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideFakeCore(core)] });
  });

  async function render() {
    const fixture = TestBed.createComponent(ConnectionsPage);
    await settle(fixture);
    return { fixture, root: fixture.nativeElement as HTMLElement };
  }

  afterEach(() => TestBed.resetTestingModule());

  it('lists apps and removes one after the confirmation', async () => {
    core.bunker.connections = [CONNECTION];
    const { fixture, root } = await render();
    expect(text(root)).toContain('Coracle');
    expect(text(root)).toContain('Asks every time');
    expect(text(root)).toContain('Signs as Alice · https://coracle.social');
    const confirm = TestBed.inject(ConfirmService);
    button(root, 'Remove').click();
    expect(confirm.pending()?.message).toBe('Remove this app?');
    confirm.answer(true);
    await settle(fixture);
    expect(core.bunkerRevoke).toHaveBeenCalledWith('c1');
    expect(text(root)).toContain('No app yet');
  });

  it('offers no Remove for the browsers of a Desktop only identity', async () => {
    core.bunker.connections = [{ ...CONNECTION, name: 'Gooti browsers', browsers: true, trust: 'basic' }];
    const { root } = await render();
    expect(text(root)).toContain('Gooti browsers');
    expect(text(root)).toContain('Basic');
    expect(Array.from(root.querySelectorAll('button')).some((b) => b.textContent?.trim() === 'Remove')).toBe(false);
  });

  it('pauses all apps and shows each relay state', async () => {
    core.bunker.relayStatus = [
      { url: 'wss://relay.getgooti.com', state: 'connected' },
      { url: 'wss://relay.nsec.app', state: 'failed', reason: 'Connection refused' },
    ];
    const { fixture, root } = await render();
    expect(text(root)).toContain('wss://relay.nsec.app Connection refused');
    const box = root.querySelector<HTMLInputElement>('input[name="paused"]')!;
    box.checked = true;
    box.dispatchEvent(new Event('change'));
    await settle(fixture);
    expect(core.bunkerSetPaused).toHaveBeenCalledWith(true);
    expect(text(root)).toContain('All apps are paused.');
  });

  it('opens Connect new app from the heading and shows its note on return', async () => {
    history.replaceState({ notice: 'The app is connected.' }, '');
    const { root } = await render();
    history.replaceState(null, '');
    expect(root.querySelector('[data-action="new-connection"]')?.textContent?.trim()).toBe('Connect new app');
    expect(text(root)).toContain('The app is connected.');
    expect(text(root)).not.toContain('Show bunker link');
  });

  it('lists every relay it listens on with the apps that use it', async () => {
    core.bunker.connections = [{ ...CONNECTION, relays: ['wss://relay.getgooti.com', 'wss://app.relay'] }];
    core.bunker.linkRelays = ['wss://relay.getgooti.com'];
    core.bunker.relayStatus = [
      { url: 'wss://relay.getgooti.com', state: 'connected' },
      { url: 'wss://app.relay', state: 'connecting' },
    ];
    const { root } = await render();
    expect(text(root)).toContain('wss://relay.getgooti.com Connected · Coracle, a new link');
    expect(text(root)).toContain('wss://app.relay Connecting… · Coracle');
    expect(root.querySelector('textarea[name="relays"]')).toBeNull();
  });

  it('says when another Gooti Desktop answers the apps', async () => {
    core.bunker.connections = [{ ...CONNECTION, servedElsewhere: true }];
    core.bunker.relayStatus = [];
    const { root } = await render();
    expect(text(root)).toContain('Answered by another Gooti Desktop');
    expect(text(root)).toContain('Another Gooti Desktop answers these apps.');
    expect(text(root)).not.toContain('once an app or a link needs them');
  });

  it('says nothing about another desktop for apps answered here', async () => {
    core.bunker.connections = [{ ...CONNECTION, servedElsewhere: false }];
    core.bunker.relayStatus = [];
    const { root } = await render();
    expect(text(root)).not.toContain('another Gooti Desktop');
    expect(text(root)).toContain('Gooti connects to relays once an app or a link needs them.');
  });
});

describe('ConnectionNewPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    core.identities.set([ALICE]);
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideFakeCore(core)] });
  });

  async function render() {
    const fixture = TestBed.createComponent(ConnectionNewPage);
    const navigate = vi.spyOn(TestBed.inject(Router), 'navigateByUrl').mockResolvedValue(true);
    await settle(fixture);
    return { fixture, root: fixture.nativeElement as HTMLElement, navigate };
  }

  afterEach(() => TestBed.resetTestingModule());

  it('shows a single-use bunker link with a QR code', async () => {
    const { fixture, root } = await render();
    button(root, 'Show bunker link').click();
    await settle(fixture);
    await new Promise((resolve) => setTimeout(resolve, 50));
    await settle(fixture);
    expect(core.bunkerLink).toHaveBeenCalledWith('alice', ['wss://relay.getgooti.com']);
    expect(root.querySelector<HTMLInputElement>('[aria-label="Bunker link"]')!.value).toContain('bunker://');
    expect(root.querySelector('app-qr-code svg')).not.toBeNull();
    expect(text(root)).toContain('The link works once, for one hour.');
  });

  it('connects a pasted nostrconnect link for the chosen identity', async () => {
    const { fixture, root, navigate } = await render();
    button(root, 'Paste nostrconnect link').click();
    await settle(fixture);
    setTextarea(root, 'textarea[name="nostrconnect"]', 'nostrconnect://abc?relay=wss://r&secret=x');
    await settle(fixture);
    button(root, 'Connect').click();
    await settle(fixture);
    expect(core.bunkerConnect).toHaveBeenCalledWith('nostrconnect://abc?relay=wss://r&secret=x', 'alice');
    expect(navigate).toHaveBeenCalledWith('/connections', { state: { notice: 'The app is connected.' } });
  });

  it('goes back to Connections once an app uses the bunker link', async () => {
    const { fixture, root, navigate } = await render();
    button(root, 'Show bunker link').click();
    await settle(fixture);
    expect(navigate).not.toHaveBeenCalled();
    core.bunkerState.set({ ...core.bunker, connections: [{ ...CONNECTION, identityId: 'alice' }] });
    await settle(fixture);
    expect(navigate).toHaveBeenCalledWith('/connections', { state: { notice: 'The app is connected.' } });
  });

  it('fills in a clicked nostrconnect link and still waits for Connect', async () => {
    core.connectLink.set('nostrconnect://abc?relay=wss%3A%2F%2Fr.example&secret=x');
    const { fixture, root } = await render();
    expect(core.connectLink()).toBeNull();
    expect(root.querySelector<HTMLTextAreaElement>('textarea[name="nostrconnect"]')!.value).toBe(
      'nostrconnect://abc?relay=wss%3A%2F%2Fr.example&secret=x',
    );
    expect(text(root)).toContain('An app sent this link. Choose the identity above, then press Connect.');
    expect(core.bunkerConnect).not.toHaveBeenCalled();
    button(root, 'Connect').click();
    await settle(fixture);
    expect(core.bunkerConnect).toHaveBeenCalledWith('nostrconnect://abc?relay=wss%3A%2F%2Fr.example&secret=x', 'alice');
  });

  it('cancels back to Connections', async () => {
    const { root, navigate } = await render();
    button(root, 'Cancel').click();
    expect(navigate).toHaveBeenCalledWith('/connections', {});
  });

  it('makes the link on relays chosen for it, starting from the defaults', async () => {
    const { fixture, root } = await render();
    const box = root.querySelector<HTMLTextAreaElement>('textarea[name="linkRelays"]')!;
    expect(box.value).toBe('wss://relay.getgooti.com');
    setTextarea(root, 'textarea[name="linkRelays"]', 'wss://a.example\nwss://b.example');
    await settle(fixture);
    button(root, 'Show bunker link').click();
    await settle(fixture);
    expect(core.bunkerLink).toHaveBeenCalledWith('alice', ['wss://a.example', 'wss://b.example']);
    expect(core.bunkerSetRelays).not.toHaveBeenCalled();
  });

  it('cannot connect identities without a private key here', async () => {
    core.identities.set([{ ...ALICE, hasPrivateKey: false, isBunker: true }]);
    const { root } = await render();
    expect(text(root)).toContain('No identity to connect');
  });
});

describe('ApprovalPanel', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    core.identities.set([ALICE]);
    core.bunker.connections = [CONNECTION];
    TestBed.configureTestingModule({ providers: [provideFakeCore(core)] });
  });

  async function render() {
    const fixture = TestBed.createComponent(ApprovalPanel);
    await settle(fixture);
    return { fixture, root: fixture.nativeElement as HTMLElement };
  }

  it('shows the oldest request and approves it, remembered as chosen', async () => {
    core.pendingAsks.set([ASK, { ...ASK, key: 'k2' }]);
    const { fixture, root } = await render();
    expect(text(root)).toContain('Sign this event?');
    expect(root.querySelector('.who .app')?.textContent).toBe('Coracle');
    expect(text(root.querySelector<HTMLElement>('.who')!)).toContain('wants to use Alice');
    expect(text(root)).toContain('Note (kind 1)');
    expect(text(root)).toContain('Hello Nostr');
    expect(text(root)).toContain('t · gooti');
    expect(text(root)).toContain('1 more waiting.');
    expect(text(root)).not.toContain('in the future');

    await choose(fixture, root, 'remember', 'Always');
    button(root, 'Approve').click();
    await settle(fixture);
    expect(core.bunkerDecide).toHaveBeenCalledWith('k1', true, 'always');

    // The next request starts at "Only this time" again.
    expect(selectLabel(root, 'remember')).toBe('Only this time');
    button(root, 'Reject').click();
    await settle(fixture);
    expect(core.bunkerDecide).toHaveBeenLastCalledWith('k2', false, 'once');
    expect(text(root)).toContain('Nothing waits');
  });

  it('approves with Ctrl+Enter and rejects with Ctrl+Shift+Enter', async () => {
    core.pendingAsks.set([ASK, { ...ASK, key: 'k2' }]);
    const { fixture } = await render();
    const enter = (shiftKey: boolean) =>
      document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', ctrlKey: true, shiftKey, bubbles: true }));
    document.dispatchEvent(new KeyboardEvent('keydown', { key: 'Enter', bubbles: true }));
    await settle(fixture);
    expect(core.bunkerDecide).not.toHaveBeenCalled();
    enter(false);
    await settle(fixture);
    expect(core.bunkerDecide).toHaveBeenCalledWith('k1', true, 'once');
    enter(true);
    await settle(fixture);
    expect(core.bunkerDecide).toHaveBeenLastCalledWith('k2', false, 'once');
  });

  it('remembers sensitive kinds for an hour at most', async () => {
    core.pendingAsks.set([{ ...ASK, details: { ...ASK.details, kind: 0, content: '{"name":"x"}' } }]);
    const { fixture, root } = await render();
    expect(text(root)).toContain('Profile (kind 0)');
    expect(text(root)).toContain('an hour at most');
    const options = await openSelect(fixture, root, 'remember');
    const disabled = options.filter((o) => o.getAttribute('aria-disabled') === 'true');
    expect(disabled.map((o) => o.textContent?.trim())).toEqual(['For 1 day', 'Always']);

    // A disabled option cannot be chosen.
    disabled[1].click();
    await settle(fixture);
    expect(selectLabel(root, 'remember')).toBe('Only this time');
  });

  it('warns about an event dated in the future', async () => {
    const later = Math.floor(Date.now() / 1000) + 3600;
    core.pendingAsks.set([{ ...ASK, details: { ...ASK.details, createdAt: later } }]);
    const { root } = await render();
    expect(text(root)).toContain('in the future');
  });

  it('names the site when Gooti browsers send one', async () => {
    core.pendingAsks.set([{ ...ASK, details: { ...ASK.details, site: 'primal.net' } }]);
    const { root } = await render();
    expect(root.querySelector('.who .app')?.textContent?.trim()).toBe('primal.net');
    expect(text(root)).toContain('through');
  });

  it('names a decryption and the other party', async () => {
    core.pendingAsks.set([
      { ...ASK, method: 'nip44_decrypt', details: { kind: null, content: null, tags: [], createdAt: null, peer: 'ab'.repeat(32) } },
    ]);
    const { root } = await render();
    expect(text(root)).toContain('Decrypt a message?');
    expect(text(root)).toContain('ab'.repeat(32));
  });
});

describe('ConnectionPage', () => {
  let core: FakeCore;

  beforeEach(() => {
    core = new FakeCore();
    core.connectionDetail = {
      ...CONNECTION,
      rules: [{ method: 'sign_event', kind: 1, policy: 'allow', createdAt: 0 }],
    };
    TestBed.configureTestingModule({ providers: [provideRouter([]), provideFakeCore(core)] });
  });

  it('changes the trust level and forgets a remembered choice', async () => {
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(text(root)).toContain('Signs as Alice · https://coracle.social');
    expect(text(root)).toContain('Sign: Note (kind 1)');
    expect(text(root)).toContain('Approve · Always');

    root.querySelector<HTMLInputElement>('input[value="basic"]')!.click();
    await settle(fixture);
    expect(core.bunkerSetTrust).toHaveBeenCalledWith('c1', 'basic');

    button(root, 'Forget').click();
    await settle(fixture);
    expect(core.bunkerForget).toHaveBeenCalledWith('c1', 'sign_event', 1);
    expect(text(root)).toContain('Nothing yet.');
  });

  it('signs here instead of another Gooti Desktop', async () => {
    core.connectionDetail = { ...CONNECTION, servedElsewhere: true, rules: [] };
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(text(root)).toContain('Another Gooti Desktop answers this app.');

    button(root, 'Sign here instead').click();
    await settle(fixture);
    expect(core.serveHere).toHaveBeenCalledWith(['alice']);
    expect(text(root)).not.toContain('Another Gooti Desktop answers this app.');
    expect(Array.from(root.querySelectorAll('button')).some((b) => b.textContent?.trim() === 'Sign here instead')).toBe(
      false,
    );
  });

    it('lets Gooti browsers change their relays', async () => {
    core.connectionDetail = { ...CONNECTION, name: 'Gooti browsers', browsers: true, rules: [] };
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector<HTMLTextAreaElement>('textarea[name="appRelays"]')!.value).toBe('wss://relay.getgooti.com');
    setTextarea(root, 'textarea[name="appRelays"]', 'wss://mine.example');
    await settle(fixture);
    button(root, 'Save relays').click();
    await settle(fixture);
    expect(core.bunkerSetAppRelays).toHaveBeenCalledWith('c1', ['wss://mine.example']);
    expect(text(root)).toContain('Updated the relays');
  });

  it('shows the app\'s recent activity with a link to all of it', async () => {
    core.connectionDetail = { ...CONNECTION, rules: [] };
    core.activityLog = [
      {
        at: 2,
        type: 'request',
        app: 'Coracle',
        connectionId: 'c1',
        identityPubkey: ALICE.pubkey,
        method: 'sign_event',
        kind: 1,
        approved: true,
        reason: 'user',
      },
      { at: 1, type: 'connected', app: 'Other', connectionId: 'c9', identityPubkey: ALICE.pubkey },
    ];
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    const rows = Array.from(root.querySelectorAll<HTMLElement>('app-activity-list .entry'));
    expect(rows).toHaveLength(1);
    expect(text(rows[0])).toContain('Coracle: Sign: Note (kind 1)');
    expect(root.querySelector('a.show-all')?.getAttribute('href')).toBe('/activity?app=c1');
  });

  it('shows the relays the app uses', async () => {
    core.connectionDetail = { ...CONNECTION, relays: ['wss://app.relay'], rules: [] };
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(text(root)).toContain('This app reaches the desktop through these relays.');
    expect(root.querySelector('.relay-list')?.textContent?.trim()).toBe('wss://app.relay');
  });

  it('warns that some sites give up quickly when Gooti browsers ask every time', async () => {
    core.connectionDetail = { ...CONNECTION, browsers: true, trust: 'ask', rules: [] };
    const fixture = TestBed.createComponent(ConnectionPage);
    fixture.componentRef.setInput('id', 'c1');
    await settle(fixture);
    const root = fixture.nativeElement as HTMLElement;
    expect(root.querySelector('.slow-sites')?.textContent).toContain('give up after about 8 seconds');

    core.connectionDetail = { ...CONNECTION, browsers: false, trust: 'ask', rules: [] };
    const other = TestBed.createComponent(ConnectionPage);
    other.componentRef.setInput('id', 'c1');
    await settle(other);
    expect((other.nativeElement as HTMLElement).querySelector('.slow-sites')).toBeNull();
  });
});
