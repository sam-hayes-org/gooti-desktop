import { Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { relaysFromText, relaysToText } from '../backup/relays';
import { CoreService, errorMessage } from '../core/core.service';
import { CopyButton } from '../shared/copy-button';
import { Select, SelectOption } from '../shared/select';
import { QrCode } from './qr-code';

export const CONNECTED_NOTICE = 'The app is connected.';

/** "Connect new app": a bunker link to show, or a nostrconnect link to paste. */
@Component({
  selector: 'app-connection-new-page',
  imports: [FormsModule, CopyButton, QrCode, Select],
  host: { class: 'page' },
  templateUrl: './connection-new.page.html',
  styleUrl: './connection-new.page.scss',
})
export class ConnectionNewPage implements OnInit {
  protected readonly core = inject(CoreService);
  readonly #router = inject(Router);

  protected readonly identityId = signal('');
  protected readonly mode = signal<'link' | 'paste' | null>(null);
  protected readonly link = signal('');
  protected readonly pasted = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  /** Set when a clicked link filled in the page. */
  protected readonly fromLink = signal(false);
  protected readonly relaysText = signal('');
  /** The apps there were when the link was shown. A new one for the identity means it connected. */
  #before: Set<string> | null = null;

  /** Identities this desktop can sign for. Bunkers from a browser cannot connect apps here. */
  protected readonly signable = computed(() => this.core.identities().filter((i) => i.hasPrivateKey));
  protected readonly identityOptions = computed<SelectOption<string>[]>(() =>
    this.signable().map((i) => ({ value: i.id, label: i.nick })),
  );

  constructor() {
    effect(() => {
      // A nostrconnect:// link clicked in a browser or another app. Connecting stays the user's step.
      const link = this.core.connectLink();
      if (link) {
        untracked(() => {
          this.core.connectLink.set(null);
          this.showPaste();
          this.pasted.set(link);
          this.fromLink.set(true);
        });
      }
    });
    effect(() => {
      // Pick the first identity once the list is there.
      const first = this.signable()[0];
      if (first && !untracked(this.identityId)) {
        this.identityId.set(first.id);
      }
    });
    effect(() => {
      // The bunker link was used: the core reloads the apps when one connects.
      const connections = this.core.bunkerState()?.connections ?? [];
      const before = this.#before;
      const identityId = untracked(this.identityId);
      if (before && connections.some((c) => c.identityId === identityId && !before.has(c.id))) {
        this.#before = null;
        this.back(CONNECTED_NOTICE);
      }
    });
  }

  async ngOnInit(): Promise<void> {
    await this.core.refreshBunker();
    this.relaysText.set(relaysToText(this.core.bunkerState()?.relays ?? []));
  }

  async showLink(): Promise<void> {
    this.fromLink.set(false);
    await this.#run(async () => {
      await this.core.refreshBunker();
      this.#before = new Set((this.core.bunkerState()?.connections ?? []).map((c) => c.id));
      this.link.set(await this.core.bunkerLink(this.identityId(), relaysFromText(this.relaysText())));
      this.mode.set('link');
    });
  }

  showPaste(): void {
    this.error.set('');
    this.link.set('');
    this.#before = null;
    this.mode.set('paste');
  }

  async connect(): Promise<void> {
    await this.#run(async () => {
      await this.core.bunkerConnect(this.pasted(), this.identityId());
      this.pasted.set('');
      this.back(CONNECTED_NOTICE);
    });
  }

  back(notice?: string): void {
    void this.#router.navigateByUrl('/connections', notice ? { state: { notice } } : {});
  }

  async #run(work: () => Promise<void>): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
