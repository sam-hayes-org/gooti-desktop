import { Component, inject, signal } from '@angular/core';
import { CoreService, errorCode, errorMessage } from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { PasswordField } from './password-field';

@Component({
  selector: 'app-vault-unlock',
  imports: [PasswordField],
  template: `
    <div class="stage">
      <div class="decision">
        <div class="brand">
          <img src="gooti.svg" width="18" height="18" alt="" />
          <span>Gooti</span>
        </div>
        <div class="body">
          <h1>Unlock Gooti</h1>
          <form (submit)="$event.preventDefault(); unlock()">
            <app-password-field [(value)]="password" [autofocus]="true" />
            @if (error()) {
              <p class="error" role="alert">{{ error() }}</p>
            }
            <button class="submit" type="submit" [disabled]="!password() || busy()">
              Unlock
            </button>
          </form>
          <button type="button" class="reset" (click)="reset()">Reset Gooti</button>
        </div>
      </div>
    </div>
  `,
  styleUrl: './vault-screen.scss',
})
export class VaultUnlock {
  readonly #core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);
  protected readonly password = signal('');
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  #errorTimer?: ReturnType<typeof setTimeout>;

  async unlock(): Promise<void> {
    if (!this.password() || this.busy()) {
      return;
    }
    this.busy.set(true);
    try {
      await this.#core.unlock(this.password());
    } catch (error) {
      // Same as the extension: the invalid password note goes away after a moment.
      this.error.set(errorCode(error) === 'invalid_password' ? 'Invalid password' : errorMessage(error));
      clearTimeout(this.#errorTimer);
      this.#errorTimer = setTimeout(() => this.error.set(''), 2000);
    } finally {
      this.busy.set(false);
    }
  }

  async reset(): Promise<void> {
    const yes = await this.#confirm.ask('Reset Gooti? All data on this desktop will be deleted.', {
      yesLabel: 'Reset',
      danger: true,
    });
    if (yes) {
      await this.#core.reset();
    }
  }
}
