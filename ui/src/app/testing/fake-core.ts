import { signal } from '@angular/core';
import { vi } from 'vitest';
import {
  ActivityEntry,
  BunkerAsk,
  BunkerInfo,
  ConnectionDetail,
  CoreService,
  IdentitySummary,
  Settings,
  BackupActivity,
  BackupInfo,
  BackupOutcome,
  UpdateInfo,
  VaultStatus,
} from '../core/core.service';

export const ALICE: IdentitySummary = {
  id: 'alice',
  nick: 'Alice',
  pubkey: '7e7e9c42a91bfef19fa929e5fda1b72e0ebc1a4c1141673e2794234d86addf4e',
  npub: 'npub10elfcs4fr0l0r8af98jlmgdh9c8tcxjvz9qkw038js35mp4dma8qzvjptg',
  color: null,
  createdAt: '2026-11-11T00:00:00.000Z',
  hasPrivateKey: true,
  isBunker: false,
  keyMissing: false,
  servedElsewhere: false,
  custody: 'shared',
};

export const DEFAULT_SETTINGS: Settings = {
  unlockAtLogin: false,
  autoLockMinutes: null,
  lockOnScreenLock: true,
  lockOnSleep: true,
  keepRunning: true,
  checkUpdates: true,
};

export const PHRASE =
  'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about';

/** Stands in for the Rust core in UI tests. */
export class FakeCore {
  readonly appInfo = signal({ version: '0.1.0', channel: 'alpha' });
  readonly status = signal<VaultStatus | null>('unlocked');
  readonly identities = signal<IdentitySummary[]>([]);
  readonly changes = signal(0);
  readonly lastAutoBackup = signal<BackupActivity | null>(null);
  readonly connectLink = signal<string | null>(null);
  readonly backingUp = signal(false);
  readonly pendingAsks = signal<BunkerAsk[]>([]);
  readonly bunkerChanges = signal(0);
  readonly bunkerState = signal<BunkerInfo | null>(null);
  readonly activityChanges = signal(0);
  readonly isApprovalWindow = signal(false);
  readonly update = signal<UpdateInfo | null>(null);
  activityLog: ActivityEntry[] = [];
  bunker: BunkerInfo = {
    relays: ['wss://relay.getgooti.com'],
    linkRelays: [],
    paused: false,
    connections: [],
    relayStatus: [],
  };
  /** What `backupInfo` returns: null while backup is off. */
  backup: BackupInfo | null = null;

  init = vi.fn(async () => undefined);
  touch = vi.fn();
  createVault = vi.fn(async () => this.status.set('unlocked'));
  unlock = vi.fn(async () => this.status.set('unlocked'));
  lock = vi.fn(async () => this.status.set('locked'));
  reset = vi.fn(async () => this.status.set('noVault'));
  changePassword = vi.fn(async () => undefined);
  refreshIdentities = vi.fn(async () => this.identities());
  identity = vi.fn(async (id: string) => {
    const found = this.identities().find((identity) => identity.id === id);
    if (!found) {
      throw { code: 'identity_not_found', message: 'Identity not found.' };
    }
    return found;
  });
  createIdentity = vi.fn(async (nick: string, _key?: string, _password?: string, _custody?: string) => ({
    ...ALICE,
    id: 'new',
    nick,
  }));
  /** Like the real one: the identity list follows. */
  setCustody = vi.fn(async (id: string, custody: 'shared' | 'desktop') => {
    this.identities.update((list) => list.map((i) => (i.id === id ? { ...i, custody } : i)));
    return { ...ALICE, ...this.identities().find((i) => i.id === id), id, custody };
  });
  renameIdentity = vi.fn(async (id: string, nick: string) => ({ ...ALICE, id, nick }));
  setIdentityColor = vi.fn(async (id: string, color: string | null) => ({ ...ALICE, id, color }));
  /** Like the real one: the identity's apps are answered here from now on. */
  serveHere = vi.fn(async (ids: string[]) => {
    this.identities.update((list) => list.map((i) => (ids.includes(i.id) ? { ...i, servedElsewhere: false } : i)));
    const here = <T extends { identityId: string; servedElsewhere: boolean }>(c: T): T =>
      ids.includes(c.identityId) ? { ...c, servedElsewhere: false } : c;
    this.bunker = { ...this.bunker, connections: this.bunker.connections.map(here) };
    this.connectionDetail = here(this.connectionDetail);
  });
  deleteIdentity = vi.fn(async () => undefined);
  revealNsec = vi.fn(async () => 'nsec1test');
  exportNcryptsec = vi.fn(async () => 'ncryptsec1test');
  settings = vi.fn(async () => ({ ...DEFAULT_SETTINGS }));
  setUnlockAtLogin = vi.fn(async (on: boolean) => ({ ...DEFAULT_SETTINGS, unlockAtLogin: on }));
  changeLockSettings = vi.fn(async (change: Partial<Settings>) => ({ ...DEFAULT_SETTINGS, ...change }));

  restoreVault = vi.fn(async () => this.status.set('unlocked'));
  backupInfo = vi.fn(async () => this.backup);
  defaultRelays = vi.fn(async () => ['wss://relay.getgooti.com']);
  generatePassphrase = vi.fn(async () => PHRASE);
  checkPassphrase = vi.fn(async () => undefined);
  enableBackup = vi.fn(async (_phrase: string, relays: string[]) => {
    this.backup = { relays, autoBackup: false, contentChars: 1234, desktopCurrent: true };
    return this.backup;
  });
  joinBackup = vi.fn(async (_phrase: string, relays: string[]) => {
    this.backup = { relays, autoBackup: false, contentChars: 1234, desktopCurrent: true };
    return this.backup;
  });
  backUpNow = vi.fn(
    async (): Promise<BackupOutcome> => ({ result: 'backedUp', took: ['Added identity Bob'], sent: [] }),
  );
  setBackupRelays = vi.fn(async (relays: string[]) => {
    this.backup = this.backup && { ...this.backup, relays };
    return this.backup;
  });
  setAutoBackup = vi.fn(async (on: boolean) => {
    this.backup = this.backup && { ...this.backup, autoBackup: on };
    return this.backup;
  });
  disableBackup = vi.fn(async () => {
    this.backup = null;
  });

  bunkerInfo = vi.fn(async () => this.bunker);
  refreshBunker = vi.fn(async () => this.bunkerState.set(this.status() === 'unlocked' ? this.bunker : null));
  refreshPending = vi.fn(async () => undefined);
  bunkerLink = vi.fn(async () => 'bunker://abc?relay=wss%3A%2F%2Frelay.getgooti.com&secret=s1&gooti=1');
  bunkerConnect = vi.fn(async () => undefined);
  bunkerDecide = vi.fn(async (key: string, _approve?: boolean, _remember?: string) => {
    this.pendingAsks.update((asks) => asks.filter((ask) => ask.key !== key));
  });
  bunkerRevoke = vi.fn(async (id: string) => {
    this.bunker = { ...this.bunker, connections: this.bunker.connections.filter((c) => c.id !== id) };
  });
  bunkerSetPaused = vi.fn(async (paused: boolean) => {
    this.bunker = { ...this.bunker, paused };
  });
  bunkerSetRelays = vi.fn(async (relays: string[]) => relays);
  bunkerSetAppRelays = vi.fn(async (_connectionId: string, relays: string[]) => relays);
  connectionDetail: ConnectionDetail = { ...CONNECTION, rules: [] };
  bunkerConnection = vi.fn(async () => this.connectionDetail);
  bunkerSetTrust = vi.fn(async (_id: string, trust: ConnectionDetail['trust']) => {
    this.connectionDetail = { ...this.connectionDetail, trust };
    return this.connectionDetail;
  });
  bunkerForget = vi.fn(async () => {
    this.connectionDetail = { ...this.connectionDetail, rules: [] };
    return this.connectionDetail;
  });
  autostartInfo = { enabled: false, hasTray: true };
  changeAppSettings = vi.fn(async (change: Partial<Settings>) => ({ ...DEFAULT_SETTINGS, ...change }));
  autostart = vi.fn(async () => this.autostartInfo);
  setAutostart = vi.fn(async (on: boolean) => {
    this.autostartInfo = { ...this.autostartInfo, enabled: on };
    return this.autostartInfo;
  });
  checkForUpdate = vi.fn(async (): Promise<UpdateInfo | null> => null);
  installUpdate = vi.fn(async () => undefined);
  openReleases = vi.fn(async () => undefined);
  openLink = vi.fn(async (_link: string) => undefined);
  activity = vi.fn(async () => this.activityLog);
  clearActivity = vi.fn(async () => {
    this.activityLog = [];
  });
}

export const CONNECTION = {
  id: 'c1',
  identityId: 'alice',
  identityNick: 'Alice',
  clientPubkey: 'eff37350d839ce3707332348af4549a96051bd695d3223af4aabce4993531d86',
  name: 'Coracle',
  url: 'https://coracle.social',
  image: null,
  perms: null,
  createdAt: 0,
  lastUsedAt: null,
  trust: 'ask' as const,
  browsers: false,
  relays: ['wss://relay.getgooti.com'],
  servedElsewhere: false,
};

export const ASK: BunkerAsk = {
  key: 'k1',
  requestId: 'r1',
  connectionId: 'c1',
  identityPubkey: ALICE.pubkey,
  clientPubkey: 'eff37350d839ce3707332348af4549a96051bd695d3223af4aabce4993531d86',
  method: 'sign_event',
  params: [],
  receivedAt: 0,
  details: { kind: 1, content: 'Hello Nostr', tags: [['t', 'gooti']], createdAt: 1_700_000_000, peer: null },
};

export function provideFakeCore(core = new FakeCore()) {
  return { provide: CoreService, useValue: core };
}

/** Lets pending promises and change detection settle. */
export async function settle(fixture: { whenStable(): Promise<unknown>; detectChanges(): void }) {
  for (let i = 0; i < 3; i++) {
    await fixture.whenStable();
    fixture.detectChanges();
  }
}

export function text(root: HTMLElement): string {
  return root.textContent?.replace(/\s+/g, ' ').trim() ?? '';
}

export function setInput(root: HTMLElement, selector: string, value: string): void {
  const input = root.querySelector<HTMLInputElement>(selector);
  if (!input) {
    throw new Error(`No input ${selector}`);
  }
  input.value = value;
  input.dispatchEvent(new Event('input'));
}

export function button(root: HTMLElement, label: string): HTMLButtonElement {
  const found = Array.from(root.querySelectorAll<HTMLButtonElement>('button')).find(
    (b) => b.textContent?.trim() === label,
  );
  if (!found) {
    throw new Error(`No button "${label}"`);
  }
  return found;
}

export function setTextarea(root: HTMLElement, selector: string, value: string): void {
  const area = root.querySelector<HTMLTextAreaElement>(selector);
  if (!area) {
    throw new Error(`No textarea ${selector}`);
  }
  area.value = value;
  area.dispatchEvent(new Event('input'));
}

type Settleable = { whenStable(): Promise<unknown>; detectChanges(): void };

/** The label shown in an `app-select` field. */
export function selectLabel(root: HTMLElement, id: string): string {
  const field = root.querySelector<HTMLButtonElement>(`#${id}`);
  if (!field) {
    throw new Error(`No select #${id}`);
  }
  return field.querySelector('.label')?.textContent?.trim() ?? '';
}

/** Opens an `app-select` and returns its options. They render in the overlay, outside `root`. */
export async function openSelect(fixture: Settleable, root: HTMLElement, id: string): Promise<HTMLElement[]> {
  const field = root.querySelector<HTMLButtonElement>(`#${id}`);
  if (!field) {
    throw new Error(`No select #${id}`);
  }
  field.click();
  await settle(fixture);
  const list = document.getElementById(field.getAttribute('aria-controls') ?? '');
  if (!list) {
    throw new Error(`Select #${id} did not open`);
  }
  return Array.from(list.querySelectorAll<HTMLElement>('[role="option"]'));
}

/** Opens an `app-select` and clicks the option with this label. */
export async function choose(fixture: Settleable, root: HTMLElement, id: string, label: string): Promise<void> {
  const options = await openSelect(fixture, root, id);
  const option = options.find((o) => o.textContent?.trim() === label);
  if (!option) {
    throw new Error(`Select #${id} has no option "${label}"`);
  }
  option.click();
  await settle(fixture);
}
