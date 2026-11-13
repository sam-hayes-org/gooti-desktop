import { Component, effect, inject, input, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';
import { ActivityEntry, ConnectionDetail, CoreService, Rule, Trust, errorMessage } from '../core/core.service';
import { relaysFromText, relaysToText } from '../backup/relays';
import { ActivityList } from '../activity/activity-list';
import { activityOf } from '../activity/activity.page';
import { ConfirmService } from '../shared/confirm.service';
import { methodLabel } from './kinds';

/** How many of an app's entries its page shows. */
export const RECENT_ACTIVITY = 20;

export const TRUST_CHOICES: { value: Trust; label: string; copy: string }[] = [
  {
    value: 'ask',
    label: 'Ask every time',
    copy: 'Gooti asks before each signature, encryption, or decryption, unless you remembered a choice.',
  },
  {
    value: 'basic',
    label: 'Basic',
    copy:
      'Notes, reactions, messages and the like are approved. Changes to your profile, follow list, ' +
      'relays, deletions and sign-ins still ask.',
  },
  {
    value: 'trust',
    label: 'Trust',
    copy: 'Everything is approved without asking. Only for apps you fully trust.',
  },
];

/** One app: how much it may do without asking, and the choices remembered for it. */
@Component({
  selector: 'app-connection-page',
  imports: [RouterLink, FormsModule, ActivityList],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <a class="back" routerLink="/connections"><i class="bi bi-chevron-left"></i> Connections</a>
      @if (detail(); as detail) {
        <h1 class="page-title">{{ detail.name ?? 'App ' + detail.clientPubkey.slice(0, 8) + '…' }}</h1>
        <p class="copy">
          Signs as {{ detail.identityNick }}
          @if (detail.url) {
            · {{ detail.url }}
          }
        </p>
        @if (error()) {
          <p class="sam-error" role="alert">{{ error() }}</p>
        }

        @if (detail.servedElsewhere) {
          <section class="group served-elsewhere">
            <p class="kicker">Signing</p>
            <p class="copy">
              Another Gooti Desktop answers this app. This desktop holds the key of
              {{ detail.identityNick }} too. If that desktop was this computer before a reset, or you no
              longer use it, sign here. This desktop then answers every app that signs as
              {{ detail.identityNick }}.
            </p>
            <button type="button" class="action quiet" [disabled]="busy()" (click)="serveHere()">
              Sign here instead
            </button>
          </section>
        }

        <section class="group">
          <p class="kicker">Relays</p>
          @if (detail.browsers) {
            <p class="copy">
              Your browsers reach this desktop through these relays. Backup takes a change to them. One relay
              per line.
            </p>
            <textarea
              class="sam-box"
              rows="3"
              name="appRelays"
              aria-label="Relays for your browsers"
              [ngModel]="relaysText()"
              (ngModelChange)="relaysText.set($event)"
            ></textarea>
            <div class="relay-actions">
              <button type="button" class="action quiet" [disabled]="busy()" (click)="saveRelays()">
                Save relays
              </button>
              @if (notice()) {
                <span class="notice" role="status">{{ notice() }}</span>
              }
            </div>
          } @else {
            <p class="copy">
              This app reaches the desktop through these relays. It chose them when it connected.
            </p>
            <ul class="relay-list">
              @for (relay of detail.relays; track relay) {
                <li class="mono">{{ relay }}</li>
              }
            </ul>
          }
        </section>

        <section class="group">
          <p class="kicker">Without asking</p>
          @for (choice of choices; track choice.value) {
            <label class="choice-row">
              <input
                type="radio"
                name="trust"
                [value]="choice.value"
                [checked]="detail.trust === choice.value"
                [disabled]="busy()"
                (change)="setTrust(choice.value)"
              />
              <span class="choice-text">
                <span class="choice-label">{{ choice.label }}</span>
                <span class="copy">{{ choice.copy }}</span>
              </span>
            </label>
          }
          @if (detail.browsers && detail.trust === 'ask') {
            <p class="copy slow-sites" role="note">
              <i class="bi bi-exclamation-triangle"></i>
              Some sites, such as Primal, give up after about 8 seconds. Approve quickly there, or
              choose Basic.
            </p>
          }
          <p class="copy">A choice you remembered as Reject always wins.</p>
        </section>

        <section class="group">
          <p class="kicker">Recent activity</p>
          @if (recent().length) {
            <app-activity-list [entries]="recent()" />
            <a class="show-all" routerLink="/activity" [queryParams]="{ app: id() }">
              Show all in Activities <i class="bi bi-chevron-right"></i>
            </a>
          } @else {
            <p class="copy">Nothing yet. Requests from this app appear here.</p>
          }
        </section>

        <section class="group">
          <p class="kicker">Remembered</p>
          @for (rule of detail.rules; track rule.method + (rule.kind ?? '')) {
            <div class="sam-card rule-row">
              <div class="rule-text">
                <span class="rule-name">{{ label(rule) }}</span>
                <span class="copy">
                  {{ rule.policy === 'allow' ? 'Approve' : 'Reject' }} · {{ until(rule) }}
                </span>
              </div>
              <button type="button" class="action quiet" [disabled]="busy()" (click)="forget(rule)">Forget</button>
            </div>
          } @empty {
            <p class="copy">Nothing yet. Choices you remember in the request window appear here.</p>
          }
        </section>

        @if (detail.browsers) {
          <p class="copy">
            This is how your browsers reach {{ detail.identityNick }}, whose key stays on this
            desktop. To end it, use the identity in browsers again on its page.
          </p>
        } @else {
          <section class="group">
            <p class="kicker danger">Danger zone</p>
            <button type="button" class="action danger" [disabled]="busy()" (click)="remove()">Remove app</button>
          </section>
        }
      }
    </div>
  `,
  styles: `
    .slow-sites {
      color: #f0c36d;
    }

    .back {
      align-self: flex-start;
      color: var(--muted);
      font-size: 14px;
      text-decoration: none;
    }

    .back:hover {
      color: #ffffff;
    }

    .group {
      margin-top: 10px;
    }

    .rule-row {
      flex-direction: row;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
    }

    .rule-text {
      display: flex;
      flex-direction: column;
      gap: 2px;
    }

    .rule-name {
      font-weight: 500;
    }

    .show-all {
      align-self: flex-start;
      color: #c5dbff;
      font-size: 14px;
      text-decoration: none;

      &:hover {
        text-decoration: underline;
      }

      .bi {
        font-size: 12px;
      }
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

    .relay-list {
      margin: 0;
      padding: 0;
      list-style: none;

      li {
        color: var(--muted);
        font-family: ui-monospace, SFMono-Regular, Menlo, Consolas, monospace;
        font-size: 13px;
        line-height: 1.6;
      }
    }

    .served-elsewhere .action {
      align-self: flex-start;
    }

    .kicker.danger {
      margin-top: 30px;
      color: #dc3545;
    }

    .action.danger {
      align-self: flex-start;
    }
  `,
})
export class ConnectionPage {
  /** Route parameter `:id`. */
  readonly id = input.required<string>();
  readonly #core = inject(CoreService);
  readonly #router = inject(Router);
  readonly #confirm = inject(ConfirmService);
  protected readonly detail = signal<ConnectionDetail | null>(null);
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly notice = signal('');
  protected readonly relaysText = signal('');
  protected readonly choices = TRUST_CHOICES;
  /** This app's latest entries in Activities. */
  protected readonly recent = signal<ActivityEntry[]>([]);

  constructor() {
    effect(() => {
      const id = this.id();
      this.#core.activityChanges();
      void this.#core
        .activity()
        .then((entries) => this.recent.set(activityOf(entries, id).slice(0, RECENT_ACTIVITY)))
        .catch(() => this.recent.set([]));
    });
    effect(() => {
      const id = this.id();
      this.#core.bunkerChanges();
      void this.#core
        .bunkerConnection(id)
        .then((detail) => {
          this.detail.set(detail);
          this.relaysText.set(relaysToText(detail.relays));
        })
        .catch(() => this.#router.navigateByUrl('/connections'));
    });
  }

  protected label(rule: Rule): string {
    return methodLabel(rule.method, rule.kind);
  }

  protected until(rule: Rule): string {
    if (!rule.expiresAt) {
      return 'Always';
    }
    const time = new Date(rule.expiresAt).toLocaleString([], { dateStyle: 'medium', timeStyle: 'short' });
    return `Until ${time}`;
  }

  async setTrust(trust: Trust): Promise<void> {
    await this.#run(async () => this.detail.set(await this.#core.bunkerSetTrust(this.id(), trust)));
  }

  async forget(rule: Rule): Promise<void> {
    await this.#run(async () =>
      this.detail.set(await this.#core.bunkerForget(this.id(), rule.method, rule.kind ?? null)),
    );
  }

  async saveRelays(): Promise<void> {
    await this.#run(async () => {
      const relays = await this.#core.bunkerSetAppRelays(this.id(), relaysFromText(this.relaysText()));
      this.relaysText.set(relaysToText(relays));
      this.notice.set('Updated the relays');
    });
  }

  async serveHere(): Promise<void> {
    const detail = this.detail();
    if (!detail) {
      return;
    }
    await this.#run(async () => {
      await this.#core.serveHere([detail.identityId]);
      this.detail.set(await this.#core.bunkerConnection(this.id()));
      await this.#core.refreshBunker();
    });
  }

  async remove(): Promise<void> {
    if (!(await this.#confirm.ask('Remove this app?', { yesLabel: 'Remove', danger: true }))) {
      return;
    }
    await this.#run(async () => {
      await this.#core.bunkerRevoke(this.id());
      await this.#router.navigateByUrl('/connections');
    });
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
