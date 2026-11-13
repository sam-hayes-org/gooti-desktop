import { Component, computed, effect, inject, OnInit, signal, untracked } from '@angular/core';
import { Router, RouterLink, RouterLinkActive, RouterOutlet } from '@angular/router';
import { CoreService, errorMessage } from './core/core.service';
import { MOD, withMod } from './shared/shortcuts';
import { NAV_ITEMS } from './nav';
import { ApprovalPanel } from './connections/approval-panel';
import { ConfirmDialog } from './shared/confirm-dialog';
import { ConfirmService } from './shared/confirm.service';
import { TextMenu } from './shared/text-menu';
import { Tooltip } from './shared/tooltip';
import { VaultStart } from './vault/vault-start';
import { VaultUnlock } from './vault/vault-unlock';

/** The relay status next to Connections. */
interface RelaySummary {
  label: string;
  tone: 'up' | 'partial' | 'down' | 'paused';
  hint: string;
}

/** "Lock after" counts from the last input in the window. Report it at most this often. */
const TOUCH_INTERVAL_MS = 20_000;
/** A fast backup still shows its bar this long, so it is seen at all. */
export const BACKUP_BAR_MIN_MS = 800;

@Component({
  imports: [
    RouterOutlet,
    RouterLink,
    RouterLinkActive,
    ApprovalPanel,
    ConfirmDialog,
    TextMenu,
    Tooltip,
    VaultStart,
    VaultUnlock,
  ],
  selector: 'app-root',
  styleUrl: './app.scss',
  templateUrl: './app.html',
  host: {
    '(document:pointerdown)': 'onInput()',
    '(document:keydown)': 'onKey($event)',
  },
})
export class App implements OnInit {
  protected readonly navItems = NAV_ITEMS;
  protected readonly lockHint = `Asks for the Gooti password again. Apps can't sign while Gooti is locked. (${MOD}+L)`;
  protected readonly core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);
  readonly #router = inject(Router);
  #lastTouch = 0;
  /** `core.backingUp`, held on for at least BACKUP_BAR_MIN_MS. */
  protected readonly backupBar = signal(false);
  #backupBarSince = 0;
  #backupBarTimer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    effect(() => {
      const busy = this.core.backingUp();
      untracked(() => this.#showBackupBar(busy));
    });
    effect(() => {
      // A clicked nostrconnect:// link: "Connect new app" takes it from there.
      if (this.core.connectLink() && this.core.status() === 'unlocked' && !this.core.isApprovalWindow()) {
        untracked(() => {
          if (this.#router.url !== '/connections/new') {
            void this.#router.navigateByUrl('/connections/new');
          }
        });
      }
    });
  }

  #showBackupBar(busy: boolean): void {
    clearTimeout(this.#backupBarTimer);
    if (busy) {
      if (!this.backupBar()) {
        this.#backupBarSince = Date.now();
        this.backupBar.set(true);
      }
      return;
    }
    const left = this.#backupBarSince + BACKUP_BAR_MIN_MS - Date.now();
    if (left <= 0) {
      this.backupBar.set(false);
    } else {
      this.#backupBarTimer = setTimeout(() => this.backupBar.set(false), left);
    }
  }
  /** How many apps sign through this desktop. Null until the bunker was read. */
  protected readonly connectionCount = computed(() => this.core.bunkerState()?.connections.length ?? null);

  /** The bunker's relays for the sidebar: how many are connected, and a color for it. */
  protected readonly relays = computed<RelaySummary | null>(() => {
    const bunker = this.core.bunkerState();
    if (!bunker) {
      return null;
    }
    if (bunker.paused) {
      return { label: 'Paused', tone: 'paused', hint: 'Paused: apps cannot reach this desktop.' };
    }
    const total = bunker.relayStatus.length;
    if (total === 0) {
      return null;
    }
    const up = bunker.relayStatus.filter((relay) => relay.state === 'connected').length;
    const tone = up === total ? 'up' : up === 0 ? 'down' : 'partial';
    const hint = `${up} of ${total} ${total === 1 ? 'relay' : 'relays'} connected`;
    return { label: `${up}/${total}`, tone, hint };
  });

  protected readonly installing = signal(false);
  protected readonly updateError = signal('');

  async installUpdate(): Promise<void> {
    this.installing.set(true);
    this.updateError.set('');
    try {
      // Gooti restarts when this works.
      await this.core.installUpdate();
    } catch (error) {
      this.updateError.set(errorMessage(error));
      this.installing.set(false);
    }
  }

  async ngOnInit(): Promise<void> {
    await this.core.init();
  }

  /** Ctrl+1 … Ctrl+6 open the pages, Ctrl+L locks. Only in the main window, unlocked, with no question open. */
  onKey(event: KeyboardEvent): void {
    this.onInput();
    if (
      !withMod(event) ||
      this.core.isApprovalWindow() ||
      this.core.status() !== 'unlocked' ||
      this.#confirm.pending()
    ) {
      return;
    }
    const page = /^Digit([1-9])$/.exec(event.code);
    if (page && !event.shiftKey) {
      const item = this.navItems[Number(page[1]) - 1];
      if (item) {
        event.preventDefault();
        void this.#router.navigateByUrl('/' + item.path);
      }
    } else if (event.code === 'KeyL' && !event.shiftKey) {
      event.preventDefault();
      void this.lock();
    }
  }

  onInput(): void {
    const now = Date.now();
    if (this.core.status() === 'unlocked' && now - this.#lastTouch > TOUCH_INTERVAL_MS) {
      this.#lastTouch = now;
      this.core.touch();
    }
  }

  async lock(): Promise<void> {
    if (await this.#confirm.ask("Lock Gooti? Apps can't sign while Gooti is locked.", { yesLabel: 'Lock' })) {
      await this.core.lock();
      // The next unlock starts on Identities, not on a page that showed a key.
      await this.#router.navigateByUrl('/identities');
    }
  }
}
