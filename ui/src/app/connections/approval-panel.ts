import { MOD, withMod } from '../shared/shortcuts';
import { Tooltip } from '../shared/tooltip';
import { Select, SelectOption } from '../shared/select';
import { Component, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { isTauri } from '@tauri-apps/api/core';
import { BunkerAsk, BunkerConnection, CoreService, Remember, errorMessage } from '../core/core.service';
import { isSensitive, kindLabel } from './kinds';

const TITLES: Record<BunkerAsk['method'], string> = {
  sign_event: 'Sign this event?',
  nip04_encrypt: 'Encrypt a message?',
  nip44_encrypt: 'Encrypt a message?',
  nip04_decrypt: 'Decrypt a message?',
  nip44_decrypt: 'Decrypt a message?',
};

export const REMEMBER_CHOICES: { value: Remember; label: string; longer: boolean }[] = [
  { value: 'once', label: 'Only this time', longer: false },
  { value: 'fiveMinutes', label: 'For 5 minutes', longer: false },
  { value: 'oneHour', label: 'For 1 hour', longer: false },
  { value: 'oneDay', label: 'For 1 day', longer: true },
  { value: 'always', label: 'Always', longer: true },
];

/** An event dated this far ahead is flagged: it would sit at the top of feeds. */
const FUTURE_SLACK_SECS = 10 * 60;

/** Approve or Reject the oldest waiting bunker request. Fills the approval window. */
@Component({
  selector: 'app-approval-panel',
  imports: [FormsModule, Select, Tooltip],
  host: { '(document:keydown)': 'onKey($event)' },
  templateUrl: './approval-panel.html',
  styleUrl: './approval-panel.scss',
})
export class ApprovalPanel {
  readonly #core = inject(CoreService);
  readonly #connections = signal<BunkerConnection[]>([]);
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly remember = signal<Remember>('once');

  protected readonly ask = computed(() => this.#core.pendingAsks()[0] ?? null);
  protected readonly waiting = computed(() => this.#core.pendingAsks().length);
  protected readonly title = computed(() => {
    const ask = this.ask();
    return ask ? TITLES[ask.method] : '';
  });
  protected readonly appName = computed(() => {
    const ask = this.ask();
    const connection = this.#connections().find((c) => c.id === ask?.connectionId);
    return connection?.name ?? `App ${ask?.clientPubkey.slice(0, 8)}…`;
  });
  protected readonly identityNick = computed(() => {
    const ask = this.ask();
    return this.#core.identities().find((i) => i.pubkey === ask?.identityPubkey)?.nick ?? 'an identity';
  });
  protected readonly kindLabel = computed(() => kindLabel(this.ask()?.details.kind));
  /** Sensitive kinds are remembered for an hour at most. */
  protected readonly sensitive = computed(() => isSensitive(this.ask()?.details.kind));
  protected readonly rememberOptions = computed<SelectOption<Remember>[]>(() =>
    REMEMBER_CHOICES.map((c) => ({ value: c.value, label: c.label, disabled: this.sensitive() && c.longer })),
  );
  protected readonly futureDated = computed(() => {
    const createdAt = this.ask()?.details.createdAt;
    return createdAt !== null && createdAt !== undefined && createdAt > Date.now() / 1000 + FUTURE_SLACK_SECS;
  });
  protected readonly dated = computed(() => {
    const createdAt = this.ask()?.details.createdAt ?? 0;
    return new Date(createdAt * 1000).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
  });

  constructor() {
    effect(() => {
      // App names live with the connections; read them when a new request arrives.
      if (this.ask()) {
        untracked(() => void this.#loadConnections());
      }
    });
    effect(() => {
      // Each request starts at "Only this time".
      this.ask();
      untracked(() => this.remember.set('once'));
    });
  }

  protected readonly mod = MOD;

  /** Ctrl+Enter approves, Ctrl+Shift+Enter rejects. */
  onKey(event: KeyboardEvent): void {
    if (withMod(event) && (event.key === 'Enter' || event.code === 'NumpadEnter')) {
      event.preventDefault();
      void this.decide(!event.shiftKey);
    }
  }

  async decide(approve: boolean): Promise<void> {
    const ask = this.ask();
    if (!ask || this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    try {
      await this.#core.bunkerDecide(ask.key, approve, this.remember());
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }

  /** The approval window closes itself once nothing waits; this is for a request that vanished. */
  close(): void {
    if (isTauri()) {
      void getCurrentWindow().close();
    }
  }

  async #loadConnections(): Promise<void> {
    const info = await this.#core.bunkerInfo().catch(() => null);
    this.#connections.set(info?.connections ?? []);
  }
}
