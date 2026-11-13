import { Component, OnInit, computed, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { Router } from '@angular/router';
import { relaysFromText, relaysToText } from '../backup/relays';
import { CoreService, errorMessage } from '../core/core.service';

@Component({
  selector: 'app-identity-new-page',
  imports: [FormsModule],
  host: { class: 'page' },
  template: `
    <div class="page-inner">
      <h1 class="page-title">New identity</h1>

      <form class="sam-flex-column gap" (submit)="$event.preventDefault(); save()">
        <div class="sam-field">
          <input
            type="text"
            name="nick"
            placeholder="Nick"
            aria-label="Nick"
            autocomplete="off"
            [(ngModel)]="nick"
            autofocus
          />
        </div>

        <section class="group">
          <label class="choice-row">
            <input type="radio" name="keySource" value="generate" [(ngModel)]="source" />
            <span class="choice-text">
              <span class="choice-label">Generate private key</span>
              <span class="copy">A new key is made on this desktop. You can show or export it later.</span>
            </span>
          </label>
          <label class="choice-row">
            <input type="radio" name="keySource" value="import" [(ngModel)]="source" />
            <span class="choice-text">
              <span class="choice-label">Import private key</span>
              <span class="copy">Paste an nsec, an ncryptsec, or a hex key.</span>
            </span>
          </label>
        </section>

        @if (source() === 'import') {
          <div class="sam-field">
            <input
              #keyInput
              type="password"
              name="key"
              placeholder="Private key (nsec, ncryptsec or hex)"
              aria-label="Private key"
              autocomplete="off"
              [(ngModel)]="key"
            />
            <button
              class="icon"
              type="button"
              [attr.aria-label]="keyInput.type === 'password' ? 'Show key' : 'Hide key'"
              (click)="keyInput.type = keyInput.type === 'password' ? 'text' : 'password'"
            >
              <i
                class="bi"
                [class.bi-eye]="keyInput.type === 'password'"
                [class.bi-eye-slash]="keyInput.type === 'text'"
              ></i>
            </button>
          </div>
          @if (isNcryptsec()) {
            <div class="sam-field">
              <input
                type="password"
                name="keyPassword"
                placeholder="Password for this ncryptsec key"
                aria-label="Password for this ncryptsec key"
                autocomplete="off"
                [(ngModel)]="keyPassword"
              />
            </div>
          }
        }

        <label class="choice-row">
          <input
            type="checkbox"
            name="desktopOnly"
            [checked]="desktopOnly()"
            (change)="desktopOnly.set($any($event.target).checked)"
          />
          <span class="choice-text">
            <span class="choice-label">Keep the private key on this desktop only</span>
            <span class="copy">
              With backup on, your browsers get a (bunker) link to this desktop instead of the key, and sign through it.
            </span>
          </span>
        </label>

        @if (desktopOnly()) {
          <section class="group relays">
            <label class="field-label" for="browserRelays">Relays for your browsers</label>
            <p class="copy">
              Browsers will reach this desktop through these relays. One relay per line.
            </p>
            <textarea
              id="browserRelays"
              class="sam-box"
              rows="3"
              name="browserRelays"
              [ngModel]="relaysText()"
              (ngModelChange)="relaysText.set($event)"
            ></textarea>
          </section>
        }

        @if (error()) {
          <p class="sam-error" role="alert">{{ error() }}</p>
        }

        <div class="form-actions">
          <button type="button" class="action quiet" (click)="cancel()">Cancel</button>
          <button type="submit" class="action primary" [disabled]="!canSave() || busy()">
            Save
          </button>
        </div>
      </form>
    </div>
  `,
})
export class IdentityNewPage implements OnInit {
  readonly #core = inject(CoreService);
  readonly #router = inject(Router);
  protected readonly nick = signal('');
  protected readonly source = signal<'generate' | 'import'>('generate');
  protected readonly key = signal('');
  protected readonly keyPassword = signal('');
  /** New identities stay on this desktop unless the user says otherwise. */
  protected readonly desktopOnly = signal(true);
  protected readonly relaysText = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal('');

  protected readonly isNcryptsec = computed(() =>
    this.key().trim().toLowerCase().startsWith('ncryptsec1'),
  );

  protected readonly canSave = computed(() => {
    if (!this.nick().trim()) {
      return false;
    }
    if (this.source() === 'generate') {
      return true;
    }
    return !!this.key().trim() && (!this.isNcryptsec() || !!this.keyPassword());
  });

  async save(): Promise<void> {
    if (!this.canSave() || this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    try {
      const imported = this.source() === 'import';
      const created = await this.#core.createIdentity(
        this.nick(),
        imported ? this.key() : undefined,
        imported && this.isNcryptsec() ? this.keyPassword() : undefined,
        this.desktopOnly() ? 'desktop' : 'shared',
        this.desktopOnly() ? relaysFromText(this.relaysText()) : undefined,
      );
      this.key.set('');
      this.keyPassword.set('');
      await this.#router.navigate(['/identities', created.id]);
    } catch (error) {
      this.error.set(errorMessage(error));
    } finally {
      this.busy.set(false);
    }
  }

  async ngOnInit(): Promise<void> {
    await this.#core.refreshBunker();
    this.relaysText.set(relaysToText(this.#core.bunkerState()?.relays ?? []));
  }

  cancel(): void {
    void this.#router.navigateByUrl('/identities');
  }
}
