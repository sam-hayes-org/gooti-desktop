import { Injectable, signal } from '@angular/core';
import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';

/** The Info page's links. The shell maps each to a fixed address (`commands::open_link`). */
export type InfoLink = 'website' | 'source' | 'author';

/** Mirrors `gooti_core::AppInfo`. */
export interface AppInfo {
  version: string;
  channel: string;
}

/** Mirrors `gooti_core::VaultStatus`. */
export type VaultStatus = 'noVault' | 'locked' | 'unlocked';

/** Mirrors `gooti_core::IdentitySummary`. Never holds a private key. */
export interface IdentitySummary {
  id: string;
  nick: string;
  pubkey: string;
  npub: string;
  color: string | null;
  createdAt: string;
  hasPrivateKey: boolean;
  /** From a browser that links it to a bunker. This desktop cannot sign with it. */
  isBunker: boolean;
  /** A Gooti Desktop pointer whose key no desktop record holds: the key is lost, unless saved. */
  keyMissing: boolean;
  /** This desktop holds the key, but another Gooti Desktop answers apps and browsers for it. */
  servedElsewhere: boolean;
  /** `desktop`: browsers never get the key; they sign through a Gooti Desktop. */
  custody: Custody;
}

export type Custody = 'shared' | 'desktop';

/** Mirrors `gooti_core::Settings`. */
export interface Settings {
  unlockAtLogin: boolean;
  autoLockMinutes: number | null;
  lockOnScreenLock: boolean;
  lockOnSleep: boolean;
  /** Closing the window keeps Gooti in the tray. */
  keepRunning: boolean;
  /** Look for a new version when Gooti starts. */
  checkUpdates: boolean;
}

export interface AppSettingsChange {
  keepRunning?: boolean;
  checkUpdates?: boolean;
}

export interface AutostartInfo {
  enabled: boolean;
  /** Without a tray, closing the window quits and Gooti cannot start hidden. */
  hasTray: boolean;
}

/** Mirrors `src-tauri/src/updates.rs::UpdateInfo`. */
export interface UpdateInfo {
  version: string;
  currentVersion: string;
  notes: string | null;
  /** Gooti installs it itself (AppImage, Windows); otherwise the package manager does. */
  canInstall: boolean;
  releasesUrl: string;
}

export interface LockSettingsChange {
  autoLockMinutes?: number | null;
  lockOnScreenLock?: boolean;
  lockOnSleep?: boolean;
}

/** Mirrors `gooti_core::BackupInfo`. Absent (null) while backup is off. */
export interface BackupInfo {
  relays: string[];
  autoBackup: boolean;
  /** Characters the sealed vault takes on a relay, out of 1,000,000. */
  contentChars: number;
  /** The relays hold everything only this desktop holds: a reset loses nothing a restore cannot bring back. */
  desktopCurrent: boolean;
}

/** Mirrors `gooti_core::backup::flow::BackupOutcome`. */
export interface BackupOutcome {
  result: 'backedUp' | 'updated' | 'current';
  took: string[];
  sent: string[];
}

/** An automatic backup finished: its outcome, or the reason it failed. */
export type BackupActivity = (BackupOutcome & { at: number }) | { error: string; at: number };

/** Mirrors `gooti_core::bunker::store::ConnectionInfo`. */
export interface BunkerConnection {
  id: string;
  identityId: string;
  identityNick: string;
  clientPubkey: string;
  name: string | null;
  url: string | null;
  image: string | null;
  perms: string | null;
  createdAt: number;
  lastUsedAt: number | null;
  trust: Trust;
  /** The browsers reaching a Desktop only identity. */
  browsers: boolean;
  /** Where this app reaches the desktop. */
  relays: string[];
  /** Another Gooti Desktop answers this app (it answers for the identity), not this one. */
  servedElsewhere: boolean;
}

export interface RelayStatus {
  url: string;
  state: 'connecting' | 'connected' | 'failed';
  reason?: string;
}

export type Trust = 'ask' | 'basic' | 'trust';
export type Remember = 'once' | 'fiveMinutes' | 'oneHour' | 'oneDay' | 'always';
export type Reason = 'remembered' | 'trustLevel' | 'user' | 'timedOut';

/** Mirrors `gooti_core::bunker::policy::Rule`: a choice remembered for one app. */
export interface Rule {
  method: string;
  kind?: number;
  policy: 'allow' | 'deny';
  createdAt: number;
  expiresAt?: number;
}

export interface ConnectionDetail extends BunkerConnection {
  trust: Trust;
  rules: Rule[];
}

/** Mirrors `gooti_core::activity::ActivityEntry`. */
export type ActivityEntry = { at: number } & (
  | {
      type: 'request';
      app: string;
      /** The app's connection; empty in entries written before 2026-11-13. */
      connectionId: string;
      identityPubkey: string;
      method: string;
      kind?: number;
      approved: boolean;
      reason: Reason;
      error?: string;
      /** The site, for a request from Gooti browsers that names it. */
      site?: string;
    }
  | { type: 'connected' | 'disconnected'; app: string; connectionId: string; identityPubkey: string }
  | {
      type: 'backup';
      automatic: boolean;
      result: 'backedUp' | 'updated' | 'current' | 'failed';
      took: string[];
      sent: string[];
      error?: string;
    }
);

/** Kinds that ask unless the app is trusted. Same list as `gooti_core::bunker::policy`. */
export const SENSITIVE_KINDS = [0, 3, 5, 62, 10000, 10002, 10050, 22242, 24242, 27235, 13194, 23194, 23195];

export interface BunkerInfo {
  /** The default relays for apps (Settings). */
  relays: string[];
  /** The relays of bunker links still waiting for an app. */
  linkRelays: string[];
  paused: boolean;
  connections: BunkerConnection[];
  relayStatus: RelayStatus[];
}

/** Mirrors `gooti_core::bunker::handler::Ask`: a request waiting for Approve or Reject. */
export interface BunkerAsk {
  key: string;
  requestId: string;
  connectionId: string;
  identityPubkey: string;
  clientPubkey: string;
  method: 'sign_event' | 'nip04_encrypt' | 'nip04_decrypt' | 'nip44_encrypt' | 'nip44_decrypt';
  params: string[];
  receivedAt: number;
  details: {
    kind: number | null;
    content: string | null;
    tags: string[][];
    createdAt: number | null;
    peer: string | null;
    /** The site, for a request from Gooti browsers that names it. */
    site?: string;
  };
}

export interface RelayFailure {
  url: string;
  reason: string;
}

/** What a failed command returns. Show `message`; branch on `code`. */
export interface CoreError {
  code: string;
  message: string;
  /** Each relay's own reason, when every relay failed. */
  failures?: RelayFailure[];
}

export function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return typeof error === 'string' ? error : 'Something went wrong. Try again.';
}

export function errorCode(error: unknown): string | undefined {
  if (typeof error === 'object' && error !== null && 'code' in error) {
    return String((error as { code: unknown }).code);
  }
  return undefined;
}

export function relayFailures(error: unknown): RelayFailure[] {
  if (typeof error === 'object' && error !== null && 'failures' in error) {
    const failures = (error as { failures: unknown }).failures;
    return Array.isArray(failures) ? (failures as RelayFailure[]) : [];
  }
  return [];
}

export const VAULT_STATUS_EVENT = 'vault-status';
export const VAULT_CHANGED_EVENT = 'vault-changed';
export const BACKUP_ACTIVITY_EVENT = 'backup-activity';
/** A backup started or every backup ended. Payload: whether any runs. */
export const BACKUP_BUSY_EVENT = 'backup-busy';
/** A `nostrconnect://` link was clicked. Take it with `connect_link_take`. */
export const CONNECT_LINK_EVENT = 'connect-link';
export const BUNKER_PENDING_EVENT = 'bunker-pending';
export const BUNKER_CONNECTIONS_EVENT = 'bunker-connections';
export const ACTIVITY_EVENT = 'activity-changed';
/** A bunker relay connected, was lost, was added, or was dropped. */
export const BUNKER_RELAYS_EVENT = 'bunker-relays';
export const UPDATE_EVENT = 'update-available';
/** The label of the small window for bunker requests (`src-tauri/src/approval.rs`). */
export const APPROVAL_WINDOW = 'approval';

/**
 * The only way the UI reaches `gooti-core`. Outside Tauri (plain `ng serve`) nothing is called
 * and the app shows the first vault screen.
 */
@Injectable({ providedIn: 'root' })
export class CoreService {
  readonly appInfo = signal<AppInfo | null>(null);
  readonly status = signal<VaultStatus | null>(null);
  readonly identities = signal<IdentitySummary[]>([]);
  /** Counts changes made outside the UI (such as an automatic backup), so pages can reload. */
  readonly changes = signal(0);
  /** Bunker requests waiting for Approve or Reject, oldest first. */
  readonly pendingAsks = signal<BunkerAsk[]>([]);
  /** Counts bunker connection changes, so the Connections page can reload. */
  readonly bunkerChanges = signal(0);
  /**
   * The bunker's apps and relays, kept current while unlocked: it reloads when an app connects or
   * is removed, a relay connects or is lost, and the vault changes (such as a backup).
   */
  readonly bunkerState = signal<BunkerInfo | null>(null);
  #bunkerReads = 0;
  /** Counts new Activity entries, so the Activity page can reload. */
  readonly activityChanges = signal(0);
  /** A newer version, found at start or by "Check now". */
  readonly update = signal<UpdateInfo | null>(null);
  /** True in the small approval window, which shows only the request panel. */
  readonly isApprovalWindow = signal(false);
  /** The last automatic backup on this desktop, since the app started. */
  readonly lastAutoBackup = signal<BackupActivity | null>(null);
  /** A clicked `nostrconnect://` link for "Connect new app", until that page takes it. */
  readonly connectLink = signal<string | null>(null);
  /** A backup runs: "Back up now", a restore, or an automatic one. Shows the bar at the top. */
  readonly backingUp = signal(false);

  async init(): Promise<void> {
    if (!isTauri()) {
      this.status.set('noVault');
      return;
    }
    this.isApprovalWindow.set(getCurrentWindow().label === APPROVAL_WINDOW);
    await listen<VaultStatus>(VAULT_STATUS_EVENT, (event) => this.#setStatus(event.payload));
    await listen(CONNECT_LINK_EVENT, () => void this.takeConnectLink());
    await listen(VAULT_CHANGED_EVENT, () => {
      this.changes.update((n) => n + 1);
      void this.refreshIdentities().catch(() => undefined);
    });
    await listen(BUNKER_PENDING_EVENT, () => void this.refreshPending());
    await listen(BUNKER_CONNECTIONS_EVENT, () => this.#bunkerChanged());
    await listen(BUNKER_RELAYS_EVENT, () => void this.refreshBunker());
    await listen(ACTIVITY_EVENT, () => this.activityChanges.update((n) => n + 1));
    await listen<UpdateInfo>(UPDATE_EVENT, (event) => this.update.set(event.payload));
    await listen<boolean>(BACKUP_BUSY_EVENT, (event) => this.backingUp.set(event.payload));
    this.backingUp.set(await this.#call<boolean>('backup_busy'));
    await listen<BackupOutcome | { error: string }>(BACKUP_ACTIVITY_EVENT, (event) => {
      this.lastAutoBackup.set({ ...event.payload, at: Date.now() });
    });
    this.appInfo.set(await this.#call<AppInfo>('app_info'));
    this.#setStatus(await this.#call<VaultStatus>('vault_status'));
  }

  // Vault

  async createVault(password: string): Promise<void> {
    await this.#call('vault_create', { password });
    this.#setStatus('unlocked');
  }

  async unlock(password: string): Promise<void> {
    await this.#call('vault_unlock', { password });
    this.#setStatus('unlocked');
  }

  async lock(): Promise<void> {
    await this.#call('vault_lock');
    this.#setStatus('locked');
  }

  async reset(): Promise<void> {
    await this.#call('vault_reset');
    this.#setStatus('noVault');
  }

  changePassword(current: string, next: string): Promise<void> {
    return this.#call('vault_change_password', { current, next });
  }

  touch(): void {
    if (isTauri()) {
      void this.#call('activity_touch').catch(() => undefined);
    }
  }

  // Identities

  /** Also reloads the bunker: identities bring and drop its apps (such as Gooti browsers). */
  async refreshIdentities(): Promise<IdentitySummary[]> {
    const identities = await this.#call<IdentitySummary[]>('identities_list');
    this.identities.set(identities);
    void this.refreshBunker();
    return identities;
  }

  identity(id: string): Promise<IdentitySummary> {
    return this.#call('identity_get', { id });
  }

  /** Without `key`, the core generates the private key and never shows it. */
  async createIdentity(
    nick: string,
    key?: string,
    keyPassword?: string,
    custody: Custody = 'shared',
    relays?: string[],
  ): Promise<IdentitySummary> {
    const created = await this.#call<IdentitySummary>('identity_create', {
      nick,
      key: key ?? null,
      keyPassword: keyPassword || null,
      custody,
      relays: relays ?? null,
    });
    await this.refreshIdentities();
    return created;
  }

  async renameIdentity(id: string, nick: string): Promise<IdentitySummary> {
    const updated = await this.#call<IdentitySummary>('identity_rename', { id, nick });
    await this.refreshIdentities();
    return updated;
  }

  async setIdentityColor(id: string, color: string | null): Promise<IdentitySummary> {
    const updated = await this.#call<IdentitySummary>('identity_set_color', { id, color });
    await this.refreshIdentities();
    return updated;
  }

  /** "Move key to desktop" (browsers reach it on `relays`) or "Use in browsers again". */
  async setCustody(id: string, custody: Custody, relays?: string[]): Promise<IdentitySummary> {
    const updated = await this.#call<IdentitySummary>('identity_set_custody', {
      id,
      custody,
      relays: relays ?? null,
    });
    await this.refreshIdentities();
    return updated;
  }

  /** "Sign here instead": this desktop answers for these identities from now on. */
  async serveHere(ids: string[]): Promise<void> {
    await this.#call('identity_serve_here', { ids });
    await this.refreshIdentities();
  }

  async deleteIdentity(id: string): Promise<void> {
    await this.#call('identity_delete', { id });
    await this.refreshIdentities();
  }

  revealNsec(id: string): Promise<string> {
    return this.#call('identity_reveal_nsec', { id });
  }

  exportNcryptsec(id: string, password: string): Promise<string> {
    return this.#call('identity_export_ncryptsec', { id, password });
  }

  /** The first screen's "Restore from backup". Nothing is stored unless the relays have a vault. */
  async restoreVault(password: string, phrase: string, relays: string[]): Promise<void> {
    await this.#call('vault_restore', { password, phrase, relays });
    this.#setStatus('unlocked');
  }

  // Backup

  backupInfo(): Promise<BackupInfo | null> {
    return this.#call('backup_info');
  }

  defaultRelays(): Promise<string[]> {
    return this.#call('backup_default_relays');
  }

  generatePassphrase(): Promise<string> {
    return this.#call('backup_generate_passphrase');
  }

  checkPassphrase(phrase: string): Promise<void> {
    return this.#call('backup_check_passphrase', { phrase });
  }

  enableBackup(phrase: string, relays: string[]): Promise<BackupInfo | null> {
    return this.#call('backup_enable', { phrase, relays });
  }

  /** "Use an existing backup": merges this vault with the one on the relays. */
  async joinBackup(phrase: string, relays: string[]): Promise<BackupInfo | null> {
    const info = await this.#call<BackupInfo | null>('backup_join', { phrase, relays });
    await this.refreshIdentities();
    return info;
  }

  async backUpNow(): Promise<BackupOutcome> {
    const outcome = await this.#call<BackupOutcome>('backup_now');
    await this.refreshIdentities();
    return outcome;
  }

  setBackupRelays(relays: string[]): Promise<BackupInfo | null> {
    return this.#call('backup_set_relays', { relays });
  }

  setAutoBackup(on: boolean): Promise<BackupInfo | null> {
    return this.#call('backup_set_auto', { on });
  }

  disableBackup(): Promise<void> {
    return this.#call('backup_disable');
  }

  // Bunker

  bunkerInfo(): Promise<BunkerInfo> {
    return this.#call('bunker_info');
  }

  /** Reads the bunker's apps and relays into `bunkerState`. A later read always wins. */
  async refreshBunker(): Promise<void> {
    const read = ++this.#bunkerReads;
    const info = this.status() === 'unlocked' ? await this.bunkerInfo().catch(() => null) : null;
    if (read === this.#bunkerReads) {
      this.bunkerState.set(info);
    }
  }

  #bunkerChanged(): void {
    this.bunkerChanges.update((n) => n + 1);
    void this.refreshBunker();
  }

  async refreshPending(): Promise<void> {
    this.pendingAsks.set(await this.#call<BunkerAsk[]>('bunker_pending').catch(() => []));
  }

  /** A new single-use `bunker://` link, valid for an hour, on these relays (default: the default relays). */
  async bunkerLink(identityId: string, relays?: string[]): Promise<string> {
    const link = await this.#call<string>('bunker_link', { identityId, relays: relays ?? null });
    await this.refreshBunker();
    return link;
  }

  /** The relays Gooti browsers reach a Desktop only identity on. */
  async bunkerSetAppRelays(connectionId: string, relays: string[]): Promise<string[]> {
    const saved = await this.#call<string[]>('bunker_set_app_relays', { connectionId, relays });
    await this.refreshBunker();
    return saved;
  }

  /** Connects the app behind a pasted `nostrconnect://` link. */
  async bunkerConnect(link: string, identityId: string): Promise<void> {
    await this.#call('bunker_connect', { link, identityId });
    this.#bunkerChanged();
  }

  async bunkerDecide(key: string, approve: boolean, remember: Remember = 'once'): Promise<void> {
    await this.#call('bunker_decide', { key, approve, remember });
    await this.refreshPending();
  }

  bunkerConnection(connectionId: string): Promise<ConnectionDetail> {
    return this.#call('bunker_connection', { connectionId });
  }

  bunkerSetTrust(connectionId: string, trust: Trust): Promise<ConnectionDetail> {
    return this.#call('bunker_set_trust', { connectionId, trust });
  }

  /** Removes a remembered choice. */
  bunkerForget(connectionId: string, method: string, kind: number | null): Promise<ConnectionDetail> {
    return this.#call('bunker_forget', { connectionId, method, kind });
  }

  // Activity

  /** Newest first. */
  activity(): Promise<ActivityEntry[]> {
    return this.#call('activity_list');
  }

  clearActivity(): Promise<void> {
    return this.#call('activity_clear');
  }

  async bunkerRevoke(connectionId: string): Promise<void> {
    await this.#call('bunker_revoke', { connectionId });
    this.#bunkerChanged();
  }

  async bunkerSetPaused(paused: boolean): Promise<void> {
    await this.#call('bunker_set_paused', { paused });
    await this.refreshBunker();
  }

  /** The default relays for apps. Changes nothing in use. */
  async bunkerSetRelays(relays: string[]): Promise<string[]> {
    const saved = await this.#call<string[]>('bunker_set_relays', { relays });
    await this.refreshBunker();
    return saved;
  }

  // Settings

  settings(): Promise<Settings> {
    return this.#call('settings_get');
  }

  setUnlockAtLogin(on: boolean): Promise<Settings> {
    return this.#call('settings_set_unlock_at_login', { on });
  }

  changeLockSettings(change: LockSettingsChange): Promise<Settings> {
    return this.#call('settings_change_lock', { change });
  }

  changeAppSettings(change: AppSettingsChange): Promise<Settings> {
    return this.#call('settings_change_app', { change });
  }

  autostart(): Promise<AutostartInfo> {
    return this.#call('autostart_get');
  }

  setAutostart(on: boolean): Promise<AutostartInfo> {
    return this.#call('autostart_set', { on });
  }

  /** `null` when this is the newest version. */
  async checkForUpdate(): Promise<UpdateInfo | null> {
    const found = await this.#call<UpdateInfo | null>('update_check');
    this.update.set(found);
    return found;
  }

  openReleases(): Promise<void> {
    return this.#call('open_releases');
  }

  /** Gooti's license and those of everything it ships with, as one text. */
  licensesText(): Promise<string> {
    return this.#call('licenses_text');
  }

  /** Opens one of the Info page's fixed links in the browser. */
  openLink(link: InfoLink): Promise<void> {
    return this.#call('open_link', { link });
  }

  /** Installs the update and restarts (AppImage and Windows only). */
  installUpdate(): Promise<void> {
    return this.#call('update_install');
  }

  /** Takes a clicked link in the main window once Gooti is unlocked; it waits in the shell until then. */
  async takeConnectLink(): Promise<void> {
    if (this.isApprovalWindow() || this.status() !== 'unlocked') {
      return;
    }
    const link = await this.#call<string | null>('connect_link_take').catch(() => null);
    if (link) {
      this.connectLink.set(link);
    }
  }

  #setStatus(status: VaultStatus): void {
    this.status.set(status);
    if (status === 'unlocked') {
      void this.refreshIdentities().catch(() => undefined);
      void this.refreshPending();
      void this.takeConnectLink();
    } else {
      this.identities.set([]);
      this.pendingAsks.set([]);
      void this.refreshBunker();
    }
  }

  #call<T = void>(command: string, args?: Record<string, unknown>): Promise<T> {
    return invoke<T>(command, args);
  }
}
