import { Component, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import { CoreService, RelayFailure, errorMessage, relayFailures } from '../core/core.service';
import { relaysFromText, relaysToText } from '../backup/relays';
import { PasswordField } from './password-field';

/** Minimum Gooti password length. Same rule as the extension and `gooti_core::MIN_PASSWORD_LENGTH`. */
export const MIN_PASSWORD_LENGTH = 4;

type Step = 'home' | 'create' | 'restore';

/** No vault yet: the first screen, then "Choose a password" or "Restore from backup". */
@Component({
  selector: 'app-vault-start',
  imports: [FormsModule, PasswordField],
  template: `
    <div class="stage">
      @switch (step()) {
        @case ('home') {
          <div class="decision">
            <div class="brand large">
              <div class="logo"><img src="gooti.svg" width="76" height="76" alt="" /></div>
              <span>Gooti</span>
            </div>
            <div class="body">
              <h1>Your identities start here.</h1>
              <button type="button" class="choice" data-choice="create" (click)="go('create')">
                <strong>Set up Gooti</strong>
                <span>and start fresh</span>
              </button>
              <button type="button" class="choice" data-choice="restore" (click)="go('restore')">
                <strong>Restore from backup</strong>
                <span>on your relays, with your passphrase</span>
              </button>
            </div>
          </div>
        }
        @case ('create') {
          <div class="decision">
            <div class="brand">
              <img src="gooti.svg" width="18" height="18" alt="" />
              <span>Gooti</span>
            </div>
            <div class="body">
              <h1>Choose a password</h1>
              <p class="hint">Sensitive data is encrypted before it is stored.</p>
              <form (submit)="$event.preventDefault(); create()">
                <app-password-field [(value)]="password" [autofocus]="true" />
                @if (error()) {
                  <p class="error" role="alert">{{ error() }}</p>
                }
                <button class="submit" type="submit" [disabled]="!canCreate() || busy()">
                  Set up Gooti
                </button>
              </form>
              <button type="button" class="quiet" (click)="go('home')">Back</button>
            </div>
          </div>
        }
        @case ('restore') {
          <div class="decision">
            <div class="brand">
              <img src="gooti.svg" width="18" height="18" alt="" />
              <span>Gooti</span>
            </div>
            <div class="body">
              <h1>Restore from backup</h1>
              <p class="hint">
                Enter the 12 words you wrote down when you turned on backup. The password is only for this
                desktop. Nothing is stored when the relays have no backup for these words.
              </p>
              <form (submit)="$event.preventDefault(); restore()">
                <app-password-field [(value)]="password" />
                <textarea
                  class="box"
                  rows="3"
                  name="phrase"
                  placeholder="12 words"
                  aria-label="12 words"
                  autocomplete="off"
                  [ngModel]="phrase()"
                  (ngModelChange)="phrase.set($event)"
                ></textarea>
                <p class="label">Relays</p>
                <textarea
                  class="box"
                  rows="2"
                  name="relays"
                  aria-label="Relays"
                  [ngModel]="relaysText()"
                  (ngModelChange)="relaysText.set($event)"
                ></textarea>
                @if (error()) {
                  <p class="error" role="alert">{{ error() }}</p>
                  @for (failure of failures(); track failure.url) {
                    <p class="failure"><span class="url">{{ failure.url }}</span> {{ failure.reason }}</p>
                  }
                }
                <button class="submit" type="submit" [disabled]="!canRestore() || busy()">
                  {{ busy() ? 'Restoring…' : 'Restore from backup' }}
                </button>
              </form>
              <button type="button" class="quiet" [disabled]="busy()" (click)="go('home')">Back</button>
            </div>
          </div>
        }
      }
    </div>
  `,
  styleUrl: './vault-screen.scss',
  styles: `
    .box {
      display: block;
      width: 100%;
      box-sizing: border-box;
      margin-top: 10px;
      padding: 10px 14px;
      border: 1px solid rgba(255, 255, 255, 0.06);
      border-radius: 12px;
      background: var(--background-light);
      color: #ffffff;
      font: inherit;
      font-size: 14px;
      resize: vertical;
      user-select: text;
    }

    .box::placeholder {
      color: var(--muted);
    }

    .label {
      margin: 12px 0 0;
      color: var(--muted);
      font-size: 13px;
    }

    .failure {
      margin: 4px 0 0;
      color: var(--muted);
      font-size: 13px;

      .url {
        color: #e8edf5;
      }
    }
  `,
})
export class VaultStart {
  readonly #core = inject(CoreService);
  protected readonly step = signal<Step>('home');
  protected readonly password = signal('');
  protected readonly phrase = signal('');
  protected readonly relaysText = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  protected readonly failures = signal<RelayFailure[]>([]);

  protected canCreate(): boolean {
    return this.password().length >= MIN_PASSWORD_LENGTH;
  }

  protected canRestore(): boolean {
    return this.canCreate() && !!this.phrase().trim() && relaysFromText(this.relaysText()).length > 0;
  }

  async go(step: Step): Promise<void> {
    this.password.set('');
    this.phrase.set('');
    this.error.set('');
    this.failures.set([]);
    this.step.set(step);
    if (step === 'restore') {
      this.relaysText.set(relaysToText(await this.#core.defaultRelays().catch(() => [])));
    }
  }

  async create(): Promise<void> {
    if (!this.canCreate() || this.busy()) {
      return;
    }
    await this.#run(() => this.#core.createVault(this.password()));
  }

  async restore(): Promise<void> {
    if (!this.canRestore() || this.busy()) {
      return;
    }
    await this.#run(() =>
      this.#core.restoreVault(this.password(), this.phrase(), relaysFromText(this.relaysText())),
    );
  }

  async #run(work: () => Promise<void>): Promise<void> {
    this.busy.set(true);
    this.error.set('');
    this.failures.set([]);
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
      this.failures.set(relayFailures(error));
    } finally {
      this.busy.set(false);
    }
  }
}
