import { Component, inject, signal } from '@angular/core';
import { CoreService, InfoLink, errorMessage } from '../core/core.service';
import { CopyButton } from '../shared/copy-button';
import { SHORTCUTS } from '../shared/shortcuts';

/** The author's Nostr key, as on the extension's Info tab. */
export const AUTHOR_NPUB = 'npub1tgyjshvelwj73t3jy0n3xllgt03elkapfl3k3n0x2wkunegkgrwssfp0u4';

/** Version and links, like the extension's Info tab. */
@Component({
  selector: 'app-info-page',
  imports: [CopyButton],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <h1 class="page-title">Info</h1>

      @if (error()) {
        <p class="sam-error" role="alert">{{ error() }}</p>
      }

      <section class="group">
        <p class="kicker">Version</p>
        <p class="copy" data-info="version">
          @if (core.appInfo(); as info) {
            {{ info.version }} ({{ info.channel }})
          } @else {
            Unknown
          }
        </p>
      </section>

      <section class="group">
        <p class="kicker">Website</p>
        <button type="button" class="link" data-link="website" (click)="open('website')">www.getgooti.com</button>
      </section>

      <section class="group">
        <p class="kicker">Source code</p>
        <button type="button" class="link" data-link="source" (click)="open('source')">
          github.com/sam-hayes-org/gooti-desktop
        </button>
      </section>

      <section class="group">
        <p class="kicker">Keyboard shortcuts</p>
        <dl class="shortcuts">
          @for (shortcut of shortcuts; track shortcut.keys) {
            <dt><kbd>{{ shortcut.keys }}</kbd></dt>
            <dd>{{ shortcut.does }}</dd>
          }
        </dl>
      </section>

      <div class="sam-card author">
        <span>
          Made with <i class="bi bi-heart-fill heart"></i> by
          <button type="button" class="link" data-link="author" (click)="open('author')">Sam Hayes</button>
        </span>
        <div class="sam-field">
          <input type="text" class="mono" aria-label="Sam Hayes's public key (npub)" readonly [value]="npub" />
          <app-copy-button [text]="npub" />
        </div>
      </div>
    </div>
  `,
  styles: `
    .link {
      align-self: flex-start;
      padding: 0;
      border: 0;
      background: none;
      color: #c5dbff;
      font: inherit;
      font-size: 14px;
      cursor: pointer;

      &:hover {
        text-decoration: underline;
      }
    }

    .shortcuts {
      display: grid;
      grid-template-columns: max-content 1fr;
      gap: 6px 16px;
      margin: 0;

      dt,
      dd {
        margin: 0;
        color: var(--muted);
        font-size: 14px;
      }

      kbd {
        padding: 2px 6px;
        border-radius: 6px;
        background: var(--background-light);
        color: #ffffff;
        font-family: inherit;
        font-size: 13px;
      }
    }

    .author {
      display: flex;
      flex-direction: column;
      gap: 10px;
      margin-top: 18px;
    }

    .heart {
      color: #dc3545;
    }
  `,
})
export class InfoPage {
  protected readonly core = inject(CoreService);
  protected readonly npub = AUTHOR_NPUB;
  protected readonly shortcuts = SHORTCUTS;
  protected readonly error = signal('');

  async open(link: InfoLink): Promise<void> {
    this.error.set('');
    try {
      await this.core.openLink(link);
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }
}
