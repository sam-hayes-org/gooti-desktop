import { Select, SelectOption } from '../shared/select';
import { Component, OnInit, inject, signal } from '@angular/core';
import { FormsModule } from '@angular/forms';
import {
  AppSettingsChange,
  CoreService,
  IdentitySummary,
  LockSettingsChange,
  Settings,
  errorMessage,
} from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { CopyButton } from '../shared/copy-button';
import { Dialog } from '../shared/dialog';
import { DefaultRelays } from '../connections/default-relays';
import { MIN_PASSWORD_LENGTH } from '../vault/vault-start';

/** "Lock after" choices. Must match `gooti_core::AUTO_LOCK_CHOICES`. */
export const AUTO_LOCK_CHOICES: { minutes: number | null; label: string }[] = [
  { minutes: null, label: 'Never' },
  { minutes: 5, label: '5 minutes' },
  { minutes: 15, label: '15 minutes' },
  { minutes: 60, label: '1 hour' },
  { minutes: 240, label: '4 hours' },
];

@Component({
  selector: 'app-settings-page',
  imports: [FormsModule, Select, Dialog, CopyButton, DefaultRelays],
  host: { class: 'page' },
  templateUrl: './settings.page.html',
  styleUrl: './settings.page.scss',
})
export class SettingsPage implements OnInit {
  readonly #core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);

  protected readonly lockOptions: SelectOption<number | null>[] = AUTO_LOCK_CHOICES.map((c) => ({
    value: c.minutes,
    label: c.label,
  }));
  protected readonly settings = signal<Settings | null>(null);
  protected readonly error = signal('');

  protected readonly currentPassword = signal('');
  protected readonly newPassword = signal('');
  protected readonly passwordBusy = signal(false);
  protected readonly passwordError = signal('');
  protected readonly passwordDone = signal(false);
  protected readonly minPasswordLength = MIN_PASSWORD_LENGTH;

  /** The reset dialog, shown when Desktop only identities would lose their keys. */
  protected readonly resetAsk = signal<{ desktopOnly: IdentitySummary[]; backupOn: boolean } | null>(null);
  protected readonly resetKeys = signal<Record<string, string>>({});
  protected readonly resetBackedUp = signal(false);
  protected readonly resetBusy = signal(false);
  protected readonly resetError = signal('');

  protected readonly autostart = signal(false);
  protected readonly hasTray = signal(true);
  protected readonly checking = signal(false);
  protected readonly updateNote = signal('');

  async ngOnInit(): Promise<void> {
    this.settings.set(await this.#core.settings());
    const autostart = await this.#core.autostart().catch(() => null);
    if (autostart) {
      this.autostart.set(autostart.enabled);
      this.hasTray.set(autostart.hasTray);
    }
  }

  async changeApp(change: AppSettingsChange): Promise<void> {
    this.error.set('');
    try {
      this.settings.set(await this.#core.changeAppSettings(change));
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }

  async setAutostart(box: HTMLInputElement): Promise<void> {
    this.error.set('');
    try {
      const info = await this.#core.setAutostart(box.checked);
      this.autostart.set(info.enabled);
    } catch (error) {
      this.error.set(errorMessage(error));
    }
    box.checked = this.autostart();
  }

  async checkNow(): Promise<void> {
    this.checking.set(true);
    this.updateNote.set('');
    try {
      const update = await this.#core.checkForUpdate();
      this.updateNote.set(update ? `Gooti ${update.version} is available.` : 'Gooti is up to date.');
    } catch (error) {
      this.updateNote.set(errorMessage(error));
    } finally {
      this.checking.set(false);
    }
  }

  async setUnlockAtLogin(box: HTMLInputElement): Promise<void> {
    this.error.set('');
    try {
      this.settings.set(await this.#core.setUnlockAtLogin(box.checked));
    } catch (error) {
      this.error.set(errorMessage(error));
      // The stored value did not change, so the binding would not reset the box by itself.
      const stored = await this.#core.settings();
      this.settings.set(stored);
      box.checked = stored.unlockAtLogin;
    }
  }

  async changeLock(change: LockSettingsChange): Promise<void> {
    this.error.set('');
    try {
      this.settings.set(await this.#core.changeLockSettings(change));
    } catch (error) {
      this.error.set(errorMessage(error));
    }
  }

  protected canChangePassword(): boolean {
    return !!this.currentPassword() && this.newPassword().length >= MIN_PASSWORD_LENGTH;
  }

  async changePassword(): Promise<void> {
    if (!this.canChangePassword() || this.passwordBusy()) {
      return;
    }
    this.passwordBusy.set(true);
    this.passwordError.set('');
    this.passwordDone.set(false);
    try {
      await this.#core.changePassword(this.currentPassword(), this.newPassword());
      this.currentPassword.set('');
      this.newPassword.set('');
      this.passwordDone.set(true);
    } catch (error) {
      this.passwordError.set(errorMessage(error));
    } finally {
      this.passwordBusy.set(false);
    }
  }

  /**
   * Reset deletes the keys of Desktop only identities, and only the desktop record brings them
   * back. When the backup does not have them yet, a dialog names them and offers to back up or
   * show the keys first.
   */
  async reset(): Promise<void> {
    const desktopOnly = this.#desktopOnly();
    const backup = desktopOnly.length ? await this.#core.backupInfo().catch(() => null) : null;
    if (desktopOnly.length === 0 || backup?.desktopCurrent) {
      const yes = await this.#confirm.ask('Reset Gooti? All data on this desktop will be deleted.', {
        yesLabel: 'Reset',
        danger: true,
      });
      if (yes) {
        await this.#core.reset();
      }
      return;
    }
    this.resetError.set('');
    this.resetKeys.set({});
    this.resetBackedUp.set(false);
    this.resetAsk.set({ desktopOnly, backupOn: backup !== null });
  }

  /** Sends the backup, so a restore brings the keys back. They stay Desktop only. */
  async backUpFirst(): Promise<void> {
    const ask = this.resetAsk();
    if (!ask) {
      return;
    }
    await this.#resetStep(async () => {
      await this.#core.backUpNow();
      if (!(await this.#core.backupInfo())?.desktopCurrent) {
        throw { code: 'desktop_behind', message: 'Some changes are not in your backup yet. Try again.' };
      }
      this.resetAsk.set({ ...ask, desktopOnly: [] });
      this.resetBackedUp.set(true);
    });
  }

  async showResetKeys(): Promise<void> {
    const ask = this.resetAsk();
    if (!ask) {
      return;
    }
    await this.#resetStep(async () => {
      const keys: Record<string, string> = {};
      for (const identity of ask.desktopOnly) {
        keys[identity.id] = await this.#core.revealNsec(identity.id);
      }
      this.resetKeys.set(keys);
    });
  }

  async resetNow(): Promise<void> {
    await this.#resetStep(async () => {
      await this.#core.reset();
      this.closeReset();
    });
  }

  closeReset(): void {
    this.resetKeys.set({});
    this.resetAsk.set(null);
  }

  #desktopOnly(): IdentitySummary[] {
    return this.#core.identities().filter((i) => i.custody === 'desktop' && i.hasPrivateKey);
  }

  async #resetStep(step: () => Promise<void>): Promise<void> {
    this.resetBusy.set(true);
    this.resetError.set('');
    try {
      await step();
    } catch (error) {
      this.resetError.set(errorMessage(error));
    } finally {
      this.resetBusy.set(false);
    }
  }
}
