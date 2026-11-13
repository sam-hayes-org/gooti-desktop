import { Component, OnInit, computed, effect, inject, signal, untracked } from '@angular/core';
import { FormsModule } from '@angular/forms';
import {
  CoreService,
  RelayFailure,
  BackupInfo,
  BackupOutcome,
  errorMessage,
  relayFailures,
} from '../core/core.service';
import { ConfirmService } from '../shared/confirm.service';
import { CopyButton } from '../shared/copy-button';
import { Dialog } from '../shared/dialog';
import { SYNC_CONTENT_LIMIT, formatVaultSize, relaysFromText, relaysToText } from './relays';

type DialogState =
  | { kind: 'none' }
  | { kind: 'phrase'; phrase: string }
  | { kind: 'restore' }
  | { kind: 'run'; running: boolean; outcome?: BackupOutcome; error?: string; failures?: RelayFailure[] };

const RESULT_TEXT: Record<BackupOutcome['result'], string> = {
  backedUp: 'Backed up',
  updated: 'Updated',
  current: 'Already up to date.',
};

@Component({
  selector: 'app-backup-page',
  imports: [FormsModule, CopyButton, Dialog],
  host: { class: 'page' },
  templateUrl: './backup.page.html',
  styleUrl: './backup.page.scss',
})
export class BackupPage implements OnInit {
  protected readonly core = inject(CoreService);
  readonly #confirm = inject(ConfirmService);

  /** `undefined` while loading, `null` while backup is off. */
  protected readonly info = signal<BackupInfo | null | undefined>(undefined);
  protected readonly relaysText = signal('');
  protected readonly phraseInput = signal('');
  protected readonly savedWords = signal(false);
  protected readonly busy = signal(false);
  protected readonly error = signal('');
  /** Each relay's reason, when every relay failed. */
  readonly #failures = signal<RelayFailure[]>([]);
  protected readonly failures = this.#failures.asReadonly();
  protected readonly notice = signal('');
  protected readonly dialog = signal<DialogState>({ kind: 'none' });
  protected readonly resultText = RESULT_TEXT;

  protected readonly vaultUsed = computed(() => formatVaultSize(this.info()?.contentChars ?? 0));
  protected readonly vaultOver = computed(() => (this.info()?.contentChars ?? 0) > SYNC_CONTENT_LIMIT);
  protected readonly vaultPercent = computed(() => {
    const chars = this.info()?.contentChars ?? 0;
    return chars <= 0 ? 0 : Math.min(100, (chars / SYNC_CONTENT_LIMIT) * 100);
  });
  protected readonly lastAuto = computed(() => {
    const activity = this.core.lastAutoBackup();
    if (!activity) {
      return '';
    }
    const time = new Date(activity.at).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    return 'error' in activity
      ? `Automatic backup failed at ${time}: ${activity.error}`
      : `Automatic backup at ${time}: ${RESULT_TEXT[activity.result]}`;
  });

  constructor() {
    // An automatic backup can change the size or the relays: reload when the core says so.
    effect(() => {
      if (this.core.changes() > 0) {
        untracked(() => void this.reload());
      }
    });
  }

  async ngOnInit(): Promise<void> {
    await this.reload();
  }

  async reload(): Promise<void> {
    const info = await this.core.backupInfo();
    this.info.set(info);
    this.relaysText.set(relaysToText(info?.relays ?? (await this.core.defaultRelays())));
  }

  // Backup off

  async createPassphrase(): Promise<void> {
    this.error.set('');
    this.savedWords.set(false);
    this.dialog.set({ kind: 'phrase', phrase: await this.core.generatePassphrase() });
  }

  async turnOn(phrase: string): Promise<void> {
    await this.#run(async () => {
      this.info.set(await this.core.enableBackup(phrase, relaysFromText(this.relaysText())));
      this.dialog.set({ kind: 'none' });
      this.notice.set('Backup is on. Press Back up now to send your identities.');
    });
  }

  openRestore(): void {
    this.error.set('');
    this.phraseInput.set('');
    this.dialog.set({ kind: 'restore' });
  }

  async restore(): Promise<void> {
    await this.#run(async () => {
      this.info.set(await this.core.joinBackup(this.phraseInput(), relaysFromText(this.relaysText())));
      this.phraseInput.set('');
      this.dialog.set({ kind: 'none' });
      this.notice.set('Merged the backup with this desktop.');
    });
  }

  closeDialog(): void {
    if (!this.busy()) {
      this.phraseInput.set('');
      this.dialog.set({ kind: 'none' });
    }
  }

  // Backup on

  async backUpNow(): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    this.notice.set('');
    this.dialog.set({ kind: 'run', running: true });
    try {
      // Like the extension, Back up now uses the relays in the box.
      this.info.set(await this.core.setBackupRelays(relaysFromText(this.relaysText())));
      const outcome = await this.core.backUpNow();
      this.dialog.set({ kind: 'run', running: false, outcome });
      this.info.set(await this.core.backupInfo());
    } catch (error) {
      this.dialog.set({
        kind: 'run',
        running: false,
        error: errorMessage(error),
        failures: relayFailures(error),
      });
    } finally {
      this.busy.set(false);
    }
  }

  async setAutoBackup(box: HTMLInputElement): Promise<void> {
    await this.#run(async () => {
      this.info.set(await this.core.setAutoBackup(box.checked));
    });
    box.checked = this.info()?.autoBackup ?? false;
  }

  async saveRelays(): Promise<void> {
    await this.#run(async () => {
      const info = await this.core.setBackupRelays(relaysFromText(this.relaysText()));
      this.info.set(info);
      this.relaysText.set(relaysToText(info?.relays ?? []));
      this.notice.set('Updated the relays');
    });
  }

  async turnOff(): Promise<void> {
    const yes = await this.#confirm.ask('Turn off backup on this desktop?', { yesLabel: 'Turn off' });
    if (!yes) {
      return;
    }
    await this.#run(async () => {
      await this.core.disableBackup();
      this.notice.set('');
      await this.reload();
    });
  }

  async #run(work: () => Promise<void>): Promise<void> {
    if (this.busy()) {
      return;
    }
    this.busy.set(true);
    this.error.set('');
    this.#failures.set([]);
    try {
      await work();
    } catch (error) {
      this.error.set(errorMessage(error));
      this.#failures.set(relayFailures(error));
    } finally {
      this.busy.set(false);
    }
  }
}
