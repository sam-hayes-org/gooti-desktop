import { Component, OnDestroy, effect, inject, input, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { relaysFromText, relaysToText } from '../backup/relays';
import { CoreService, IdentitySummary, errorMessage } from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { CopyButton } from '../shared/copy-button';
import { colorForPubkey } from './appearance';
import { IdentityAvatar } from './identity-avatar';

@Component({
  selector: 'app-identity-detail-page',
  imports: [FormsModule, RouterLink, IdentityAvatar, CopyButton],
  host: { class: 'page' },
  templateUrl: './identity-detail.page.html',
  styleUrl: './identity-detail.page.scss',
})
export class IdentityDetailPage implements OnDestroy {
  /** Route parameter `:id`. */
  readonly id = input.required<string>();

  readonly #core = inject(CoreService);
  readonly #router = inject(Router);
  readonly #confirm = inject(ConfirmService);

  protected readonly identity = signal<IdentitySummary | null>(null);
  protected readonly nick = signal('');
  protected readonly nsec = signal<string | null>(null);
  protected readonly exportPassword = signal('');
  protected readonly exported = signal<string | null>(null);
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  /** "Move key to desktop" is open, with the relays for the browsers. */
  protected readonly moving = signal(false);
  protected readonly relaysText = signal('');
  protected readonly exportError = signal('');

  constructor() {
    effect(() => {
      void this.load(this.id());
    });
  }

  ngOnDestroy(): void {
    // Nothing secret stays on screen after leaving the page.
    this.nsec.set(null);
    this.exported.set(null);
  }

  protected color(identity: IdentitySummary): string {
    return colorForPubkey(identity.pubkey, identity.color);
  }

  async load(id: string): Promise<void> {
    this.nsec.set(null);
    this.exported.set(null);
    this.error.set('');
    try {
      const identity = await this.#core.identity(id);
      this.identity.set(identity);
      this.nick.set(identity.nick);
    } catch {
      await this.#router.navigateByUrl('/identities');
    }
  }

  protected nickChanged(): boolean {
    const identity = this.identity();
    return !!identity && !!this.nick().trim() && this.nick().trim() !== identity.nick;
  }

  async saveNick(): Promise<void> {
    await this.#run(async (identity) => {
      this.identity.set(await this.#core.renameIdentity(identity.id, this.nick()));
    });
  }

  async setColor(color: string): Promise<void> {
    await this.#run(async (identity) => {
      this.identity.set(await this.#core.setIdentityColor(identity.id, color));
    });
  }

  async showNsec(): Promise<void> {
    await this.#run(async (identity) => {
      this.nsec.set(await this.#core.revealNsec(identity.id));
    });
  }

  async startMove(): Promise<void> {
    await this.#core.refreshBunker();
    this.relaysText.set(relaysToText(this.#core.bunkerState()?.relays ?? []));
    this.moving.set(true);
  }

  async moveToDesktop(): Promise<void> {
    await this.#run(async (identity) => {
      this.identity.set(
        await this.#core.setCustody(identity.id, 'desktop', relaysFromText(this.relaysText())),
      );
      this.moving.set(false);
    });
  }

  /** The other desktop stops answering for this identity at its next backup. */
  async serveHere(): Promise<void> {
    await this.#run(async (identity) => {
      await this.#core.serveHere([identity.id]);
      this.identity.set(await this.#core.identity(identity.id));
    });
  }

  async useInBrowsers(): Promise<void> {
    const yes = await this.#confirm.ask(
      'Use this key in browsers again? This puts the private key back on every browser and desktop with your passphrase.',
      { yesLabel: 'Use in browsers', danger: true },
    );
    if (yes) {
      await this.#run(async (identity) => {
        this.identity.set(await this.#core.setCustody(identity.id, 'shared'));
      });
    }
  }

  hideNsec(): void {
    this.nsec.set(null);
  }

  async exportKey(): Promise<void> {
    const identity = this.identity();
    if (!identity || this.busy()) {
      return;
    }
    this.busy.set(true);
    this.exportError.set('');
    try {
      this.exported.set(await this.#core.exportNcryptsec(identity.id, this.exportPassword()));
      this.exportPassword.set('');
    } catch (error) {
      this.exportError.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }

  async delete(): Promise<void> {
    const identity = this.identity();
    if (!identity) {
      return;
    }
    const yes = await this.#confirm.ask('Delete this identity?', { yesLabel: 'Delete', danger: true });
    if (!yes) {
      return;
    }
    await this.#run(async () => {
      await this.#core.deleteIdentity(identity.id);
      await this.#router.navigateByUrl('/identities');
    });
  }

  async #run(work: (identity: IdentitySummary) => Promise<void>): Promise<void> {
    const identity = this.identity();
    if (!identity || this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    try {
      await work(identity);
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
