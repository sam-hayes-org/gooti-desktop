import { Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { RouterLink } from '@angular/router';
import { BunkerAsk, CoreService, Trust, errorMessage } from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { methodLabel } from './kinds';

export const TRUST_LABELS: Record<Trust, string> = {
  ask: 'Asks every time',
  basic: 'Basic',
  trust: 'Trusted',
};

@Component({
  selector: 'app-connections-page',
  imports: [FormsModule, RouterLink],
  host: { class: 'page' },
  templateUrl: './connections.page.html',
  styleUrl: './connections.page.scss',
})
export class ConnectionsPage implements OnInit {
  protected readonly core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);

  /** Live: the core reloads it when an app or a relay changes. */
  protected readonly info = this.core.bunkerState;
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly notice = signal('');

  async ngOnInit(): Promise<void> {
    // "Connect new app" comes back here with a note once the app connected.
    const notice = (history.state as { notice?: unknown } | null)?.notice;
    if (typeof notice === 'string') {
      this.notice.set(notice);
    }
    await this.reload();
  }

  async reload(): Promise<void> {
    await this.core.refreshBunker();
  }

  /** Every app is answered by another Gooti Desktop, so this one listens on nothing. */
  protected allElsewhere(): boolean {
    const connections = this.info()?.connections ?? [];
    return connections.length > 0 && connections.every((c) => c.servedElsewhere);
  }

  /** Who uses a relay: the apps on it, and a link waiting for an app. */
  protected usedBy(url: string): string {
    const info = this.info();
    if (!info) {
      return '';
    }
    const names = [...new Set(info.connections.filter((c) => c.relays.includes(url)).map((c) => this.appName(c)))];
    if (info.linkRelays.includes(url)) {
      names.push('a new link');
    }
    return names.join(', ');
  }

  async remove(id: string): Promise<void> {
    if (!(await this.#confirm.ask('Remove this app?', { yesLabel: 'Remove', danger: true }))) {
      return;
    }
    await this.#run(async () => {
      await this.core.bunkerRevoke(id);
      await this.reload();
    });
  }

  async setPaused(box: HTMLInputElement): Promise<void> {
    await this.#run(async () => {
      await this.core.bunkerSetPaused(box.checked);
      await this.reload();
    });
    box.checked = this.info()?.paused ?? false;
  }

  protected readonly trustLabel: Record<Trust, string> = TRUST_LABELS;

  protected askLabel(ask: BunkerAsk): string {
    const app = this.info()?.connections.find((c) => c.id === ask.connectionId);
    return `${app ? this.appName(app) : 'An app'}: ${methodLabel(ask.method, ask.details.kind)}`;
  }

  async decide(key: string, approve: boolean): Promise<void> {
    await this.#run(() => this.core.bunkerDecide(key, approve));
  }

  protected appName(connection: { name: string | null; clientPubkey: string }): string {
    return connection.name ?? `App ${connection.clientPubkey.slice(0, 8)}…`;
  }

  protected when(ms: number | null): string {
    return ms ? new Date(ms).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' }) : 'Never';
  }

  async #run(work: () => Promise<void>): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    this.notice.set('');
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
