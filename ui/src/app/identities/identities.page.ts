import { Tooltip } from '../shared/tooltip';
import { Component, computed, inject, signal } from '@angular/core';
import { RouterLink } from '@angular/router';
import { CoreService, errorMessage } from '../core/core.service';
import { colorForPubkey, shortNpub } from './appearance';
import { IdentityAvatar } from './identity-avatar';

@Component({
  selector: 'app-identities-page',
  imports: [RouterLink, IdentityAvatar, Tooltip],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <div class="page-head">
        <h1 class="page-title">Identities</h1>
        <button type="button" class="action primary" routerLink="/identities/new" data-action="new-identity">
          <i class="bi bi-plus-lg"></i> New identity
        </button>
      </div>

      @if (servedElsewhere().length) {
        <div class="sam-card served-card" role="note">
          <p class="title">{{ servedTitle() }}</p>
          <p class="copy">
            This desktop holds the keys too. If that desktop was this computer before a reset, or you no longer
            use it, sign here. The other desktop stops at its next backup.
          </p>
          @if (serveError()) {
            <p class="sam-error" role="alert">{{ serveError() }}</p>
          }
          <button type="button" class="action primary" [disabled]="serving()" (click)="serveHere()">
            Sign here
          </button>
        </div>
      }

      @if (siteIdentities().length && regular().length) {
        <div class="group-head">
          <button
            type="button"
            class="group-toggle"
            data-action="toggle-regular-identities"
            [attr.aria-expanded]="regularOpen()"
            (click)="regularOpen.set(!regularOpen())"
          >
            <i class="bi" [class.bi-chevron-right]="!regularOpen()" [class.bi-chevron-down]="regularOpen()"></i>
            <span>Regular identities</span>
          </button>
          <span class="count">{{ regular().length }}</span>
        </div>
      }
      @for (identity of (regularOpen() || !siteIdentities().length ? regular() : []); track identity.id) {
        <a
          class="identity"
          [routerLink]="['/identities', identity.id]"
          [style.border-left-color]="color(identity.pubkey, identity.color)"
        >
          <app-identity-avatar [pubkey]="identity.pubkey" [color]="identity.color" [size]="32" />
          <span class="nick">{{ identity.nick }}</span>
          @if (identity.keyMissing) {
            <span class="badge-bunker warn" appTooltip="No Gooti Desktop with your passphrase holds the key">
              Key missing
            </span>
          } @else if (identity.isBunker) {
            <span class="badge-bunker">Bunker</span>
          } @else if (identity.custody === 'desktop') {
            <span class="badge-bunker" appTooltip="The key stays on this desktop">Desktop only</span>
          }
          <span class="npub">{{ short(identity.npub) }}</span>
          <i class="bi bi-chevron-right"></i>
        </a>
      } @empty {
        @if (!siteIdentities().length && !regular().length) {
          <div class="sam-card empty-card">
            <p class="title">No identity yet</p>
            <p class="copy">Create one with a new key, or import a key you already have.</p>
          </div>
        }
      }

      @if (siteIdentities().length) {
        <div class="group-head">
          <button
            type="button"
            class="group-toggle"
            data-action="toggle-site-identities"
            [attr.aria-expanded]="siteOpen()"
            (click)="siteOpen.set(!siteOpen())"
          >
            <i class="bi" [class.bi-chevron-right]="!siteOpen()" [class.bi-chevron-down]="siteOpen()"></i>
            <span>Site-only identities</span>
          </button>
          <i
            class="bi bi-info-circle info"
            tabindex="0"
            aria-label="What are site-only identities?"
            appTooltip="Each identity was created with “New site-only identity” for a specific website to test that site without your regular identities. Delete them when you are done or choose to keep them as regular identities."
          ></i>
          <span class="count">{{ siteIdentities().length }}</span>
        </div>
        @if (siteOpen()) {
          @for (identity of siteIdentities(); track identity.id) {
            <a
              class="identity"
              [routerLink]="['/identities', identity.id]"
              [style.border-left-color]="color(identity.pubkey, identity.color)"
            >
              <app-identity-avatar [pubkey]="identity.pubkey" [color]="identity.color" [size]="32" />
              <span class="nick">{{ identity.nick }}</span>
              @if (identity.nick !== identity.site) {
                <span class="badge-bunker">{{ identity.site }}</span>
              }
              <span class="npub">{{ short(identity.npub) }}</span>
              <i class="bi bi-chevron-right"></i>
            </a>
          }
        }
      }
    </div>
  `,
  styles: `
    .identity {
      height: 56px;
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 0 14px 0 12px;
      background: var(--background-light);
      border-radius: 8px;
      border-left: 4px solid var(--primary);
      color: #ffffff;
      text-decoration: none;
      cursor: pointer;

      &:hover {
        background: var(--background-light-hover);
      }
    }

    .nick {
      flex: 1;
      min-width: 0;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      font-weight: 500;
    }

    .badge-bunker {
      color: var(--muted);
      font-size: 13px;
    }

    .badge-bunker.warn {
      color: #f0c36d;
    }

    .served-card {
      display: flex;
      flex-direction: column;
      align-items: flex-start;
      gap: 8px;
    }

    .npub {
      color: var(--muted);
      font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
      font-size: 13px;
    }

    .bi-chevron-right {
      color: var(--muted);
    }

    .group-head {
      display: flex;
      align-items: center;
      gap: 8px;
      margin-top: 8px;
      color: var(--muted);
      font-size: 14px;

      .count {
        margin-left: auto;
      }
    }

    .group-toggle {
      display: flex;
      align-items: center;
      gap: 8px;
      padding: 6px 2px;
      border: 0;
      background: none;
      color: inherit;
      font: inherit;
      font-weight: 500;
      text-align: left;
      cursor: pointer;

      &:hover {
        color: #ffffff;
      }

      .bi-chevron-right,
      .bi-chevron-down {
        color: inherit;
      }
    }

    .info {
      cursor: help;

      &:hover,
      &:focus-visible {
        color: #ffffff;
      }
    }
  `,
})
export class IdentitiesPage {
  protected readonly core = inject(CoreService);
  protected readonly color = colorForPubkey;
  protected readonly short = shortNpub;
  protected readonly serving = signal(false);
  /** Site-only identities: made in a browser with "New site-only identity". The group starts open. */
  protected readonly regular = computed(() => this.core.identities().filter((i) => !i.site));
  protected readonly siteIdentities = computed(() => this.core.identities().filter((i) => !!i.site));
  protected readonly siteOpen = signal(true);
  protected readonly regularOpen = signal(true);
  protected readonly serveError = signal('');
  protected readonly servedElsewhere = computed(() => this.core.identities().filter((i) => i.servedElsewhere));
  protected readonly servedTitle = computed(() => {
    const nicks = this.servedElsewhere().map((i) => i.nick);
    const names = nicks.length > 1 ? `${nicks.slice(0, -1).join(', ')} and ${nicks.at(-1)}` : nicks[0];
    return `${names} ${nicks.length > 1 ? 'sign' : 'signs'} on another Gooti Desktop`;
  });

  async serveHere(): Promise<void> {
    this.serving.set(true);
    this.serveError.set('');
    try {
      await this.core.serveHere(this.servedElsewhere().map((i) => i.id));
    } catch (error) {
      this.serveError.set(errorMessage(error));
    } finally {
      this.serving.set(false);
    }
  }
}
