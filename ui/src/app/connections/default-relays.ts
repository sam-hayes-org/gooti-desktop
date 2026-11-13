import { Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { relaysFromText, relaysToText } from '../backup/relays';
import { CoreService, errorMessage } from '../core/core.service';

/** Settings → "Default relays for apps": fills new links and new Desktop only identities. */
@Component({
  selector: 'app-default-relays',
  imports: [FormsModule],
  template: `
    <section class="group">
      <p class="kicker">Default relays for apps</p>
      <p class="copy">
        New bunker links and new Desktop only identities start with these relays. Changing them changes nothing
        that is already set up. One relay per line.
      </p>
      <textarea
        class="sam-box"
        rows="3"
        name="defaultRelays"
        aria-label="Default relays for apps"
        [ngModel]="text()"
        (ngModelChange)="text.set($event)"
      ></textarea>
      <div class="relay-actions">
        <button type="button" class="action quiet" [disabled]="busy()" (click)="save()">Save relays</button>
        @if (notice()) {
          <span class="notice" role="status">{{ notice() }}</span>
        }
      </div>
      @if (error()) {
        <p class="sam-error" role="alert">{{ error() }}</p>
      }
    </section>
  `,
  styles: `
    :host {
      display: contents;
    }

    .relay-actions {
      display: flex;
      align-items: center;
      gap: 12px;
    }

    .notice {
      color: #b6e3c4;
      font-size: 14px;
    }
  `,
})
export class DefaultRelays implements OnInit {
  readonly #core = inject(CoreService);
  protected readonly text = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly notice = signal('');

  async ngOnInit(): Promise<void> {
    await this.#core.refreshBunker();
    this.text.set(relaysToText(this.#core.bunkerState()?.relays ?? []));
  }

  async save(): Promise<void> {
    this.busy.set(true);
    this.error.set('');
    this.notice.set('');
    try {
      this.text.set(relaysToText(await this.#core.bunkerSetRelays(relaysFromText(this.text()))));
      this.notice.set('Updated the relays');
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }
}
